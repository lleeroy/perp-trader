use crate::{
    error::TradingError,
    model::{
		position::{Position, PositionSide, PositionStatus},
		token::Token, Exchange,
	},
	perp::{backpack::BackpackClient, lighter::client::LighterClient, PerpExchange},
	storage::{
		storage_position::PositionStorage,
		storage_strategy::{StrategyMetadata, StrategyStorage},
	},
	trader::{strategy::{StrategyStatus, TradingStrategy}, wallet::{Wallet, WalletTradingClient}},
};

use anyhow::Context;
use chrono::{DateTime, Duration, Utc};
use inquire::Confirm;
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


    #[allow(unused)]
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
                }
            }
        }
        Ok(())
    }

    /// Monitor and close strategies when they reach their close_at time
    ///
    /// # Arguments
    /// * `strategies` - Vector of strategy metadata to monitor
    ///
    /// # Returns
    /// * `Ok(())` - All strategies were successfully closed
    /// * `Err(TradingError)` - If any error occurs during monitoring/closing
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
            info!("🎯 Strategy {} | Token: {} | Wallets: {:?} | Close at: {}", 
                strategy.id, strategy.token_symbol, strategy.wallet_ids, strategy.close_at);
        }

		// Wait until the earliest close time
		{
			let now = Utc::now();
			if earliest_close > now {
				let wait_duration = (earliest_close - now)
					.to_std()
					.unwrap_or(TokioDuration::from_secs(0));

				info!(
					"⏳ Waiting {} seconds until first strategy close time...",
					wait_duration.as_secs()
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

    /// Farm points on Backpack using multiple wallets with balanced long/short positions
    /// 
    /// # Arguments
    /// * `duration_hours` - Duration in hours for how long positions should remain open (4-8 hours)
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
        info!("   Close at: {}", strategy.close_at);
        
        Ok(strategy)
    }

    /// Farm points on Lighter using multiple wallets with balanced long/short positions
    /// 
    /// # Arguments
    /// * `duration_hours` - Duration in hours for how long positions should remain open (4-8 hours)
    /// 
    /// # Returns
    /// * `Ok(TradingStrategy)` - The executed trading strategy with all positions
    /// * `Err(TradingError)` - If any error occurs during execution
    /// 
    /// # Strategy
    /// - First checks if any wallets are involved in active strategies
    /// - If active strategies found: waits for them to complete before proceeding
    /// - Fetches USDC balance from each available wallet on Lighter
    /// - Randomly selects a token to trade
    /// - Generates balanced long/short allocations (market neutral for break-even)
    /// - Opens positions on all wallets
    /// - Returns a TradingStrategy tracking all positions
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
        let selected_token = Token::bnb();
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
        let should_proceed = Confirm::new("Do you agree with this strategy and want to proceed with opening positions?")
            .with_default(false)
            .prompt()
            .context("Failed to get final confirmation")?;

        if !should_proceed {
            warn!("❌ Strategy cancelled. No positions were opened.");
            return Err(TradingError::InvalidInput("Strategy cancelled. No positions were opened.".into()));
        }
        
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
        info!("   Close at: {}", strategy.close_at);    
        
        Ok(strategy)
    }


    // ===== Small internal helpers to reduce duplication =====
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

    #[allow(unused)]
	fn find_wallet(&self, wallet_id: u8) -> Result<&Wallet, TradingError> {
		self.wallets
			.iter()
			.find(|w| w.id == wallet_id)
			.ok_or_else(|| TradingError::InvalidInput(format!("Wallet #{} not found", wallet_id)))
	}

    #[allow(unused)]
	async fn mark_position_failed(&self, position_id: &str) -> Result<(), TradingError> {
		self
			.position_storage
			.update_position_status(position_id, PositionStatus::Failed, Some(Utc::now()), None)
			.await
	}

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

    fn get_lighter_client(&self, wallet_id: u8) -> Result<LighterClient, TradingError> {
        Ok(self.wallet_trading_clients
            .iter().find(|w| w.wallet.id == wallet_id)
            .ok_or_else(|| TradingError::InvalidInput(format!("Lighter client for wallet #{} not found", wallet_id)))?
            .lighter_client
            .clone())
    }

    fn get_backpack_client(&self, wallet_id: u8) -> Result<BackpackClient, TradingError> {
        Ok(self.wallet_trading_clients
            .iter().find(|w| w.wallet.id == wallet_id)
            .ok_or_else(|| TradingError::InvalidInput(format!("Backpack client for wallet #{} not found", wallet_id)))?
            .backpack_client
            .clone())
    }

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

	pub fn select_random_token(&self) -> Result<Token, TradingError> {
		let supported = Token::get_supported_tokens();
		let mut rng = rand::thread_rng();

		supported
			.choose(&mut rng)
			.cloned()
			.ok_or_else(|| TradingError::InvalidInput("No tokens available".into()))
	}

	/// Checks for active strategies involving this client's wallets.
	/// If conflicts are present, waits for them to close and returns an error instructing caller to retry.
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
