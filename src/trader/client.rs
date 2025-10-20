#![allow(unused)]

use crate::{
    error::TradingError, model::{position::{Position, PositionStatus}, token::Token}, perp::{backpack::BackpackClient, PerpExchange}, storage::{storage_position::PositionStorage, storage_strategy::{StrategyMetadata, StrategyStorage}}, trader::{
        strategy::TradingStrategy, wallet::Wallet
    }
};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rand::seq::SliceRandom;
use sqlx::PgPool;

pub struct TraderClient {
    wallet: Wallet,
    position_storage: PositionStorage,
    strategy_storage: StrategyStorage,
}

impl TraderClient {
    /// Create a new trader client with a database pool
    pub async fn new(wallet_id: u8, pool: PgPool) -> Result<Self, TradingError> {
        let wallet = Wallet::load_from_json(wallet_id)?;
        let position_storage = PositionStorage::new(pool.clone()).await?;
        let strategy_storage = StrategyStorage::new(pool).await?;

        Ok(Self { 
            wallet, 
            position_storage,
            strategy_storage,
        })
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
    /// - Fetches USDC balance from each wallet
    /// - Randomly selects a token to trade
    /// - Generates balanced long/short allocations (market neutral)
    /// - Opens positions on all wallets
    /// - Returns a TradingStrategy tracking all positions
    pub async fn farm_points_on_backpack_from_multiple_wallets(
        wallets: Vec<Wallet>,
        pool: PgPool,
    ) -> Result<TradingStrategy, TradingError> {
        if wallets.is_empty() {
            return Err(TradingError::InvalidInput("No wallets provided".into()));
        }

        if wallets.len() < 3 {
            return Err(TradingError::InvalidInput("At least 3 wallets are required".into()));
        }

        info!("🎯 Starting farming strategy with {} wallets", wallets.len());

        // Step 1: Fetch USDC balances from all wallets
        let mut wallet_balances = Vec::new();
        for wallet in &wallets {
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
            let wallet = wallets
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
        let strategy_storage = StrategyStorage::new(pool.clone()).await?;
        strategy_storage.save_strategy(&strategy).await?;
        
        // Save all positions
        let position_storage = PositionStorage::new(pool).await?;
        for position in strategy.longs.iter().chain(strategy.shorts.iter()) {
            position_storage.save_position(position).await?;
        }
        
        info!("✅ Strategy {} executed successfully!", strategy_id);
        info!("   Token: {}", strategy.token_symbol);
        info!("   Long positions: {} | Total size: {}", strategy.longs.len(), strategy.longs_size);
        info!("   Short positions: {} | Total size: {}", strategy.shorts.len(), strategy.shorts_size);
        info!("   Close at: {}", strategy.close_at);
        
        Ok(strategy)
    }

    pub fn get_supported_tokens(&self) -> Vec<Token> {
        Token::get_supported_tokens()
    }

    // ===== Position Storage Methods =====
    /// Save or update a position in storage
    pub async fn save_position(&self, position: &Position) -> Result<(), TradingError> {
        self.position_storage.save_position(position).await
    }

    /// Get a specific position by ID
    pub async fn get_position(&self, id: &str) -> Result<Option<Position>, TradingError> {
        self.position_storage.get_position(id).await
    }

    /// Get all stored positions
    pub async fn get_all_positions(&self) -> Result<Vec<Position>, TradingError> {
        self.position_storage.get_all_positions().await
    }

    /// Get positions for a specific exchange
    pub async fn get_positions_by_exchange(&self, exchange: crate::model::exchange::Exchange) -> Result<Vec<Position>, TradingError> {
        self.position_storage.get_positions_by_exchange(exchange).await
    }

    /// Get all active (Open or Closing) positions
    pub async fn get_active_positions(&self) -> Result<Vec<Position>, TradingError> {
        self.position_storage.get_active_positions().await
    }

    /// Update a position's status and related fields
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
    pub async fn delete_position(&self, id: &str) -> Result<(), TradingError> {
        self.position_storage.delete_position(id).await
    }

    // ===== Strategy Storage Methods =====
    /// Save or update a strategy in storage
    pub async fn save_strategy(&self, strategy: &TradingStrategy) -> Result<(), TradingError> {
        self.strategy_storage.save_strategy(strategy).await
    }

    /// Get all active strategies
    pub async fn get_active_strategies(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        self.strategy_storage.get_active_strategies().await
    }

    /// Get all strategies
    pub async fn get_all_strategies(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        self.strategy_storage.get_all_strategies().await
    }
}
