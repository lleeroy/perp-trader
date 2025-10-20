use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rand::Rng;
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use crate::{error::TradingError, model::{Position, PositionSide}};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingStrategy {
    pub id: String,
    pub token_symbol: String,
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

    /// Check if the strategy should be closed based on current time
    pub fn should_close(&self) -> bool {
        Utc::now() >= self.close_at && self.status == StrategyStatus::Running
    }

    /// Get all position IDs in this strategy
    pub fn get_all_position_ids(&self) -> Vec<String> {
        self.longs
            .iter()
            .chain(self.shorts.iter())
            .map(|p| p.id.clone())
            .collect()
    }

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
        wallet_balances: Vec<(u8, Decimal)>,
    ) -> Result<Vec<WalletAllocation>, TradingError> {
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
        
        // Randomly decide how many wallets go long vs short
        // Ensure at least 1 long and at least 1 short (with remaining as shorts)
        let num_longs = rng.gen_range(1..wallet_balances.len());
        
        // Shuffle wallet indices to randomly assign to long/short
        let mut wallet_indices: Vec<usize> = (0..wallet_balances.len()).collect();
        use rand::seq::SliceRandom;
        wallet_indices.shuffle(&mut rng);
        
        let long_indices = &wallet_indices[0..num_longs];
        let short_indices = &wallet_indices[num_longs..];
        
        // Calculate total balance for each side
        let long_total_balance: Decimal = long_indices
            .iter()
            .map(|&i| wallet_balances[i].1)
            .sum();
        let short_total_balance: Decimal = short_indices
            .iter()
            .map(|&i| wallet_balances[i].1)
            .sum();
        
        // To achieve market neutrality, we need equal USD value on both sides
        // We'll use the smaller of the two totals as the maximum we can trade
        let tradeable_amount = long_total_balance.min(short_total_balance);
        
        // Generate allocations for longs
        let mut allocations = Vec::new();
        
        for &idx in long_indices {
            let (wallet_id, balance) = wallet_balances[idx];
            // Proportional allocation based on wallet's balance relative to its side's total
            let proportion = balance / long_total_balance;
            let usdc_amount = tradeable_amount * proportion;
            let percentage = (usdc_amount / balance) * Decimal::from(100);
            
            allocations.push(WalletAllocation {
                wallet_id,
                side: PositionSide::Long,
                usdc_amount,
                percentage,
            });
        }
        
        for &idx in short_indices {
            let (wallet_id, balance) = wallet_balances[idx];
            // Proportional allocation based on wallet's balance relative to its side's total
            let proportion = balance / short_total_balance;
            let usdc_amount = tradeable_amount * proportion;
            let percentage = (usdc_amount / balance) * Decimal::from(100);
            
            allocations.push(WalletAllocation {
                wallet_id,
                side: PositionSide::Short,
                usdc_amount,
                percentage,
            });
        }
        
        // Log the allocation strategy
        info!("Generated balanced allocation strategy:");
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
}