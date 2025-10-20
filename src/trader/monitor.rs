#![allow(unused)]

use crate::{
    error::TradingError, model::PositionStatus, perp::{backpack::BackpackClient, PerpExchange}, storage::{storage_position::PositionStorage, storage_strategy::{StrategyMetadata, StrategyStorage}}, trader::{
        strategy::StrategyStatus,
        wallet::Wallet,
    }
};
use chrono::Utc;
use std::collections::HashMap;
use tokio::time::{interval, Duration};



/// Strategy monitor that checks and closes strategies
pub struct StrategyMonitor {
    position_storage: PositionStorage,
    strategy_storage: StrategyStorage,
    wallets: HashMap<u8, Wallet>,
}

impl StrategyMonitor {
    /// Create a new strategy monitor
    pub fn new(
        position_storage: PositionStorage,
        strategy_storage: StrategyStorage,
        wallets: Vec<Wallet>,
    ) -> Self {
        let wallets_map = wallets.into_iter()
            .map(|w| (w.id, w))
            .collect();
        
        Self {
            position_storage,
            strategy_storage,
            wallets: wallets_map,
        }
    }

    /// Check all active strategies and close those that should be closed
    pub async fn check_and_close_strategies(&self) -> Result<(), TradingError> {
        let strategies_to_close = self.strategy_storage.get_strategies_to_close()?;
        
        if strategies_to_close.is_empty() {
            return Ok(());
        }

        info!("🔍 Found {} strategies to close", strategies_to_close.len());

        for strategy_meta in strategies_to_close {
            match self.close_strategy(&strategy_meta).await {
                Ok(_) => {
                    info!("✅ Strategy {} closed successfully", strategy_meta.id);
                }
                Err(e) => {
                    error!("❌ Failed to close strategy {}: {}", strategy_meta.id, e);
                    // Mark strategy as failed
                    self.strategy_storage.update_strategy_status(
                        &strategy_meta.id,
                        StrategyStatus::Failed,
                        None,
                        None,
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Close a specific strategy by closing all its positions
    async fn close_strategy(&self, strategy_meta: &StrategyMetadata) -> Result<(), TradingError> {
        info!("🔄 Closing strategy {} ({})", strategy_meta.id, strategy_meta.token_symbol);
        
        // Update strategy status to Closing
        self.strategy_storage.update_strategy_status(
            &strategy_meta.id,
            StrategyStatus::Closing,
            None,
            None,
        )?;

        let all_position_ids = strategy_meta.get_all_position_ids();
        let mut closed_positions = Vec::new();
        let mut failed = false;

        // Close each position
        for position_id in all_position_ids {
            let position = self.position_storage.get_position(&position_id)?;
            
            if let Some(mut pos) = position {
                // Find the wallet for this position
                // We need to determine which wallet opened this position
                // For now, we'll try to close it with any available wallet
                // In production, you might want to store wallet_id in the position
                
                let result = self.close_position_with_any_wallet(&pos).await;
                
                match result {
                    Ok(realized_pnl) => {
                        // Update position as closed
                        self.position_storage.update_position_status(
                            &pos.id,
                            PositionStatus::Closed,
                            Some(Utc::now()),
                            Some(realized_pnl),
                        )?;
                        closed_positions.push((pos.id.clone(), realized_pnl));
                        info!("  ✓ Closed position {} | PnL: {:.2}", pos.id, realized_pnl);
                    }
                    Err(e) => {
                        error!("  ✗ Failed to close position {}: {}", pos.id, e);
                        failed = true;
                        // Mark position as failed
                        self.position_storage.update_position_status(
                            &pos.id,
                            PositionStatus::Failed,
                            Some(Utc::now()),
                            None,
                        )?;
                    }
                }
            }
        }

        // Calculate total PnL
        let total_pnl: rust_decimal::Decimal = closed_positions
            .iter()
            .map(|(_, pnl)| pnl)
            .sum();

        // Update strategy status
        let final_status = if failed {
            StrategyStatus::Failed
        } else {
            StrategyStatus::Closed
        };

        self.strategy_storage.update_strategy_status(
            &strategy_meta.id,
            final_status,
            Some(Utc::now()),
            Some(total_pnl),
        )?;

        info!(
            "💰 Strategy {} final PnL: {:.2} USDC ({})",
            strategy_meta.id,
            total_pnl,
            final_status
        );

        Ok(())
    }

    /// Try to close a position using any available wallet
    /// In production, you'd want to close with the same wallet that opened it
    async fn close_position_with_any_wallet(
        &self,
        position: &crate::model::Position,
    ) -> Result<rust_decimal::Decimal, TradingError> {
        // For now, just use the first available wallet
        // You might want to store wallet_id in positions for proper tracking
        let wallet = self.wallets.values().next()
            .ok_or_else(|| TradingError::InvalidInput("No wallets available".into()))?;
        
        let client = BackpackClient::new(wallet);
        
        // Here you would implement the actual position closing logic
        // For now, returning a mock PnL
        // TODO: Implement actual position closing via exchange API
        warn!("⚠️ Position closing not fully implemented yet - using mock PnL");
        Ok(rust_decimal::Decimal::ZERO)
    }

    /// Run the monitoring loop indefinitely
    pub async fn run_monitoring_loop(self, check_interval_secs: u64) -> Result<(), TradingError> {
        let mut interval = interval(Duration::from_secs(check_interval_secs));
        
        info!("🚀 Starting strategy monitor (checking every {}s)", check_interval_secs);
        
        loop {
            interval.tick().await;
            
            if let Err(e) = self.check_and_close_strategies().await {
                error!("Error in monitoring loop: {}", e);
            }
        }
    }

    /// Get summary of all active strategies
    pub fn get_active_strategies_summary(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        self.strategy_storage.get_active_strategies()
    }
}

