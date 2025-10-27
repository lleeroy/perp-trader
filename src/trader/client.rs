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
use rust_decimal::Decimal;
use rand::{seq::SliceRandom};
use sqlx::PgPool;
use tokio::time::{sleep, Duration as TokioDuration};
use colored::*;


pub struct TraderClient {
    wallets: Vec<Wallet>,
    wallet_trading_clients: Vec<WalletTradingClient>,
    position_storage: PositionStorage,
    strategy_storage: StrategyStorage,
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
            wallet_trading_clients,
            position_storage,
            strategy_storage,
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


            match self.close_all_positions_on_lighter().await {
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
    /// 
    /// This method provides automated strategy lifecycle management by:
    /// 1. Calculating the earliest close time across all strategies
    /// 2. Waiting until the appropriate close times
    /// 3. Closing positions for each strategy when its time arrives
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
    /// - Strategies are closed sequentially in the order provided
    /// - Each strategy waits for its specific close time
    /// - Failed closures are marked and alerted but don't stop other strategies
    /// - Local time display is adjusted to UTC+8 for user convenience
    pub async fn monitor_and_close_strategies(
        &self,
        strategies: Vec<StrategyMetadata>,
    ) -> Result<(), TradingError> {
        if strategies.is_empty() {
            return Err(TradingError::InvalidInput("No strategies provided".into()));
        }

		// Find the earliest close time across all strategies
		let earliest_close = strategies
			.iter()
			.map(|s| s.close_at)
			.min()
			.unwrap_or_else(Utc::now);

        // Display strategies being monitored
        info!("📋 Monitoring {} strategies:", strategies.len());
        for strategy in &strategies {
            let close_at_local = strategy.close_at + chrono::Duration::hours(8);
            info!("🎯 Strategy {} | Token: {} | Wallets: {:?} | Close at (your local, UTC+8): {}", 
                strategy.id, strategy.token_symbol, strategy.wallet_ids, close_at_local.format("%H:%M"));
        }

		// Wait until the earliest close time
		{
			let now = Utc::now();
			if earliest_close > now {
				let wait_duration = (earliest_close - now)
					.to_std()
					.unwrap_or(TokioDuration::from_secs(0));

				info!(
					"⏳ Waiting {} minutes until first strategy close time...",
					wait_duration.as_secs() / 60 as u64
				);

				self.wait_until(earliest_close).await;
			}
		}

        // Close each strategy
        for strategy in strategies {
			// Check if it's time to close this strategy
			{
				let now = Utc::now();
				if strategy.close_at > now {
					let remaining = (strategy.close_at - now)
						.to_std()
						.unwrap_or(TokioDuration::from_secs(0));
					if remaining.as_secs() > 0 {
						info!(
							"⏳ Waiting {} seconds before closing strategy {}...",
							remaining.as_secs(),
							strategy.id
						);
						self.wait_until(strategy.close_at).await;
					}
				}
			}

            info!("🔄 Closing strategy {} ({})", strategy.id, strategy.token_symbol);
            
            // Update strategy status to Closing
			self
				.set_strategy_status(&strategy.id, StrategyStatus::Closing, None, None)
				.await?;

            let mut has_failures = false;
            match self.close_all_positions_on_lighter().await {
                Ok(_) => {
                    info!("✅ All positions closed successfully");
                }
                Err(e) => {
                    error!("❌ Failed to close all positions: {}", e);
                    has_failures = true;
                    let strategy_clone = strategy.clone();

                    tokio::spawn(async move {
                        let alerter = TelegramAlerter::new();
                        let error = TradingError::SigningError("Signing error".to_string());
                        
                        if let Err(e) = alerter.send_strategy_error_alert(&strategy_clone, &error).await {
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

            info!("💰 Strategy {} completed | Status: {}", strategy.id, final_status);
        }

        Ok(())
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
    /// # Arguments
    /// * `duration_hours` - How long positions should remain open (typically 4-8 hours)
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
        duration_hours: i64,
    ) -> Result<TradingStrategy, TradingError> {
        info!("🎯 Starting Backpack farming strategy with {} wallets", self.wallets.len());
        
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
		let selected_token = self.select_random_token()?;
        info!("🎲 Selected token: {:?}", selected_token.symbol);

        // Step 4: Open positions for each allocation (in parallel)
        let close_at = Utc::now() + Duration::hours(duration_hours);
        info!("⏱️  Strategy duration: {} hours", duration_hours);
        
        // Create futures for all position openings
        let mut position_futures = Vec::new();
        
        for allocation in allocations {
            let token = selected_token.clone();
            let side = allocation.side;
            let usdc_amount = allocation.usdc_amount;
            
            // Create a future for opening this position
            let future = async move {
                let backpack_client = self.get_backpack_client(allocation.wallet_id)?;
                
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
            "   Close at (your local, UTC+8): {} ({}), in {} minutes",
            close_at_local.format("%a %H:%M"),
            close_at_local.format("%Y-%m-%d"),
            minutes_from_now
        );
        
        Ok(strategy)
    }

    /// Execute a market-neutral farming strategy on Lighter exchange
    /// 
    /// Similar to Backpack farming but optimized for Lighter exchange with
    /// additional safety features and confirmation steps.
    /// 
    /// # Enhanced Features
    /// - **Pre-trade preview**: Displays strategy details before execution
    /// - **Partial failure handling**: Automatically rolls back if any position fails
    /// - **Detailed logging**: Comprehensive progress and status reporting
    /// - **Time flexibility**: Duration specified in minutes for precise control
    /// 
    /// # Arguments
    /// * `duration_minutes` - Duration in minutes for position lifetime
    /// 
    /// # Returns
    /// * `Ok(TradingStrategy)` - Complete strategy with all opened positions
    /// * `Err(TradingError)` - If any position fails (with automatic rollback)
    /// 
    /// # Safety Mechanisms
    /// - All-or-nothing position opening with automatic rollback on failures
    /// - Balance verification before trading
    /// - Strategy preview for user confirmation (commented out but available)
    /// - Comprehensive error handling and cleanup
    pub async fn farm_points_on_lighter_from_multiple_wallets(
        &self,
        duration_minutes: i64,
    ) -> Result<TradingStrategy, TradingError> {
        info!("🎯 Starting Lighter farming strategy with {} wallets", self.wallets.len());

        // Handle conflicting strategies across our wallets (retry-after-close behavior)
        self.handle_conflicting_strategies().await?;
        info!("✅ No active strategies found for these wallets.");

        // Step 1: Fetch USDC balances from all wallets on Lighter
        let wallet_balances = self.fetch_wallet_balances_on_lighter().await?;
        for (id, balance) in wallet_balances.iter() {
            info!("💰 Wallet #{}: {:.2} USDC", id, balance);
        }

        // Step 2: Generate balanced long/short allocations
        let allocations = TradingStrategy::generate_balanced_allocations(&wallet_balances)?;

        // Step 3: Randomly select a token to trade
        let selected_token = self.select_random_token()?;
        let token_symbol = selected_token.symbol.to_string();
        info!("🎲 Selected token: {:?}", selected_token.symbol);

        // Step 4: Open positions for each allocation (in parallel)
        let close_at = Utc::now() + Duration::minutes(duration_minutes);
        info!("⏱️  Strategy duration: {} minutes", duration_minutes);
        
        // Create futures for all position openings
        let mut position_futures = Vec::new();

        // Display strategy preview
        TradingStrategy::display_strategy_preview(
            "Lighter",
            &token_symbol,
            &allocations,
            &wallet_balances,
            duration_minutes
        );

        // Ask for final confirmation before opening positions
        // let should_proceed = Confirm::new("Do you agree with this strategy and want to proceed with opening positions?")
        //     .with_default(false)
        //     .prompt()
        //     .context("Failed to get final confirmation")?;

        // if !should_proceed {
        //     warn!("❌ Strategy cancelled. No positions were opened.");
        //     return Err(TradingError::InvalidInput("Strategy cancelled. No positions were opened.".into()));
        // }
        
        for allocation in allocations {
            let token = selected_token.clone();
            let side = allocation.side;
            let usdc_amount = allocation.usdc_amount;
            
            // Create a future for opening this position
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
        
        // Execute all position openings in parallel
        info!("🚀 Opening {} positions in parallel...", position_futures.len());
        let results = futures::future::join_all(position_futures).await;

        // Check all results: separate successful and failed positions
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

        // If there were any errors, consider this a partial failure
        if !errors.is_empty() {
            let error = errors
                .into_iter()
                .next()
                .unwrap_or_else(|| TradingError::InvalidInput("Unknown error opening positions".to_string()));

            error!("❌ Position opening failed: {}", error);

            if !opened_positions.is_empty() {
                warn!(
                    "🔄 Rolling back {} successfully opened position(s)...",
                    opened_positions.len()
                );
                
                match self.close_all_positions_on_lighter().await {
                    Ok(_) => {
                        info!("✅ All positions rolled back successfully");
                    }
                    Err(e) => {
                        error!("❌ Failed to roll back positions: {}", e);
                    }
                }
            } else {
                warn!("No positions succeeded, nothing to roll back.");
            }

            return Err(error);
        }
        
        // Separate results into longs and shorts
        let mut long_positions = Vec::new();
        let mut short_positions = Vec::new();
        
        for (position, side, _wallet_id, _client) in opened_positions {
            match side {
                crate::model::PositionSide::Long => long_positions.push(position),
                crate::model::PositionSide::Short => short_positions.push(position),
            }
        }
        
        info!("✅ All positions opened successfully!");

        // Step 5: Build the trading strategy
        let mut strategy = TradingStrategy::build_from_positions(
            selected_token.get_symbol_string(Exchange::Lighter),
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
        info!("   Long positions: {} | Total size: {:.2} USDC", strategy.longs.len(), strategy.longs_size);
        info!("   Short positions: {} | Total size: {:.2} USDC", strategy.shorts.len(), strategy.shorts_size);
        let close_at_local = strategy.close_at + chrono::Duration::hours(8);
        let now_local = Utc::now() + chrono::Duration::hours(8);
        let minutes_from_now = ((close_at_local - now_local).num_minutes()).max(0);
        
        info!(
            "   Close at (your local, UTC+8): {} ({}), in {} minutes",
            close_at_local.format("%a %H:%M"),
            close_at_local.format("%Y-%m-%d"),
            minutes_from_now
        );
        
        Ok(strategy)
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
    fn get_lighter_client(&self, wallet_id: u8) -> Result<LighterClient, TradingError> {
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
    fn get_backpack_client(&self, wallet_id: u8) -> Result<BackpackClient, TradingError> {
        Ok(self.wallet_trading_clients
            .iter().find(|w| w.wallet.id == wallet_id)
            .ok_or_else(|| TradingError::InvalidInput(format!("Backpack client for wallet #{} not found", wallet_id)))?
            .backpack_client
            .clone())
    }


    /// Close all open positions on Lighter exchange across all wallets
    /// 
    /// This method attempts to close every open position for every wallet
    /// managed by this client. Used for emergency shutdowns and rollbacks.
    /// 
    /// # Returns
    /// * `Ok(())` - All close operations initiated successfully
    /// * `Err(TradingError)` - If any close operation fails
    async fn close_all_positions_on_lighter(&self) -> Result<(), TradingError> {
        let mut futures = Vec::new();
        for wallet in &self.wallets {
            let client = LighterClient::new(&wallet).await?;
            
            futures.push(async move {
                client.close_all_positions().await
            });
        }

        let results = futures::future::join_all(futures).await;
        for result in results {
            if let Err(e) = result {
                error!("❌ Failed to close all positions on Lighter: {}", e);
                return Err(e);
            }
        }

        Ok(())
    }


    /// Fetch USDC balances for all wallets from Backpack exchange
    /// 
    /// # Returns
    /// * `Ok(Vec<(u8, Decimal)>)` - Vector of (wallet_id, balance) pairs
    /// * `Err(TradingError)` - If any wallet has insufficient balance or API fails
	pub async fn fetch_wallet_balances_on_backpack(&self) -> Result<Vec<(u8, Decimal)>, TradingError> {
		let mut balances = Vec::with_capacity(self.wallets.len());

		for wallet in &self.wallets {
			let client = self.get_backpack_client(wallet.id)?;
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


    /// Fetch USDC balances for all wallets from Lighter exchange
    /// 
    /// # Returns
    /// * `Ok(Vec<(u8, Decimal)>)` - Vector of (wallet_id, balance) pairs
    /// * `Err(TradingError)` - If any wallet has insufficient balance or API fails
	pub async fn fetch_wallet_balances_on_lighter(&self) -> Result<Vec<(u8, Decimal)>, TradingError> {
		let mut balances = Vec::with_capacity(self.wallets.len());

		for wallet in &self.wallets {
			let client = self.get_lighter_client(wallet.id)?;
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



    /// Randomly select a token from the supported tokens list
    /// 
    /// # Returns
    /// * `Ok(Token)` - Randomly selected token for trading
    /// * `Err(TradingError)` - If no supported tokens are available
	pub fn select_random_token(&self) -> Result<Token, TradingError> {
		let supported = Token::get_supported_tokens();
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
			
            return Err(TradingError::InvalidInput(
				"Active strategies were completed. Please call the function again to create new trades.".into(),
			));
		}

		Ok(())
	}


    #[allow(unused)]
    pub fn get_supported_tokens(&self, exchange: Exchange) -> Vec<Token> {
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
