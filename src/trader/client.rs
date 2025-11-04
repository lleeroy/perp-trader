/// A comprehensive trading client that manages multiple wallets, positions, and strategies
/// across different exchanges (Backpack, Lighter).
/// 
/// # Features
/// - Multi-wallet management with balanced position allocation
/// - Cross-exchange trading (Backpack and Lighter)
/// - Strategy-based position management with automatic closing
/// - Real-time monitoring and error handling
/// - Persistent storage for positions and strategies
/// - Telegram alerts for strategy failures

use crate::{
    alert::telegram::TelegramAlerter, error::TradingError, model::{
		position::{Position, PositionSide, PositionStatus},
		token::Token, Exchange,
	}, perp::{backpack::BackpackClient, lighter::client::LighterClient, PerpExchange}, storage::{
		storage_position::PositionStorage,
		storage_strategy::{StrategyMetadata, StrategyStorage},
	}, trader::{strategy::{StrategyStatus, TradingStrategy}, wallet::{Wallet, WalletTradingClient}}
};

use chrono::{DateTime, Duration, Utc};
use rust_decimal::{Decimal};
use rand::{seq::SliceRandom, Rng};
use rust_decimal_macros::dec;
use sqlx::PgPool;
use tokio::time::{sleep, Duration as TokioDuration};
use colored::*;


const MAX_ATTEMPTS: usize = 10;

pub struct TraderClient {
    pub wallets: Vec<Wallet>,
    position_storage: PositionStorage,
    strategy_storage: StrategyStorage,
    wallet_trading_clients: Vec<WalletTradingClient>,   
}

impl TraderClient {
    /// Create a new trader client with specified wallets and database connection
    /// 
    /// # Arguments
    /// * `wallet_ids` - Vector of wallet IDs to load (must have at least 3 wallets)
    /// * `pool` - PostgreSQL connection pool for data persistence
    /// 
    /// # Returns
    /// * `Ok(Self)` - Successfully initialized trader client
    /// * `Err(TradingError)` - If wallet loading fails or insufficient wallets provided
    /// 
    /// # Errors
    /// * `TradingError::InvalidInput` - If no wallet IDs provided or fewer than 3 wallets
    /// * `TradingError::WalletError` - If wallet configuration files cannot be loaded
    /// * `TradingError::StorageError` - If database connections cannot be established
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

        let wallet_trading_clients = futures::future::try_join_all(
            wallets.iter().cloned().map(WalletTradingClient::new)
        ).await?;

        Ok(Self { 
            wallets, 
            position_storage,
            strategy_storage,
            wallet_trading_clients,
        })
    }



    /// Immediately close all active strategies and their positions
    /// 
    /// This method is used for emergency shutdown or manual intervention.
    /// It will:
    /// 1. Find all active strategies
    /// 2. Set their status to "Closing"
    /// 3. Attempt to close all positions on Lighter
    /// 4. Update strategy status to "Closed" (success) or "Failed" (errors)
    /// 5. Send Telegram alerts for any failures
    pub async fn close_all_active_strategies(&self) -> Result<(), TradingError> {
        let strategies = self.get_active_strategies().await?;

        for strategy in strategies {
            self.set_strategy_status(&strategy.id, 
                StrategyStatus::Closing, 
                None, None)
            .await?;


            match self.close_positions_on_lighter_for_wallets_group(&strategy.wallet_ids).await {
                Ok(_) => {
                    info!("✅ Strategy {} closed successfully", strategy.id);
                    self.set_strategy_status(&strategy.id, StrategyStatus::Closed, Some(Utc::now()), None).await?;
                }
                Err(e) => {
                    error!("{}", format!("❌ Failed to close all positions: {} | YOU NEED TO CLOSE THE POSITIONS MANUALLY!", e).on_red());
                    self.set_strategy_status(&strategy.id, StrategyStatus::Failed, Some(Utc::now()), None).await?;

                    let alerter = TelegramAlerter::new();                    
                    if let Err(e) = alerter.send_strategy_error_alert(&strategy, &e).await {
                        error!("{}", format!("❌ Failed to send strategy error alert: {}", e).on_red());
                    }
                }
            }
        }

        Ok(())
    }

    /// Monitor strategies and automatically close them when their close time is reached
    /// or if any position is within 15% of liquidation
    /// 
    /// This method provides automated strategy lifecycle management by:
    /// 1. Continuously monitoring liquidation levels for ALL active strategies
    /// 2. Closing strategies immediately if liquidation risk is detected in ANY strategy
    /// 3. Closing strategies when their scheduled close time arrives
    /// 4. Handling failures with appropriate status updates and alerts
    /// 
    /// # Arguments
    /// * `strategies` - Vector of strategy metadata to monitor and close
    /// 
    /// # Returns
    /// * `Ok(())` - All strategies were processed (successfully or marked as failed)
    /// * `Err(TradingError)` - If input validation fails or critical errors occur
    /// 
    /// # Behavior
    /// - ALL strategies are monitored for liquidation continuously
    /// - Liquidation checks are performed every 30 seconds across all strategies
    /// - Strategies are closed at their scheduled times or immediately on liquidation risk
    /// - Failed closures are marked and alerted but don't stop other strategies
    /// - Local time display is adjusted to UTC+8 for user convenience
    pub async fn monitor_and_close_strategies(
        &self,
        strategies: Vec<StrategyMetadata>,
    ) -> Result<(), TradingError> {
        if strategies.is_empty() {
            return Err(TradingError::InvalidInput("No strategies provided".into()));
        }

        // Display strategies being monitored
        info!("📋 Monitoring {} strategies:", strategies.len());
        for strategy in &strategies {
            let close_at_local = strategy.close_at + chrono::Duration::hours(8);
            info!("🎯 Strategy {} | Token: {} | Wallets: {:?} | Close at: {}", 
                strategy.id, strategy.token_symbol, strategy.wallet_ids, close_at_local.format("%H:%M"));
        }

        // Track which strategies are still active
        let mut active_strategies: Vec<StrategyMetadata> = strategies.clone();
        
        const CHECK_INTERVAL_SECS: u64 = 30;
        const LIQUIDATION_THRESHOLD: Decimal = dec!(15.0);

        // Main monitoring loop - continues until all strategies are closed
        while !active_strategies.is_empty() {
            let now = Utc::now();
            
            // Check liquidation levels for ALL active strategies in parallel
            info!("🔍 Checking liquidation levels for {} active strategies...", active_strategies.len());
            
            let mut strategies_to_close = Vec::new();
            
            // Create futures for all liquidation checks
            let check_futures = active_strategies.iter().map(|strategy| {
                let strategy_clone = strategy.clone();
                async move {
                    // Check if it's time to close this strategy normally
                    if Utc::now() >= strategy_clone.close_at {
                        return (strategy_clone, Some((Decimal::ZERO, false, true))); // (percentage, is_emergency, is_scheduled)
                    }
                    
                    // Check liquidation levels
                    match self.check_strategy_liquidation_levels(&strategy_clone).await {
                        Ok(Some(min_percentage)) => {
                            (strategy_clone, Some((min_percentage, min_percentage < LIQUIDATION_THRESHOLD, false)))
                        }
                        Ok(None) => {
                            (strategy_clone, None)
                        }
                        Err(e) => {
                            warn!("⚠️ Failed to check liquidation for strategy {}: {}", strategy_clone.id, e);
                            (strategy_clone, None)
                        }
                    }
                }
            });
            
            // Execute all checks in parallel
            let check_results = futures::future::join_all(check_futures).await;
            
            // Process results
            for (strategy, result) in check_results {
                if let Some((min_percentage, is_emergency, is_scheduled)) = result {
                    if is_scheduled {
                        info!("⏰ Strategy {} has reached its scheduled close time", strategy.id);
                        strategies_to_close.push((strategy, false)); // false = normal close
                    } else if is_emergency {
                        error!(
                            "⚠️ {} Position in strategy {} is within {:.2}% of liquidation (threshold: {:.2}%)",
                            "CRITICAL:".on_red().bold(),
                            strategy.id,
                            min_percentage,
                            LIQUIDATION_THRESHOLD
                        );
                        strategies_to_close.push((strategy, true)); // true = emergency close
                    } else if min_percentage < Decimal::from(20) {
                        warn!(
                            "⚠️ Strategy {} has positions within {:.2}% of liquidation",
                            strategy.id, min_percentage
                        );
                    }
                }
            }
            
            // Close strategies that need closing
            for (strategy, is_emergency) in strategies_to_close {
                if is_emergency {
                    warn!("🚨 Initiating EMERGENCY close for strategy {} due to liquidation risk", strategy.id);
                } else {
                    info!("🔄 Closing strategy {} ({}) - scheduled time reached", strategy.id, strategy.token_symbol);
                }
                
                // Update strategy status to Closing
                self
                    .set_strategy_status(&strategy.id, StrategyStatus::Closing, None, None)
                    .await?;

                let mut has_failures = false;
                match self.close_positions_on_lighter_for_wallets_group(&strategy.wallet_ids).await {
                    Ok(_) => {
                        if is_emergency {
                            info!("✅ Emergency close successful for strategy {}", strategy.id);
                        } else {
                            info!("✅ All positions closed successfully for strategy {}", strategy.id);
                        }
                    }
                    Err(e) => {
                        error!("❌ Failed to close positions for strategy {}: {}", strategy.id, e);
                        has_failures = true;
                        let strategy_clone = strategy.clone();

                        tokio::spawn(async move {
                            let alerter = TelegramAlerter::new();
                            
                            if let Err(e) = alerter.send_strategy_error_alert(&strategy_clone, &e).await {
                                error!("{}", format!("❌ Failed to send strategy error alert: {}", e).on_red());
                            }
                        });
                    }
                }

                // Update strategy status based on whether there were failures
                let final_status = if has_failures { StrategyStatus::Failed } else { StrategyStatus::Closed };

                self
                    .set_strategy_status(&strategy.id, final_status, Some(Utc::now()), None)
                    .await?;

                if is_emergency {
                    info!("💰 Strategy {} EMERGENCY closed | Status: {}", strategy.id, final_status);
                } else {
                    info!("💰 Strategy {} completed | Status: {}", strategy.id, final_status);
                }
                
                // Remove from active strategies
                active_strategies.retain(|s| s.id != strategy.id);
            }
            
            // If there are still active strategies, sleep before next check
            if !active_strategies.is_empty() {
                // Calculate time until next scheduled close
                let next_close = active_strategies
                    .iter()
                    .map(|s| s.close_at)
                    .min()
                    .unwrap_or_else(|| Utc::now() + Duration::seconds(CHECK_INTERVAL_SECS as i64));
                
                let time_until_next = (next_close - Utc::now())
                    .to_std()
                    .unwrap_or(TokioDuration::from_secs(CHECK_INTERVAL_SECS));
                
                // Sleep for the shorter of: time until next close or check interval
                let sleep_duration = time_until_next.min(TokioDuration::from_secs(CHECK_INTERVAL_SECS));
                
                if sleep_duration.as_secs() > 0 {
                    let remaining_strategies = active_strategies.len();
                    let next_check_secs = sleep_duration.as_secs();
                    info!(
                        "⏳ {} active strategies remaining. Next check in {} seconds...",
                        remaining_strategies,
                        next_check_secs
                    );
                    sleep(sleep_duration).await;
                }
            }
        }
        
        info!("🎊 All strategies have been closed!");
        Ok(())
    }

    /// Check liquidation levels for all positions in a strategy
    /// 
    /// # Arguments
    /// * `strategy` - Strategy metadata containing wallet IDs
    /// 
    /// # Returns
    /// * `Ok(Some(Decimal))` - Minimum percentage to liquidation across all positions
    /// * `Ok(None)` - No positions found or unable to check
    /// * `Err(TradingError)` - Error checking positions
    async fn check_strategy_liquidation_levels(
        &self,
        strategy: &StrategyMetadata,
    ) -> Result<Option<Decimal>, TradingError> {
        let mut min_percentage: Option<Decimal> = None;

        for &wallet_id in &strategy.wallet_ids {
            let client = match self.get_lighter_client(wallet_id) {
                Ok(c) => c,
                Err(e) => {
                    warn!("⚠️ Could not get Lighter client for wallet {}: {}", wallet_id, e);
                    continue;
                }
            };

            let positions = match client.get_active_positions().await {
                Ok(p) => p,
                Err(e) => {
                    warn!("⚠️ Could not fetch positions for wallet {}: {}", wallet_id, e);
                    continue;
                }
            };

            for position in positions {
                let percentage = position.get_percentage_to_liquidation();

                if percentage == Decimal::ZERO {    
                    continue;
                }

                match min_percentage {
                    None => min_percentage = Some(percentage),
                    Some(current_min) if percentage < current_min => {
                        min_percentage = Some(percentage);
                    }
                    _ => {}
                }
            }
        }

        Ok(min_percentage)
    }

    /// Execute a market-neutral farming strategy on Backpack exchange
    /// 
    /// This method implements a sophisticated trading strategy designed for
    /// points farming while minimizing market exposure through balanced
    /// long/short positions across multiple wallets.
    /// 
    /// # Strategy Overview
    /// 1. **Conflict Resolution**: Checks for active strategies and waits if needed
    /// 2. **Balance Checking**: Verifies sufficient USDC balance in all wallets
    /// 3. **Token Selection**: Randomly selects a trading token from supported list
    /// 4. **Position Allocation**: Creates balanced long/short allocations across wallets
    /// 5. **Parallel Execution**: Opens all positions concurrently for efficiency
    /// 6. **Strategy Tracking**: Creates and persists strategy metadata for monitoring
    /// 
    /// # Returns
    /// * `Ok(TradingStrategy)` - Complete strategy with all opened positions
    /// * `Err(TradingError)` - If any step fails or insufficient balances
    /// 
    /// # Market Neutral Approach
    /// The strategy aims for market neutrality by balancing long and short positions
    /// across the wallet pool, reducing overall market exposure while maximizing
    /// points farming potential.
    pub async fn farm_points_on_backpack_from_multiple_wallets(
        &self,
    ) -> Result<TradingStrategy, TradingError> {
        info!("🎯 Starting Backpack farming strategy with {} wallets", self.wallets.len());
        let mut rng = rand::thread_rng();
        let duration_minutes = rng.gen_range(1..=3);
        info!("⏱️  Strategy duration: {} minutes", duration_minutes);

        // Handle conflicting strategies across our wallets (retry-after-close behavior)
        self.handle_conflicting_strategies().await?;
        info!("✅ No active strategies found for these wallets.");

		// Step 1: Fetch USDC balances from all wallets
		let wallet_balances = self.fetch_wallet_balances_on_backpack().await?;
		for (id, balance) in &wallet_balances {
			info!("💰 Wallet #{}: {:.2} USDC", id, balance);
		}

        // Step 2: Generate balanced long/short allocations
        let allocations = TradingStrategy::generate_balanced_allocations(&wallet_balances)?;

		// Step 3: Randomly select a token to trade
		let selected_token = self.select_random_token(&Exchange::Backpack)?;
        info!("🎲 Selected token: {:?}", selected_token.symbol);

        // Step 4: Open positions for each allocation (in parallel)
        let close_at = Utc::now() + Duration::minutes(duration_minutes);
        info!("⏱️  Strategy duration: {} minutes", duration_minutes);
        
        // Create futures for all position openings
        let mut position_futures = Vec::new();
        
        for allocation in allocations {
            let token = selected_token.clone();
            let side = allocation.side;
            let usdc_amount = allocation.usdc_amount;
            
            // Create a future for opening this position
            let future = async move {
                let backpack_client = BackpackClient::new(self.find_wallet(allocation.wallet_id)?);
                
                let position = backpack_client
                    .open_position(
                        token,
                        side,
                        close_at,
                        usdc_amount,
                    )
                    .await?;
                Ok::<(Position, PositionSide), TradingError>((position, side))
            };
            
            position_futures.push(future);
        }
        
        // Execute all position openings in parallel
        info!("🚀 Opening {} positions in parallel...", position_futures.len());
        let results = futures::future::join_all(position_futures).await;
        
        // Separate results into longs and shorts
        let mut long_positions = Vec::new();
        let mut short_positions = Vec::new();
        
        for result in results {
            let (position, side) = result?;
            match side {
                crate::model::PositionSide::Long => long_positions.push(position),
                crate::model::PositionSide::Short => short_positions.push(position),
            }
        }
        
        info!("✅ All positions opened successfully!");

        // Step 5: Build the trading strategy
        let mut strategy = TradingStrategy::build_from_positions(
            selected_token.get_symbol_string(Exchange::Backpack),
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

        let close_at_local = strategy.close_at + chrono::Duration::hours(8);
        let now_local = Utc::now() + chrono::Duration::hours(8);
        let minutes_from_now = ((close_at_local - now_local).num_minutes()).max(0);

        info!(
            "   Close at: {} ({}), in {} minutes",
            close_at_local.format("%a %H:%M"),
            close_at_local.format("%Y-%m-%d"),
            minutes_from_now
        );
        
        Ok(strategy)
    }

    /// Execute a market-neutral farming strategy on Lighter exchange with wallet grouping
    /// 
    /// Enhanced version that automatically groups wallets when more than 3 wallets are available.
    /// Each group trades a different random token for better diversification.
    /// 
    /// # Enhanced Features
    /// - **Wallet Grouping**: Automatically creates groups of 3-5 wallets when >3 wallets available
    /// - **Multi-Token Diversification**: Each group trades a different random token
    /// - **Group-based Strategies**: Separate strategy tracking for each wallet group
    /// - **Pre-trade preview**: Displays strategy details before execution
    /// - **Partial failure handling**: Automatically rolls back if any position fails
    /// - **Detailed logging**: Comprehensive progress and status reporting
    /// - **Time flexibility**: Duration specified in minutes for precise control
    /// 
    /// # Arguments
    /// * `duration_minutes` - Duration in minutes for position lifetime
    /// 
    /// # Returns
    /// * `Ok(TradingStrategy)` - Complete strategy with all opened positions (first group's strategy)
    /// * `Err(TradingError)` - If any position fails (with automatic rollback)
    /// 
    /// # Grouping Logic
    /// - If ≤3 wallets: Single group with one random token
    /// - If >3 wallets: Random groups of 3-5 wallets, each with different random tokens
    /// - Groups are created randomly for better distribution
    /// - Each group executes as an independent market-neutral strategy
    /// 
    /// # Safety Mechanisms
    /// - All-or-nothing position opening with automatic rollback on failures
    /// - Balance verification before trading
    /// - Strategy preview for user confirmation (commented out but available)
    /// - Comprehensive error handling and cleanup
    pub async fn farm_points_on_lighter_from_multiple_wallets(&self) -> Result<Vec<TradingStrategy>, TradingError> {
        info!("🎯 Starting Lighter farming strategy with {} wallets", self.wallets.len());
        let mut rng = rand::thread_rng();
        let duration_minutes = rng.gen_range(120..=300);

        // Handle conflicting strategies across our wallets (retry-after-close behavior)
        self.handle_conflicting_strategies().await?;
        // Handle conflicting positions on Lighter exchange
        self.handle_conflicting_positions().await?;

        // Step 1: Fetch USDC balances from all wallets on Lighter
        let wallet_balances = self.fetch_wallet_balances_on_lighter().await?;
        for (id, balance) in wallet_balances.iter() {
            info!("💰 Wallet #{}: {:.2} USDC", id, balance);
        }

        // Step 2: Create wallet groups based on total wallet count
        let wallet_groups = if self.wallets.len() > 3 {
            self.create_random_wallet_groups(3, 5)?
        } else {
            // Single group with all wallets
            vec![self.wallets.iter().map(|w| w.id).collect()]
        };

        info!("📊 Created {} wallet group(s) for trading:", wallet_groups.len());
        for (i, group) in wallet_groups.iter().enumerate() {
            info!("   Group {}: {:?}", i + 1, group);
        }

        // Step 3: Execute strategies for each group
        let mut all_strategies: Vec<TradingStrategy> = Vec::new();

        for (group_index, wallet_group) in wallet_groups.into_iter().enumerate() {
            info!("🚀 Executing strategy for group {} (wallets: {:?})", group_index + 1, wallet_group);

            let close_at = Utc::now() + Duration::minutes(duration_minutes + rng.gen_range(1..=5));
            info!("⏱️  Strategy duration: {} minutes", duration_minutes);

            // Filter balances for this group
            let group_balances: Vec<(u8, Decimal)> = wallet_balances
                .iter()
                .filter(|(id, _)| wallet_group.contains(id))
                .cloned()
                .collect();

            if group_balances.is_empty() {
                warn!("⚠️  Group {} has no wallets with balances, skipping", group_index + 1);
                continue;
            }

            // Generate balanced allocations for this group
            let allocations = TradingStrategy::generate_balanced_allocations(&group_balances)?;

            // Select a random token for this group (different for each group)
            let selected_token = self.select_random_token(&Exchange::Lighter)?;
            let token_symbol = selected_token.symbol.to_string();
            info!("🎲 Group {} selected token: {:?}", group_index + 1, selected_token.symbol);

            // Create futures for all position openings in this group
            let mut position_futures = Vec::new();
            
            for allocation in allocations {
                let token = selected_token.clone();
                let side = allocation.side;
                let usdc_amount = allocation.usdc_amount;
                
                let future = async move {
                    let lighter_client = self.get_lighter_client(allocation.wallet_id)?;
                    let position = lighter_client
                        .open_position(
                            token,
                            side,
                            close_at,
                            usdc_amount,
                        )
                        .await?;
                    
                    Ok::<(Position, PositionSide, u8, LighterClient), TradingError>
                    ((position.clone(), side, allocation.wallet_id, lighter_client))
                };
                
                position_futures.push(future);
            }
        
            // Execute all position openings in parallel for this group
            info!("🚀 Opening {} positions for group {}...", position_futures.len(), group_index + 1);
            let results = futures::future::join_all(position_futures).await;

            // Check all results for this group
            let mut opened_positions = Vec::new();
            let mut errors = Vec::new();

            for result in results {
                match result {
                    Ok((position, side, wallet, client)) => {
                        opened_positions.push((position, side, wallet, client));
                    }
                    Err(e) => {
                        errors.push(e);
                    }
                }
            }

            // If there were any errors in this group, roll back and return error
            if !errors.is_empty() {
                let error = errors
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| TradingError::InvalidInput("Unknown error opening positions".to_string()));

                error!("❌ Position opening failed for group {}: {}", group_index + 1, error);

                if !opened_positions.is_empty() {
                    warn!(
                        "🔄 Rolling back {} successfully opened position(s) from group {}...",
                        opened_positions.len(),
                        group_index + 1
                    );
                    
                    // Roll back only positions from this group
                    self.close_positions_on_lighter_for_wallets_group(&wallet_group).await?;
                } else {
                    warn!("No positions succeeded in group {}, nothing to roll back.", group_index + 1);
                }

                return Err(error);
            }
        
            // Separate results into longs and shorts for this group
            let mut long_positions = Vec::new();
            let mut short_positions = Vec::new();
            
            for (position, side, _wallet_id, _client) in opened_positions {
                match side {
                    crate::model::PositionSide::Long => long_positions.push(position),
                    crate::model::PositionSide::Short => short_positions.push(position),
                }
            }
        
            info!("✅ All positions opened successfully for group {}!", group_index + 1);

            // Build the trading strategy for this group
            let mut strategy = TradingStrategy::build_from_positions(
                selected_token.get_symbol_string(Exchange::Lighter),
                long_positions.clone(), 
                short_positions.clone()
            )?;

            // Add group information to strategy metadata
            strategy.wallet_ids = wallet_group;
        
            // Link positions to strategy and save
            let strategy_id = strategy.id.clone();
            for position in strategy.longs.iter_mut().chain(strategy.shorts.iter_mut()) {
                position.strategy_id = Some(strategy_id.clone());
            }

            // Save strategy
            self.strategy_storage.save_strategy(&strategy).await?;
            all_strategies.push(strategy.clone());

            info!("✅ Strategy {} for group {} executed successfully!", strategy_id, group_index + 1);
            info!("   Token: {}", token_symbol);
            info!("   Long positions: {} | Total size: {:.2} USDC", strategy.longs.len(), strategy.longs_size);
            info!("   Short positions: {} | Total size: {:.2} USDC", strategy.shorts.len(), strategy.shorts_size);
        }
        
        // Display summary of all executed strategies
        info!("🎊 All {} strategy groups completed successfully!", all_strategies.len());
        for (i, strategy) in all_strategies.iter().enumerate() {
            let close_at_local = strategy.close_at + chrono::Duration::hours(8);
            let now_local = Utc::now() + chrono::Duration::hours(8);
            let minutes_from_now = ((close_at_local - now_local).num_minutes()).max(0);
            
            info!(
                "   Group {}: {} | Wallets: {:?} | Close in {} minutes",
                i + 1,
                strategy.token_symbol,
                strategy.wallet_ids,
                minutes_from_now
            );
        }

        Ok(all_strategies)
    }

    /// Close all open positions on Lighter exchange across all wallets, in parallel, retrying up to 5 attempts.
    ///
    /// This method attempts to close every open position for every wallet,
    /// spawning all close operations as concurrent async tasks. Retries up to 5 times if failures occur.
    ///
    /// # Returns
    /// * `Ok(())` - All close operations completed successfully
    /// * `Err(TradingError)` - If any close operation fails after all attempts
    #[allow(unused)]
    pub async fn close_all_positions_on_lighter_for_all_wallets(&self) -> Result<(), TradingError> {
        use futures::future::try_join_all;

        for attempt in 1..=MAX_ATTEMPTS {
            info!("Attempt {} to close all positions on Lighter (all wallets)...", attempt);

            let close_futures = self.wallets.iter().map(|wallet| {
                let wallet = wallet.clone();

                async move {
                    let client = LighterClient::new(&wallet).await?;
                    client.close_all_positions().await
                }
            });

            let results = try_join_all(close_futures).await;

            match results {
                Ok(_) => {
                    info!("✅ Successfully closed all positions on Lighter on attempt {}", attempt);
                    return Ok(());
                }
                Err(e) => {
                    error!("❌ Attempt {} failed to close all positions on Lighter: {}", attempt, e);
                    if attempt < MAX_ATTEMPTS {
                        info!("Retrying in 350ms...");
                        sleep(crate::Duration::from_millis(350)).await;
                    } else {
                        error!("❌ Ultimately failed to close all positions on Lighter after {} attempts", MAX_ATTEMPTS);
                        return Err(TradingError::ExchangeError(format!(
                            "Failed to close all positions on Lighter after {} attempts: {} | YOU NEED TO CLOSE THE POSITIONS MANUALLY!",
                            MAX_ATTEMPTS, e
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// Close open positions on Lighter exchange for a specific group of wallets, retrying up to 5 attempts.
    ///
    /// This method attempts to close positions only for the provided wallets,
    /// spawning all close operations as concurrent async tasks. Retries up to 5 times if failures occur.
    ///
    /// # Arguments
    /// * `wallet_ids` - Vector of wallet IDs to close positions for
    ///
    /// # Returns
    /// * `Ok(())` - All positions closed successfully for the group
    /// * `Err(TradingError)` - If any close operation fails after all attempts
    async fn close_positions_on_lighter_for_wallets_group(&self, wallet_ids: &[u8]) -> Result<(), TradingError> {
        use futures::future::try_join_all;

        for attempt in 1..=MAX_ATTEMPTS {
            info!(
                "Attempt {} to close positions on Lighter for wallet group {:?}...",
                attempt, wallet_ids
            );

            let close_futures = wallet_ids.iter().map(|&wallet_id| {
                let wallet = self.find_wallet(wallet_id);
                async move {
                    let wallet = wallet?;
                    let client = LighterClient::new(wallet).await?;
                    client.close_all_positions().await
                }
            });

            let results = try_join_all(close_futures).await;

            match results {
                Ok(_) => {
                    info!("✅ Successfully closed all positions for wallet group on attempt {}", attempt);
                    return Ok(());
                }
                Err(e) => {
                    error!(
                        "❌ Attempt {} failed to close positions for wallet group {:?}: {}",
                        attempt, wallet_ids, e
                    );

                    if attempt < MAX_ATTEMPTS {
                        info!("Retrying in 350ms...");
                        sleep(crate::Duration::from_millis(350)).await;
                    } else {
                        error!(
                            "❌ Ultimately failed to close positions for wallet group {:?} after {} attempts",
                            wallet_ids, MAX_ATTEMPTS
                        );
                        return Err(TradingError::ExchangeError(format!(
                            "Failed to close positions for wallet group {:?} after {} attempts: {} | YOU NEED TO CLOSE THE POSITIONS MANUALLY!",
                            wallet_ids, MAX_ATTEMPTS, e
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// Close open positions on Lighter exchange for a single wallet, retrying up to 5 attempts.
    ///
    /// This method attempts to close all open positions for a given wallet by ID,
    /// and retries up to 5 times if failures occur.
    ///
    /// # Arguments
    /// * `wallet_id` - The wallet ID for which to close positions
    ///
    /// # Returns
    /// * `Ok(())` - All positions closed successfully for the wallet
    /// * `Err(TradingError)` - If the close operation fails after all attempts
    async fn close_positions_on_lighter_for_wallet(&self, wallet_id: u8) -> Result<(), TradingError> {
        for attempt in 1..=MAX_ATTEMPTS {
            info!("Attempt {} to close positions on Lighter for wallet {}...", attempt, wallet_id);
            let client_result = self.get_lighter_client(wallet_id);
            let client = match client_result {
                Ok(c) => c,
                Err(e) => {
                    error!("❌ Failed to get Lighter client for wallet {}: {}", wallet_id, e);
                    return Err(e);
                }
            };

            let close_result = client.close_all_positions().await;
            match close_result {
                Ok(_) => {
                    info!("✅ Successfully closed all positions for wallet {} on attempt {}", wallet_id, attempt);
                    return Ok(());
                }
                Err(e) => {
                    error!("❌ Attempt {} failed to close positions for wallet {}: {}", attempt, wallet_id, e);
                    if attempt < MAX_ATTEMPTS {
                        info!("Retrying in 350ms...");
                        sleep(crate::Duration::from_millis(350)).await;
                    } else {
                        error!("❌ Ultimately failed to close positions for wallet {} after {} attempts", wallet_id, MAX_ATTEMPTS);
                        return Err(TradingError::ExchangeError(format!(
                            "Failed to close positions for wallet {} after {} attempts: {} | YOU NEED TO CLOSE THE POSITIONS MANUALLY!",
                            wallet_id, MAX_ATTEMPTS, e
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// Create random wallet groups with 3-5 wallets per group
    /// 
    /// # Arguments
    /// * `min_group_size` - Minimum number of wallets per group (default: 3)
    /// * `max_group_size` - Maximum number of wallets per group (default: 5)
    /// 
    /// # Returns
    /// * `Ok(Vec<Vec<u8>>)` - Vector of wallet groups, each containing wallet IDs
    /// * `Err(TradingError)` - If grouping is not possible
    fn create_random_wallet_groups(&self, min_group_size: usize, max_group_size: usize) -> Result<Vec<Vec<u8>>, TradingError> {
        if min_group_size > max_group_size {
            return Err(TradingError::InvalidInput("min_group_size cannot be greater than max_group_size".into()));
        }

        if self.wallets.len() < min_group_size {
            return Err(TradingError::InvalidInput(format!(
                "Not enough wallets ({}) for minimum group size ({})",
                self.wallets.len(),
                min_group_size
            )));
        }

        let mut wallet_ids: Vec<u8> = self.wallets.iter().map(|w| w.id).collect();
        let mut rng = rand::thread_rng();
        
        // Shuffle wallets for random distribution
        wallet_ids.shuffle(&mut rng);
        
        let mut groups = Vec::new();
        let mut current_index = 0;
        
        while current_index < wallet_ids.len() {
            // Calculate remaining wallets
            let remaining = wallet_ids.len() - current_index;
            
            // Determine group size
            let group_size = if remaining <= max_group_size {
                // Last group - take all remaining
                remaining
            } else {
                // Calculate optimal group size to avoid very small last groups
                let optimal_size = if remaining - min_group_size <= max_group_size {
                    // Distribute evenly to avoid small last group
                    let groups_remaining = (remaining as f64 / max_group_size as f64).ceil() as usize;
                    (remaining + groups_remaining - 1) / groups_remaining // ceiling division
                } else {
                    // Use random size within bounds
                    rng.gen_range(min_group_size..=max_group_size)
                };
                
                optimal_size.min(remaining)
            };
            
            // Create group
            let group = wallet_ids[current_index..current_index + group_size].to_vec();
            groups.push(group);
            current_index += group_size;
        }
        
        // Ensure last group meets minimum size requirement by redistributing if needed
        if let Some(last_group) = groups.last() {
            if last_group.len() < min_group_size && groups.len() > 1 {
                let last_group = groups.pop().unwrap();
                if let Some(prev_group) = groups.last_mut() {
                    prev_group.extend(last_group);
                }
            }
        }
        
        Ok(groups)
    }


    // ===== Internal Helper Methods =====

    /// Wait asynchronously until the specified deadline
    /// 
    /// # Arguments
    /// * `deadline` - DateTime to wait until
	async fn wait_until(&self, deadline: DateTime<Utc>) {
		let now = Utc::now();
		if deadline > now {
			let wait = (deadline - now)
				.to_std()
				.unwrap_or(TokioDuration::from_secs(0));
            
			if wait.as_secs() > 0 {
				sleep(wait).await;
			}
		}
	}

    /// Find a wallet by ID from the loaded wallets
    /// 
    /// # Arguments
    /// * `wallet_id` - Numeric wallet identifier
    /// 
    /// # Returns
    /// * `Ok(&Wallet)` - Reference to the found wallet
    /// * `Err(TradingError)` - If wallet ID not found
    #[allow(unused)]
	fn find_wallet(&self, wallet_id: u8) -> Result<&Wallet, TradingError> {
		self.wallets
			.iter()
			.find(|w| w.id == wallet_id)
			.ok_or_else(|| TradingError::InvalidInput(format!("Wallet #{} not found", wallet_id)))
	}

    /// Mark a position as failed in storage
    /// 
    /// # Arguments
    /// * `position_id` - Unique identifier of the position to mark as failed
    #[allow(unused)]
	async fn mark_position_failed(&self, position_id: &str) -> Result<(), TradingError> {
		self
			.position_storage
			.update_position_status(position_id, PositionStatus::Failed, Some(Utc::now()), None)
			.await
	}


    /// Update the status of a strategy in storage
    /// 
    /// # Arguments
    /// * `strategy_id` - Unique strategy identifier
    /// * `status` - New status to set
    /// * `closed_at` - Optional close timestamp (for completed strategies)
    /// * `total_pnl` - Optional PNL value (for completed strategies)
	async fn set_strategy_status(
		&self,
		strategy_id: &str,
		status: StrategyStatus,
		closed_at: Option<DateTime<Utc>>,
		total_pnl: Option<Decimal>,
	) -> Result<(), TradingError> {
		self
			.strategy_storage
			.update_strategy_status(strategy_id, status, closed_at, total_pnl)
			.await
	}


    /// Get a Lighter client for a specific wallet
    /// 
    /// # Arguments
    /// * `wallet_id` - Wallet identifier
    /// 
    /// # Returns
    /// * `Ok(LighterClient)` - Configured client for the wallet
    /// * `Err(TradingError)` - If wallet client not found
    pub fn get_lighter_client(&self, wallet_id: u8) -> Result<LighterClient, TradingError> {
        Ok(self.wallet_trading_clients
            .iter().find(|w| w.wallet.id == wallet_id)
            .ok_or_else(|| TradingError::InvalidInput(format!("Lighter client for wallet #{} not found", wallet_id)))?
            .lighter_client
            .clone())
    }


    /// Get a Backpack client for a specific wallet
    /// 
    /// # Arguments
    /// * `wallet_id` - Wallet identifier
    /// 
    /// # Returns
    /// * `Ok(BackpackClient)` - Configured client for the wallet
    /// * `Err(TradingError)` - If wallet client not found
    // fn get_backpack_client(&self, wallet_id: u8) -> Result<BackpackClient, TradingError> {
    //     Ok(self.wallet_trading_clients
    //         .iter().find(|w| w.wallet.id == wallet_id)
    //         .ok_or_else(|| TradingError::InvalidInput(format!("Backpack client for wallet #{} not found", wallet_id)))?
    //         .backpack_client
    //         .clone())
    // }



    /// Fetch USDC balances for all wallets from Backpack exchange
    /// 
    /// # Returns
    /// * `Ok(Vec<(u8, Decimal)>)` - Vector of (wallet_id, balance) pairs
    /// * `Err(TradingError)` - If any wallet has insufficient balance or API fails
	pub async fn fetch_wallet_balances_on_backpack(&self) -> Result<Vec<(u8, Decimal)>, TradingError> {
		let mut balances = Vec::with_capacity(self.wallets.len());

		for wallet in &self.wallets {
			let client = BackpackClient::new(&wallet);
			let balance = client.get_usdc_balance().await?;

			if balance <= Decimal::ZERO {
				return Err(TradingError::InvalidInput(format!(
					"Wallet #{} has insufficient USDC balance: {}",
					wallet.id, balance
				)));
			}

			balances.push((wallet.id, balance));
		}
		Ok(balances)
	}


    /// Fetch USDC balances for all wallets from Lighter exchange in parallel
    /// 
    /// # Returns
    /// * `Ok(Vec<(u8, Decimal)>)` - Vector of (wallet_id, balance) pairs
    /// * `Err(TradingError)` - If any wallet has insufficient balance or API fails
    pub async fn fetch_wallet_balances_on_lighter(&self) -> Result<Vec<(u8, Decimal)>, TradingError> {
        use futures::future::try_join_all;

        // For each wallet, spawn an async block to fetch its balance
        let balance_futures = self.wallets.iter().map(|wallet| {
            async move {
                let client = self.get_lighter_client(wallet.id)?;
                let balance = client.get_usdc_balance().await?;
                if balance <= Decimal::ZERO {
                    return Err(TradingError::InvalidInput(format!(
                        "Wallet #{} has insufficient USDC balance: {}",
                        wallet.id, balance
                    )));
                }
                Ok((wallet.id, balance))
            }
        });

        let balances: Vec<(u8, Decimal)> = try_join_all(balance_futures).await?;
        Ok(balances)
    }



    /// Randomly select a token from the supported tokens list
    /// 
    /// # Returns
    /// * `Ok(Token)` - Randomly selected token for trading
    /// * `Err(TradingError)` - If no supported tokens are available
	pub fn select_random_token(&self, exchange: &Exchange) -> Result<Token, TradingError> {
		let supported = Token::get_supported_tokens(exchange);
		let mut rng = rand::thread_rng();

		supported
			.choose(&mut rng)
			.cloned()
			.ok_or_else(|| TradingError::InvalidInput("No tokens available".into()))
	}

    /// Check for and handle conflicting active strategies
    /// 
    /// This method prevents strategy conflicts by:
    /// 1. Identifying active strategies that use any of this client's wallets
    /// 2. Waiting for those strategies to complete naturally
    /// 3. Returning an error instructing the caller to retry
    /// 
    /// # Returns
    /// * `Ok(())` - No conflicting strategies found
    /// * `Err(TradingError)` - Conflicts were found and handled, retry needed
	async fn handle_conflicting_strategies(&self) -> Result<(), TradingError> {
		let active_strategies = self.strategy_storage.get_active_strategies().await?;

		let conflicting: Vec<StrategyMetadata> = active_strategies
			.into_iter()
			.filter(|strategy| strategy.wallet_ids.iter().any(|id| self.wallets.iter().any(|w| w.id == *id)))
			.collect();

		if !conflicting.is_empty() {
			info!("⚠️  Found {} active strategies involving the provided wallets", conflicting.len());
			info!("📋 Waiting for these strategies to complete before starting new trades...");
			self.monitor_and_close_strategies(conflicting).await?;

            info!("🔄 Active strategies were completed. Starting new trades...");
		}

        info!("✅ No active strategies found for these wallets.");
		Ok(())
	}

    /// Check for and handle conflicting positions on Lighter exchange
    /// 
    /// This method prevents position conflicts by:
    /// 1. Identifying active positions on any of this client's wallets
    /// 2. Waiting for those positions to complete naturally
    /// 3. Returning an error instructing the caller to retry
    /// 
    /// # Returns
    async fn handle_conflicting_positions(&self) -> Result<(), TradingError> {
        for wallet in self.wallets.iter() {
            let client = self.get_lighter_client(wallet.id)?;
            let positions = client.get_active_positions().await?;
            if positions.is_empty() {
                continue;
            }

            info!("⚠️  Found {} active positions on wallet {}", positions.len(), wallet.id);
            info!("📋 Waiting for these positions to complete before starting new trades...");
            self.close_positions_on_lighter_for_wallet(wallet.id).await?;
        }

        info!("✅ No active positions found on any wallets.");
        Ok(())
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
