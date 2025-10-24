#![allow(unused)]

use chrono::{DateTime, Utc};
use rust_decimal::{prelude::FromPrimitive, Decimal};
use rand::Rng;
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use crate::{error::TradingError, model::{Position, PositionSide}};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingStrategy {
    pub id: String,
    pub token_symbol: String,
    pub wallet_ids: Vec<u8>,
    pub longs: Vec<Position>,
    pub shorts: Vec<Position>,
    pub shorts_size: Decimal,
    pub longs_size: Decimal,
    pub opened_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub close_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub realized_pnl: Option<Decimal>,
    pub status: StrategyStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrategyStatus {
    Running,
    Closing,
    Closed,
    Failed
}

impl std::fmt::Display for StrategyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StrategyStatus::Running => write!(f, "RUNNING"),
            StrategyStatus::Closing => write!(f, "CLOSING"),
            StrategyStatus::Closed => write!(f, "CLOSED"),
            StrategyStatus::Failed => write!(f, "FAILED"),
        }
    }
}

impl std::str::FromStr for StrategyStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "RUNNING" => Ok(StrategyStatus::Running),
            "CLOSING" => Ok(StrategyStatus::Closing),
            "CLOSED" => Ok(StrategyStatus::Closed),
            "FAILED" => Ok(StrategyStatus::Failed),
            _ => Err(format!("Invalid StrategyStatus: {}", s)),
        }
    }
}

/// Represents a wallet with its allocation for a trade
#[derive(Debug, Clone)]
pub struct WalletAllocation {
    pub wallet_id: u8,
    pub side: PositionSide,
    pub usdc_amount: Decimal,
    pub percentage: Decimal,
}

impl TradingStrategy {
    pub fn build_from_positions(
        token_symbol: String,
        longs: Vec<Position>,
        shorts: Vec<Position>
    ) -> Result<Self, TradingError> {
        if longs.is_empty() && shorts.is_empty() {
            return Err(TradingError::InvalidInput(
                "Strategy must have at least one position".into()
            ));
        }

        let id = Uuid::new_v4().to_string();
        let longs_size = longs.iter().map(|l| l.size).sum();
        let shorts_size = shorts.iter().map(|s| s.size).sum();
        
        // Extract unique wallet IDs from all positions
        let mut wallet_ids: Vec<u8> = longs.iter()
            .chain(shorts.iter())
            .map(|p| p.wallet_id)
            .collect();
        wallet_ids.sort_unstable();
        wallet_ids.dedup();
        
        // Get timestamps from first available position
        let first_position = longs.first().or(shorts.first()).unwrap();
        let opened_at = first_position.opened_at;
        let updated_at = opened_at;
        let close_at = first_position.close_at;
        let closed_at = None;
        let realized_pnl = None;
        let status = StrategyStatus::Running;
        
        Ok(Self {
            id,
            token_symbol,
            wallet_ids,
            longs,
            shorts,
            shorts_size,
            longs_size,
            opened_at,
            updated_at,
            close_at,
            closed_at,
            realized_pnl,
            status
        })
    }

    #[allow(unused)]
    /// Check if the strategy should be closed based on current time
    pub fn should_close(&self) -> bool {
        Utc::now() >= self.close_at && self.status == StrategyStatus::Running
    }

    #[allow(unused)]
    /// Get all position IDs in this strategy
    pub fn get_all_position_ids(&self) -> Vec<String> {
        self.longs
            .iter()
            .chain(self.shorts.iter())
            .map(|p| p.id.clone())
            .collect()
    }

    #[allow(unused)]
    /// Calculate total PnL from all positions
    pub fn calculate_total_pnl(&self) -> Option<Decimal> {
        let all_positions: Vec<&Position> = self.longs
            .iter()
            .chain(self.shorts.iter())
            .collect();
        
        if all_positions.iter().all(|p| p.realized_pnl.is_some()) {
            Some(all_positions.iter().filter_map(|p| p.realized_pnl).sum())
        } else {
            None
        }
    }
    
    /// Generate balanced long/short allocations from wallet balances
    /// Ensures total long value ≈ total short value for market neutrality
    pub fn generate_balanced_allocations(
        wallet_balances: &Vec<(u8, Decimal)>,
    ) -> Result<Vec<WalletAllocation>, TradingError> {
        use rand::seq::SliceRandom;
        use rand::Rng;

        if wallet_balances.len() < 3 {
            return Err(TradingError::InvalidInput(
                "At least 3 wallets are required".into()
            ));
        }
        
        let mut rng = rand::thread_rng();
        let total_balance: Decimal = wallet_balances.iter().map(|(_, b)| b).sum();
        if total_balance <= Decimal::ZERO {
            return Err(TradingError::InvalidInput(
                "Total wallet balance must be greater than zero".into()
            ));
        }

        // Shuffle wallet indices to randomly assign to long/short, as before
        let mut wallet_indices: Vec<usize> = (0..wallet_balances.len()).collect();
        wallet_indices.shuffle(&mut rng);

        // Randomly split into long/short groups, ensuring at least 1 for each
        let num_longs = rng.gen_range(1..wallet_balances.len());
        let long_indices = &wallet_indices[0..num_longs];
        let short_indices = &wallet_indices[num_longs..];

        // Calculate side totals (for future normalization of allocations)
        let long_total_balance: Decimal = long_indices.iter().map(|&i| wallet_balances[i].1).sum();
        let short_total_balance: Decimal = short_indices.iter().map(|&i| wallet_balances[i].1).sum();

        // We'll use the minimum group total as the tradeable amount for both sides for neutrality
        let tradeable_amount = long_total_balance.min(short_total_balance);

        let mut allocations = Vec::new();

        // Instead of always distributing fully by balance, generate random fractions to multiply
        // each wallet's possible allocation within its group.
        // This produces random allocation percentages per wallet (but doesn't exceed balance)
        let mut random_factors: Vec<f64> = (0..wallet_balances.len()).map(|_| rng.gen_range(0.15..1.0)).collect();

        // Generate random allocations for longs
        let mut long_side_randoms: Vec<f64> = long_indices.iter().map(|&i| random_factors[i]).collect();
        let long_side_sum: f64 = long_side_randoms.iter().sum();
        for (&idx, &rf) in long_indices.iter().zip(long_side_randoms.iter()) {
            let (wallet_id, balance) = wallet_balances[idx];
            // assign this wallet a proportion of the SIDE's tradeable amount, proportional to random factor
            let proportion = rf / long_side_sum;
            let usdc_amount = Decimal::from_f64(tradeable_amount.to_string().parse::<f64>().unwrap() * proportion).unwrap();
            // Don't allocate more than the wallet has
            let usdc_amount = usdc_amount.min(balance);
            let percentage = if balance > Decimal::ZERO {
                (usdc_amount / balance) * Decimal::from(100)
            } else {
                Decimal::ZERO
            };
            allocations.push(WalletAllocation {
                wallet_id,
                side: PositionSide::Long,
                usdc_amount,
                percentage,
            });
        }

        // Same for shorts
        let mut short_side_randoms: Vec<f64> = short_indices.iter().map(|&i| random_factors[i]).collect();
        let short_side_sum: f64 = short_side_randoms.iter().sum();
        for (&idx, &rf) in short_indices.iter().zip(short_side_randoms.iter()) {
            let (wallet_id, balance) = wallet_balances[idx];
            let proportion = rf / short_side_sum;
            let usdc_amount = Decimal::from_f64(tradeable_amount.to_string().parse::<f64>().unwrap() * proportion).unwrap();
            let usdc_amount = usdc_amount.min(balance);
            let percentage = if balance > Decimal::ZERO {
                (usdc_amount / balance) * Decimal::from(100)
            } else {
                Decimal::ZERO
            };
            allocations.push(WalletAllocation {
                wallet_id,
                side: PositionSide::Short,
                usdc_amount,
                percentage,
            });
        }

        // Log the allocation strategy
        info!("Generated RANDOMIZED balanced allocation strategy:");
        let long_total: Decimal = allocations.iter()
            .filter(|a| a.side == PositionSide::Long)
            .map(|a| a.usdc_amount)
            .sum();
        let short_total: Decimal = allocations.iter()
            .filter(|a| a.side == PositionSide::Short)
            .map(|a| a.usdc_amount)
            .sum();
        
        for alloc in &allocations {
            info!(
                "  Wallet #{}: {} {:.2}% ({:.2} USDC)",
                alloc.wallet_id, alloc.side, alloc.percentage, alloc.usdc_amount
            );
        }
        
        info!("  Total LONG: {:.2} USDC | Total SHORT: {:.2} USDC", long_total, short_total);

        Ok(allocations)
    }

    /// Display strategy preview before execution
    pub fn display_strategy_preview(
        exchange_name: &str,
        token_symbol: &str,
        allocations: &[WalletAllocation],
        wallet_balances: &[(u8, Decimal)],
        duration_minutes: i64
    ) {
        println!("\nSTRATEGY PREVIEW\n");
        
        println!("\n📍 Exchange: {}", exchange_name);
        println!("🪙 Token: {}", token_symbol);
        println!("📅 Duration: {} minutes", duration_minutes);
        
        println!("\n💰 Wallet Balances:");
        for (id, balance) in wallet_balances {
            println!("   Wallet #{}: {:.2} USDC", id, balance);
        }
        
        let longs: Vec<_> = allocations.iter().filter(|a| a.side == PositionSide::Long).collect();
        let shorts: Vec<_> = allocations.iter().filter(|a| a.side == PositionSide::Short).collect();
        
        println!("\n📊 Planned LONG Positions ({}):", longs.len());
        for (i, allocation) in longs.iter().enumerate() {
            println!("   {}. Wallet #{} - ${:.2} USDC ({:.1}%)", 
                i + 1, allocation.wallet_id, allocation.usdc_amount, allocation.percentage);
        }
        
        println!("\n📉 Planned SHORT Positions ({}):", shorts.len());
        for (i, allocation) in shorts.iter().enumerate() {
            println!("   {}. Wallet #{} - ${:.2} USDC ({:.1}%)", 
                i + 1, allocation.wallet_id, allocation.usdc_amount, allocation.percentage);
        }
        
    }

}