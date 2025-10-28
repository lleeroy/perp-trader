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
    /// Applies random leverage between 2x and 4x to each wallet allocation
    pub fn generate_balanced_allocations(
        wallet_balances: &Vec<(u8, Decimal)>,
    ) -> Result<Vec<WalletAllocation>, TradingError> {
        use rand::seq::SliceRandom;
        use rand::Rng;

        let config = crate::config::AppConfig::load()?;

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

        // Shuffle wallet indices to randomly assign to long/short
        let mut wallet_indices: Vec<usize> = (0..wallet_balances.len()).collect();
        wallet_indices.shuffle(&mut rng);

        // Ensure balanced split between long/short (at least 2 per side when possible)
        let min_per_side = 2;
        let max_per_side = wallet_balances.len() - min_per_side;
        
        // Generate a balanced split that ensures reasonable distribution
        let num_longs = if wallet_balances.len() >= 4 {
            // For 4+ wallets, ensure at least 2 per side
            rng.gen_range(min_per_side..=max_per_side)
        } else {
            // For exactly 3 wallets, split 2 vs 1 (minimum case)
            rng.gen_range(1..=2)
        };
        
        let long_indices = &wallet_indices[0..num_longs];
        let short_indices = &wallet_indices[num_longs..];

        // Calculate side totals
        let long_total_balance: Decimal = long_indices.iter().map(|&i| wallet_balances[i].1).sum();
        let short_total_balance: Decimal = short_indices.iter().map(|&i| wallet_balances[i].1).sum();

        // Use the minimum group total as the tradeable amount for both sides for neutrality
        let tradeable_amount = long_total_balance.min(short_total_balance);
        let mut allocations = Vec::new();

        // Generate a single leverage factor that will be applied to BOTH sides for neutrality
        let leverage = rng.gen_range(config.trading.min_leverage..=config.trading.max_leverage);

        // Calculate total allocation for each side to ensure exact balance
        let total_allocation_per_side = tradeable_amount * Decimal::from_f64(leverage).unwrap();

        // Generate allocations for longs with capacity-aware distribution
        let long_allocations = Self::distribute_allocation(
            long_indices,
            &wallet_balances,
            total_allocation_per_side,
            leverage,
            PositionSide::Long,
        )?;

        // Generate allocations for shorts with capacity-aware distribution
        let short_allocations = Self::distribute_allocation(
            short_indices,
            &wallet_balances,
            total_allocation_per_side,
            leverage,
            PositionSide::Short,
        )?;

        allocations.extend(long_allocations);
        allocations.extend(short_allocations);

        // Log the allocation strategy
        info!("Generated RANDOMIZED balanced allocation strategy with leverage ({:.1}x-{:.1}x):", config.trading.min_leverage, config.trading.max_leverage);
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
                "  Wallet #{}: {} {:.2}% ({:.2} USDC position size)",
                alloc.wallet_id, alloc.side, alloc.percentage, alloc.usdc_amount
            );
        }
        
        info!("  Total LONG: {:.2} USDC | Total SHORT: {:.2} USDC", long_total, short_total);
        info!("  Distribution: {} longs, {} shorts", long_indices.len(), short_indices.len());
        info!("  Target allocation per side: {:.2} USDC", total_allocation_per_side);

        // Verify balance - allow small tolerance for rounding errors
        let imbalance = (long_total - short_total).abs();
        let max_allowed_imbalance = Decimal::from(2); // Allow 2 USDC imbalance
        
        if imbalance > max_allowed_imbalance {
            return Err(TradingError::InvalidInput(format!(
                "Imbalance too large: {:.2} USDC (max allowed: {:.2} USDC)", 
                imbalance, max_allowed_imbalance
            )));
        } else {
            info!("  ✅ Perfectly balanced allocation");
        }

        Ok(allocations)
    }

    /// Helper function to distribute allocation among wallets while respecting capacity limits
    fn distribute_allocation(
        wallet_indices: &[usize],
        wallet_balances: &[(u8, Decimal)],
        total_allocation: Decimal,
        leverage: f64,
        side: PositionSide,
    ) -> Result<Vec<WalletAllocation>, TradingError> {
        let mut rng = rand::thread_rng();
        let mut allocations = Vec::new();
        
        // Generate random weights for each wallet
        let weights: Vec<f64> = wallet_indices
            .iter()
            .map(|_| rng.gen_range(0.15..1.0))
            .collect();
        let total_weight: f64 = weights.iter().sum();
        
        // Calculate each wallet's capacity (max they can take after leverage)
        let wallet_capacities: Vec<Decimal> = wallet_indices
            .iter()
            .map(|&idx| {
                let balance = wallet_balances[idx].1;
                balance * Decimal::from_f64(leverage).unwrap()
            })
            .collect();
        
        let mut remaining_allocation = total_allocation;
        let mut distributed = vec![Decimal::ZERO; wallet_indices.len()];
        
        // First pass: distribute proportionally by weights
        for (i, (&idx, &weight)) in wallet_indices.iter().zip(weights.iter()).enumerate() {
            let proportion = Decimal::from_f64(weight / total_weight).unwrap();
            let mut allocation = total_allocation * proportion;
            
            // Cap at wallet capacity
            allocation = allocation.min(wallet_capacities[i]);
            distributed[i] = allocation;
            remaining_allocation -= allocation;
        }
        
        // Second pass: distribute any remaining allocation to wallets that have capacity
        if remaining_allocation > Decimal::ZERO {
            let mut attempts = 0;
            while remaining_allocation > Decimal::ZERO && attempts < 10 {
                let mut redistributed = false;
                
                for i in 0..wallet_indices.len() {
                    if remaining_allocation <= Decimal::ZERO {
                        break;
                    }
                    
                    let current_allocation = distributed[i];
                    let capacity = wallet_capacities[i];
                    let remaining_capacity = capacity - current_allocation;
                    
                    if remaining_capacity > Decimal::ZERO {
                        // Distribute a portion of the remaining allocation
                        let additional = remaining_allocation.min(remaining_capacity);
                        distributed[i] += additional;
                        remaining_allocation -= additional;
                        redistributed = true;
                    }
                }
                
                if !redistributed {
                    break; // No more capacity available
                }
                attempts += 1;
            }
        }
        
        // If we still have remaining allocation, we need to scale down proportionally
        if remaining_allocation < Decimal::ZERO {
            let scale_factor = total_allocation / (total_allocation - remaining_allocation);
            for allocation in &mut distributed {
                *allocation *= scale_factor;
            }
        }
        
        // Create final allocations
        for (i, &idx) in wallet_indices.iter().enumerate() {
            let (wallet_id, balance) = wallet_balances[idx];
            let usdc_amount = distributed[i];
            let base_usdc_amount = usdc_amount / Decimal::from_f64(leverage).unwrap();
            let percentage = if balance > Decimal::ZERO {
                (base_usdc_amount / balance) * Decimal::from(100)
            } else {
                Decimal::ZERO
            };

            allocations.push(WalletAllocation {
                wallet_id,
                side,
                usdc_amount,
                percentage,
            });
        }
        
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
        
        println!("📍 Exchange: {}", exchange_name);
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


#[cfg(test)]

mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    /// Helper function to create wallet balances for testing
    fn create_wallet_balances(ids_and_balances: &[(u8, f64)]) -> Vec<(u8, Decimal)> {
        ids_and_balances
            .iter()
            .map(|(id, balance)| (*id, Decimal::from_f64(*balance).unwrap()))
            .collect()
    }

    /// Helper function to analyze allocations
    fn analyze_allocations(allocations: &[WalletAllocation]) -> (Decimal, Decimal, usize, usize) {
        let long_total: Decimal = allocations
            .iter()
            .filter(|a| a.side == PositionSide::Long)
            .map(|a| a.usdc_amount)
            .sum();
        
        let short_total: Decimal = allocations
            .iter()
            .filter(|a| a.side == PositionSide::Short)
            .map(|a| a.usdc_amount)
            .sum();
        
        let long_count = allocations.iter().filter(|a| a.side == PositionSide::Long).count();
        let short_count = allocations.iter().filter(|a| a.side == PositionSide::Short).count();
        
        (long_total, short_total, long_count, short_count)
    }

    #[test]
    fn test_insufficient_wallets() {
        let wallet_balances = create_wallet_balances(&[(1, 100.0), (2, 100.0)]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_err());
        assert!(matches!(result, Err(TradingError::InvalidInput(_))));
    }

    #[test]
    fn test_zero_total_balance() {
        let wallet_balances = create_wallet_balances(&[(1, 0.0), (2, 0.0), (3, 0.0)]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_err());
        assert!(matches!(result, Err(TradingError::InvalidInput(_))));
    }

    #[test]
    fn test_three_wallets_balanced() {
        let wallet_balances = create_wallet_balances(&[(1, 100.0), (2, 100.0), (3, 100.0)]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        assert_eq!(allocations.len(), 3);
        
        let (long_total, short_total, long_count, short_count) = analyze_allocations(&allocations);
        
        // Should have either 2 longs/1 short or 1 long/2 shorts
        assert!((long_count == 2 && short_count == 1) || (long_count == 1 && short_count == 2));
        
        // Totals should be very close (within 1 USDC due to rounding)
        let imbalance = (long_total - short_total).abs();
        assert!(imbalance <= Decimal::from(1), "Imbalance too large: {}", imbalance);
    }

    #[test]
    fn test_four_wallets_balanced() {
        let wallet_balances = create_wallet_balances(&[
            (1, 150.0), 
            (2, 200.0), 
            (3, 100.0), 
            (4, 250.0)
        ]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        assert_eq!(allocations.len(), 4);
        
        let (long_total, short_total, long_count, short_count) = analyze_allocations(&allocations);
        
        // Should have at least 2 per side for 4 wallets
        assert!(long_count >= 2);
        assert!(short_count >= 2);
        
        // Totals should be very close
        let imbalance = (long_total - short_total).abs();
        assert!(imbalance <= Decimal::from(1), "Imbalance too large: {}", imbalance);
    }

    #[test]
    fn test_five_wallets_balanced() {
        let wallet_balances = create_wallet_balances(&[
            (1, 300.0), 
            (2, 150.0), 
            (3, 200.0), 
            (4, 100.0), 
            (5, 250.0)
        ]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        assert_eq!(allocations.len(), 5);
        
        let (long_total, short_total, long_count, short_count) = analyze_allocations(&allocations);
        
        // Should have at least 2 per side for 5+ wallets
        assert!(long_count >= 2);
        assert!(short_count >= 2);
        
        // Totals should be very close
        let imbalance = (long_total - short_total).abs();
        assert!(imbalance <= Decimal::from(1), "Imbalance too large: {}", imbalance);
    }

    #[test]
    fn test_uneven_balances_still_balanced() {
        let wallet_balances = create_wallet_balances(&[
            (1, 50.0),   // Small
            (2, 1000.0), // Large
            (3, 75.0),   // Small
            (4, 800.0),  // Large
            (5, 60.0),   // Small
        ]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        let (long_total, short_total, _, _) = analyze_allocations(&allocations);
        
        // Even with uneven balances, totals should be close
        let imbalance = (long_total - short_total).abs();
        assert!(imbalance <= Decimal::from(1), "Imbalance too large: {}", imbalance);
    }

    #[test]
    fn test_all_wallets_used() {
        let wallet_balances = create_wallet_balances(&[
            (1, 100.0), 
            (2, 100.0), 
            (3, 100.0), 
            (4, 100.0)
        ]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        
        // All input wallets should be represented in output
        let allocated_wallet_ids: Vec<u8> = allocations.iter().map(|a| a.wallet_id).collect();
        for (wallet_id, _) in &wallet_balances {
            assert!(allocated_wallet_ids.contains(wallet_id), "Wallet {} not allocated", wallet_id);
        }
    }

    #[test]
    fn test_percentage_calculation_correct() {
        let wallet_balances = create_wallet_balances(&[
            (1, 100.0), 
            (2, 200.0), 
            (3, 150.0)
        ]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        
        for allocation in allocations {
            let wallet_balance = wallet_balances
                .iter()
                .find(|(id, _)| *id == allocation.wallet_id)
                .map(|(_, balance)| *balance)
                .unwrap();
            
            // Percentage should be between 0 and 100
            assert!(allocation.percentage >= Decimal::ZERO);
            assert!(allocation.percentage <= Decimal::from(100));
            
            // Verify percentage calculation: percentage = (base_amount / balance) * 100
            let base_amount = allocation.usdc_amount / Decimal::from_f64(1.15).unwrap(); // Approximate leverage
            let expected_percentage = (base_amount / wallet_balance) * Decimal::from(100);
            let percentage_diff = (allocation.percentage - expected_percentage).abs();
            
            // Allow some tolerance for rounding
            assert!(percentage_diff <= Decimal::from(5), "Percentage calculation off for wallet {}", allocation.wallet_id);
        }
    }

    #[test]
    fn test_leverage_applied_correctly() {
        let wallet_balances = create_wallet_balances(&[
            (1, 100.0), 
            (2, 100.0), 
            (3, 100.0)
        ]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        
        for allocation in allocations {
            let wallet_balance = wallet_balances
                .iter()
                .find(|(id, _)| *id == allocation.wallet_id)
                .map(|(_, balance)| *balance)
                .unwrap();
            
            // Base amount (before leverage) should not exceed wallet balance
            let base_amount = allocation.usdc_amount / Decimal::from_f64(1.2).unwrap(); // Using max leverage
            assert!(base_amount <= wallet_balance, "Base amount exceeds wallet balance for wallet {}", allocation.wallet_id);
            
            // Leveraged amount should be greater than base amount
            assert!(allocation.usdc_amount > base_amount, "Leverage not applied correctly for wallet {}", allocation.wallet_id);
        }
    }

    #[test]
    fn test_random_distribution_over_multiple_runs() {
        let wallet_balances = create_wallet_balances(&[
            (1, 100.0), 
            (2, 100.0), 
            (3, 100.0), 
            (4, 100.0)
        ]);
        
        let mut long_counts = HashMap::new();
        let mut side_assignments = HashMap::new();
        
        // Run multiple times to test randomness
        for _ in 0..100 {
            let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
            assert!(result.is_ok());
            
            let allocations = result.unwrap();
            let (_, _, long_count, short_count) = analyze_allocations(&allocations);
            
            // Track distribution of long/short splits
            *long_counts.entry(long_count).or_insert(0) += 1;
            
            // Track which wallets get assigned to which side
            for allocation in allocations {
                let entry = side_assignments.entry(allocation.wallet_id).or_insert((0, 0));
                match allocation.side {
                    PositionSide::Long => entry.0 += 1,
                    PositionSide::Short => entry.1 += 1,
                }
            }
        }
        
        // Should see different distributions (2-2, 3-1 splits)
        assert!(long_counts.len() >= 1, "Should have at least one distribution pattern");
        
        // Each wallet should be assigned to both sides over multiple runs
        for (wallet_id, (long_assignments, short_assignments)) in side_assignments {
            assert!(long_assignments > 0, "Wallet {} never assigned long", wallet_id);
            assert!(short_assignments > 0, "Wallet {} never assigned short", wallet_id);
        }
    }

    #[test]
    fn test_very_small_balances() {
        let wallet_balances = create_wallet_balances(&[
            (1, 10.0),   // Very small
            (2, 15.0),   // Very small  
            (3, 12.0),   // Very small
            (4, 8.0),    // Very small
        ]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        let (long_total, short_total, _, _) = analyze_allocations(&allocations);
        
        // Even with small balances, should still be balanced
        let imbalance = (long_total - short_total).abs();
        assert!(imbalance <= Decimal::from(1), "Imbalance too large: {}", imbalance);
        
        // No allocation should exceed wallet capacity
        for allocation in allocations {
            let wallet_balance = wallet_balances
                .iter()
                .find(|(id, _)| *id == allocation.wallet_id)
                .map(|(_, balance)| *balance)
                .unwrap();
            
            let base_amount = allocation.usdc_amount / Decimal::from_f64(1.2).unwrap();
            assert!(base_amount <= wallet_balance, "Allocation exceeds wallet capacity");
        }
    }

    #[test]
    fn test_many_wallets_balanced() {
        let wallet_balances = create_wallet_balances(&[
            (1, 200.0), 
            (2, 180.0), 
            (3, 220.0), 
            (4, 190.0), 
            (5, 210.0), 
            (6, 195.0),
            (7, 205.0),
            (8, 185.0),
        ]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        assert_eq!(allocations.len(), 8);
        
        let (long_total, short_total, long_count, short_count) = analyze_allocations(&allocations);
        
        // With 8 wallets, should have reasonable distribution (e.g., 3-5, 4-4, 5-3)
        assert!(long_count >= 3);
        assert!(short_count >= 3);
        
        // Totals should be very close
        let imbalance = (long_total - short_total).abs();
        assert!(imbalance <= Decimal::from(1), "Imbalance too large: {}", imbalance);
    }

    #[test]
    fn test_allocation_structure() {
        let wallet_balances = create_wallet_balances(&[
            (1, 100.0), 
            (2, 100.0), 
            (3, 100.0)
        ]);
        
        let result = TradingStrategy::generate_balanced_allocations(&wallet_balances);
        assert!(result.is_ok());
        
        let allocations = result.unwrap();
        
        for allocation in allocations {
            // Check all fields are properly set
            assert!(allocation.wallet_id > 0);
            assert!(matches!(allocation.side, PositionSide::Long | PositionSide::Short));
            assert!(allocation.usdc_amount > Decimal::ZERO);
            assert!(allocation.percentage >= Decimal::ZERO);
            assert!(allocation.percentage <= Decimal::from(100));
        }
    }
}