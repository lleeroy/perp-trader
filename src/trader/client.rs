
use crate::{
    error::TradingError, 
    model::{position::{Position, PositionStatus}, 
    token::Token}, 
    perp::{backpack::BackpackClient, PerpExchange}, 
    storage::{storage_position::PositionStorage, storage_strategy::{StrategyMetadata, StrategyStorage}}, 
    trader::{strategy::TradingStrategy, wallet::Wallet}
};

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rand::seq::SliceRandom;
use sqlx::PgPool;
use tokio::time::{sleep, Duration};


pub struct TraderClient {
    wallets: Vec<Wallet>,
    position_storage: PositionStorage,
    strategy_storage: StrategyStorage,
}

impl TraderClient {
    /// Create a new trader client with a database pool
    pub async fn new(wallet_ids: Vec<u8>, pool: PgPool) -> Result<Self, TradingError> {

        if wallet_ids.is_empty() {
            return Err(TradingError::InvalidInput("No wallet IDs provided".into()));
        }

        if wallet_ids.len() < 3 {
            return Err(TradingError::InvalidInput("At least 3 wallets are required".into()));
        }

        let wallets = wallet_ids.iter()
            .map(|id| Wallet::load_from_json(*id))
            .collect::<Result<Vec<Wallet>, TradingError>>()?;

        let position_storage = PositionStorage::new(pool.clone()).await?;
        let strategy_storage = StrategyStorage::new(pool).await?;

        Ok(Self { 
            wallets, 
            position_storage,
            strategy_storage,
        })
    }

    /// Monitor and close strategies when they reach their close_at time
    ///
    /// # Arguments
    /// * `strategies` - Vector of strategy metadata to monitor
    ///
    /// # Returns
    /// * `Ok(())` - All strategies were successfully closed
    /// * `Err(TradingError)` - If any error occurs during monitoring/closing
    async fn monitor_and_close_strategies(
        &self,
        strategies: Vec<StrategyMetadata>,
    ) -> Result<(), TradingError> {
        if strategies.is_empty() {
            return Err(TradingError::InvalidInput("No strategies provided".into()));
        }

        // Find the earliest close time across all strategies
        let earliest_close = strategies.iter()
            .map(|s| s.close_at)
            .min()
            .unwrap_or_else(Utc::now);

        // Display strategies being monitored
        info!("📋 Monitoring {} strategies:", strategies.len());
        for strategy in &strategies {
            info!("  🎯 Strategy {} | Token: {} | Wallets: {:?} | Close at: {}", 
                strategy.id, strategy.token_symbol, strategy.wallet_ids, strategy.close_at);
        }

        // Wait until the earliest close time
        let now = Utc::now();
        if earliest_close > now {
            let wait_duration = (earliest_close - now).to_std()
                .unwrap_or(Duration::from_secs(0));
            
            info!("⏳ Waiting {} seconds until first strategy close time...", wait_duration.as_secs());
            sleep(wait_duration).await;
        }

        // Close each strategy
        for strategy in strategies {
            // Check if it's time to close this strategy
            let now = Utc::now();
            if strategy.close_at > now {
                let remaining = (strategy.close_at - now).to_std()
                    .unwrap_or(Duration::from_secs(0));
                if remaining.as_secs() > 0 {
                    info!("⏳ Waiting {} seconds before closing strategy {}...", 
                        remaining.as_secs(), strategy.id);
                    sleep(remaining).await;
                }
            }

            info!("🔄 Closing strategy {} ({})", strategy.id, strategy.token_symbol);
            
            // Update strategy status to Closing
            self.strategy_storage.update_strategy_status(
                &strategy.id,
                crate::trader::strategy::StrategyStatus::Closing,
                None,
                None,
            ).await?;

            // Get all positions for this strategy
            let all_position_ids = strategy.get_all_position_ids();
            let mut closed_positions = Vec::new();
            let mut has_failures = false;

            // Close each position in the strategy
            for position_id in all_position_ids {
                if let Some(position) = self.position_storage.get_position(&position_id).await? {
                    // Find the wallet for this position
                    let wallet = self.wallets.iter().find(|w| w.id == position.wallet_id)
                        .ok_or_else(|| TradingError::InvalidInput(
                            format!("Wallet #{} not found for position {}", position.wallet_id, position.id)
                        ))?;

                    // Create client and close position
                    let client = BackpackClient::new(wallet);
                    
                    match client.close_position(&position).await {
                        Ok(closed_position) => {
                            // Update position in database
                            self.position_storage.update_position_status(
                                &closed_position.id,
                                PositionStatus::Closed,
                                closed_position.closed_at,
                                closed_position.realized_pnl,
                            ).await?;
                            
                            if let Some(pnl) = closed_position.realized_pnl {
                                closed_positions.push((closed_position.id.clone(), pnl));
                                info!("  ✅ Position {} closed | PnL: {:.2}", closed_position.id, pnl);
                            } else {
                                info!("  ✅ Position {} closed | PnL: N/A", closed_position.id);
                            }
                        }
                        Err(e) => {
                            error!("  ❌ Failed to close position {}: {}", position.id, e);
                            has_failures = true;

                            // Mark position as failed but continue with other positions
                            self.position_storage.update_position_status(
                                &position.id,
                                PositionStatus::Failed,
                                Some(Utc::now()),
                                None,
                            ).await?;
                        }
                    }
                }
            }

            // Calculate total PnL for the strategy
            let total_pnl: Decimal = closed_positions.iter()
                .map(|(_, pnl)| pnl)
                .sum();

            // Update strategy status based on whether there were failures
            let final_status = if has_failures {
                crate::trader::strategy::StrategyStatus::Failed
            } else {
                crate::trader::strategy::StrategyStatus::Closed
            };

            self.strategy_storage.update_strategy_status(
                &strategy.id,
                final_status,
                Some(Utc::now()),
                Some(total_pnl),
            ).await?;

            info!("💰 Strategy {} completed | Status: {} | Total PnL: {:.2} USDC", 
                strategy.id, final_status, total_pnl);
        }

        Ok(())
    }

    /// Farm points on Backpack using multiple wallets with balanced long/short positions
    /// 
    /// # Arguments
    /// * `wallets` - Vector of at least 3 wallets to use for trading
    /// * `pool` - PostgreSQL connection pool
    /// 
    /// # Returns
    /// * `Ok(TradingStrategy)` - The executed trading strategy with all positions
    /// * `Err(TradingError)` - If any error occurs during execution
    /// 
    /// # Strategy
    /// - First checks if any wallets are involved in active strategies
    /// - If active strategies found: waits for them to complete before proceeding
    /// - Then checks if any wallets have orphaned active positions
    /// - For wallets with existing positions: monitors them until close time
    /// - For wallets without positions: creates new trades
    /// - Fetches USDC balance from each available wallet
    /// - Randomly selects a token to trade
    /// - Generates balanced long/short allocations (market neutral)
    /// - Opens positions on available wallets
    /// - Returns a TradingStrategy tracking all positions
    pub async fn farm_points_on_backpack_from_multiple_wallets(
        &self,
    ) -> Result<TradingStrategy, TradingError> {
        info!("🎯 Starting farming strategy with {} wallets", self.wallets.len());
        let active_strategies = self.strategy_storage.get_active_strategies().await?;
        
        // Filter strategies that involve any of our wallet IDs
        let conflicting_strategies: Vec<StrategyMetadata> = active_strategies
            .into_iter()
            .filter(|strategy| {
                strategy.wallet_ids.iter().any(|id| self.wallets.iter().any(|w| w.id == *id))
            })
            .collect();

        if !conflicting_strategies.is_empty() {
            info!("⚠️  Found {} active strategies involving the provided wallets", conflicting_strategies.len());
            info!("📋 Waiting for these strategies to complete before starting new trades...");
            
            // Monitor and close all conflicting strategies
            self.monitor_and_close_strategies(conflicting_strategies).await?;

            // After closing, we need to call the function again to start new trades
            return Err(TradingError::InvalidInput(
                "Active strategies were completed. Please call the function again to create new trades.".into()
            ));
        }

        info!("✅ No active strategies found for these wallets.");

        // Step 1: Fetch USDC balances from all wallets
        let mut wallet_balances = Vec::new();
        for wallet in &self.wallets {
            let client = BackpackClient::new(wallet);
            let balance = client.get_usdc_balance().await?;
            
            if balance <= Decimal::ZERO {
                return Err(TradingError::InvalidInput(
                    format!("Wallet #{} has insufficient USDC balance: {}", wallet.id, balance)
                ));
            }
            
            info!("💰 Wallet #{}: {:.2} USDC", wallet.id, balance);
            wallet_balances.push((wallet.id, balance));
        }

        // Step 2: Generate balanced long/short allocations
        let allocations = TradingStrategy::generate_balanced_allocations(wallet_balances)?;

        // Step 3: Randomly select a token to trade
        let supported_tokens = Token::get_supported_tokens();
        let mut rng = rand::thread_rng();
        let selected_token = supported_tokens
            .choose(&mut rng)
            .ok_or_else(|| TradingError::InvalidInput("No tokens available".into()))?
            .clone();
        
        info!("🎲 Selected token: {}", selected_token.symbol);

        // Step 4: Open positions for each allocation
        let mut long_positions = Vec::new();
        let mut short_positions = Vec::new();
        
        for allocation in allocations {
            // Find the wallet for this allocation
            let wallet = self.wallets
                .iter()
                .find(|w| w.id == allocation.wallet_id)
                .ok_or_else(|| TradingError::InvalidInput(
                    format!("Wallet #{} not found", allocation.wallet_id)
                ))?;
            
            let client = BackpackClient::new(wallet);
            
            // Open position
            let position = client
                .open_position(
                    selected_token.clone(),
                    allocation.side,
                    allocation.usdc_amount,
                )
                .await?;
            
            // Store position based on side
            match allocation.side {
                crate::model::PositionSide::Long => long_positions.push(position),
                crate::model::PositionSide::Short => short_positions.push(position),
            }
        }

        // Step 5: Build the trading strategy
        let mut strategy = TradingStrategy::build_from_positions(
            selected_token.symbol.clone(),
            long_positions, 
            short_positions
        )?;
        
        // Step 6: Link positions to strategy and save everything
        let strategy_id = strategy.id.clone();
        
        // Update all positions with the strategy_id
        for position in strategy.longs.iter_mut().chain(strategy.shorts.iter_mut()) {
            position.strategy_id = Some(strategy_id.clone());
        }
        
        // Save strategy first
        self.strategy_storage.save_strategy(&strategy).await?;
        
        info!("✅ Strategy {} executed successfully!", strategy_id);
        info!("   Token: {}", strategy.token_symbol);
        info!("   Long positions: {} | Total size: {}", strategy.longs.len(), strategy.longs_size);
        info!("   Short positions: {} | Total size: {}", strategy.shorts.len(), strategy.shorts_size);
        info!("   Close at: {}", strategy.close_at);
        
        Ok(strategy)
    }


    #[allow(unused)]
    pub fn get_supported_tokens(&self) -> Vec<Token> {
        Token::get_supported_tokens()
    }

    // ===== Position Storage Methods =====
    /// Save or update a position in storage
    #[allow(unused)]
    pub async fn save_position(&self, position: &Position) -> Result<(), TradingError> {
        self.position_storage.save_position(position).await
    }

    /// Get a specific position by ID
    #[allow(unused)]
    pub async fn get_position(&self, id: &str) -> Result<Option<Position>, TradingError> {
        self.position_storage.get_position(id).await
    }

    /// Get all stored positions
    #[allow(unused)]
    pub async fn get_all_positions(&self) -> Result<Vec<Position>, TradingError> {
        self.position_storage.get_all_positions().await
    }

    /// Get positions for a specific exchange
    #[allow(unused)]
    pub async fn get_positions_by_exchange(&self, exchange: crate::model::exchange::Exchange) -> Result<Vec<Position>, TradingError> {
        self.position_storage.get_positions_by_exchange(exchange).await
    }

    /// Get all active (Open or Closing) positions
    #[allow(unused)]
    pub async fn get_active_positions(&self) -> Result<Vec<Position>, TradingError> {
        self.position_storage.get_active_positions().await
    }

    /// Update a position's status and related fields
    #[allow(unused)]
    pub async fn update_position_status(
        &self,
        id: &str,
        status: PositionStatus,
        closed_at: Option<DateTime<Utc>>,
        realized_pnl: Option<Decimal>,
    ) -> Result<(), TradingError> {
        self.position_storage.update_position_status(id, status, closed_at, realized_pnl).await
    }

    /// Delete a position from storage (use sparingly)
    #[allow(unused)]
    pub async fn delete_position(&self, id: &str) -> Result<(), TradingError> {
        self.position_storage.delete_position(id).await
    }

    // ===== Strategy Storage Methods =====
    /// Save or update a strategy in storage
    #[allow(unused)]
    pub async fn save_strategy(&self, strategy: &TradingStrategy) -> Result<(), TradingError> {
        self.strategy_storage.save_strategy(strategy).await
    }

    /// Get all active strategies
    #[allow(unused)]
    pub async fn get_active_strategies(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        self.strategy_storage.get_active_strategies().await
    }

    /// Get all strategies
    #[allow(unused)]
    pub async fn get_all_strategies(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        self.strategy_storage.get_all_strategies().await
    }
}
