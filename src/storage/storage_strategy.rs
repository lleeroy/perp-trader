#![allow(unused)]

use crate::error::TradingError;
use crate::trader::strategy::{StrategyStatus, TradingStrategy};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use sqlx::{PgPool, Row};
use std::str::FromStr;

/// PostgreSQL-based storage for trading strategies
pub struct StrategyStorage {
    pool: PgPool,
}

impl StrategyStorage {
    /// Create a new storage instance with a database pool
    pub async fn new(pool: PgPool) -> Result<Self, TradingError> {
        let storage = Self { pool };
        storage.init_schema().await?;
        Ok(storage)
    }

    /// Initialize the database schema
    async fn init_schema(&self) -> Result<(), TradingError> {
        // Create strategies table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS strategies (
                id TEXT PRIMARY KEY,
                token_symbol TEXT NOT NULL,
                wallet_ids TEXT NOT NULL,
                longs_size TEXT NOT NULL,
                shorts_size TEXT NOT NULL,
                status TEXT NOT NULL,
                opened_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL,
                close_at TIMESTAMPTZ NOT NULL,
                closed_at TIMESTAMPTZ,
                realized_pnl TEXT,
                long_position_ids TEXT NOT NULL,
                short_position_ids TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create index for faster lookups
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_strategy_status 
            ON strategies(status)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_strategy_close_at 
            ON strategies(close_at)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Save or update a strategy
    pub async fn save_strategy(&self, strategy: &TradingStrategy) -> Result<(), TradingError> {
        let wallet_ids = strategy
            .wallet_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let long_position_ids = strategy
            .longs
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>()
            .join(",");

        let short_position_ids = strategy
            .shorts
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>()
            .join(",");

        sqlx::query(
            r#"
            INSERT INTO strategies 
            (id, token_symbol, wallet_ids, longs_size, shorts_size, status, opened_at, updated_at, close_at, closed_at, realized_pnl, long_position_ids, short_position_ids)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (id) DO UPDATE SET
                token_symbol = EXCLUDED.token_symbol,
                wallet_ids = EXCLUDED.wallet_ids,
                longs_size = EXCLUDED.longs_size,
                shorts_size = EXCLUDED.shorts_size,
                status = EXCLUDED.status,
                opened_at = EXCLUDED.opened_at,
                updated_at = EXCLUDED.updated_at,
                close_at = EXCLUDED.close_at,
                closed_at = EXCLUDED.closed_at,
                realized_pnl = EXCLUDED.realized_pnl,
                long_position_ids = EXCLUDED.long_position_ids,
                short_position_ids = EXCLUDED.short_position_ids
            "#,
        )
        .bind(&strategy.id)
        .bind(&strategy.token_symbol)
        .bind(wallet_ids)
        .bind(strategy.longs_size.to_string())
        .bind(strategy.shorts_size.to_string())
        .bind(strategy.status.to_string())
        .bind(strategy.opened_at)
        .bind(strategy.updated_at)
        .bind(strategy.close_at)
        .bind(strategy.closed_at)
        .bind(strategy.realized_pnl.map(|pnl| pnl.to_string()))
        .bind(long_position_ids)
        .bind(short_position_ids)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a strategy by ID (without loading full position details)
    pub async fn get_strategy_metadata(
        &self,
        id: &str,
    ) -> Result<Option<StrategyMetadata>, TradingError> {
        let row = sqlx::query(
            r#"
            SELECT id, token_symbol, wallet_ids, longs_size, shorts_size, status, opened_at, updated_at, 
                   close_at, closed_at, realized_pnl, long_position_ids, short_position_ids
            FROM strategies WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(StrategyMetadata {
                id: row.try_get("id")?,
                token_symbol: row.try_get("token_symbol")?,
                wallet_ids: parse_wallet_ids(row.try_get("wallet_ids")?),
                longs_size: Decimal::from_str(row.try_get("longs_size")?).unwrap(),
                shorts_size: Decimal::from_str(row.try_get("shorts_size")?).unwrap(),
                status: StrategyStatus::from_str(row.try_get("status")?).unwrap(),
                opened_at: row.try_get("opened_at")?,
                updated_at: row.try_get("updated_at")?,
                close_at: row.try_get("close_at")?,
                closed_at: row.try_get("closed_at")?,
                realized_pnl: row
                    .try_get::<Option<String>, _>("realized_pnl")?
                    .and_then(|s| Decimal::from_str(&s).ok()),
                long_position_ids: parse_position_ids(row.try_get("long_position_ids")?),
                short_position_ids: parse_position_ids(row.try_get("short_position_ids")?),
            })),
            None => Ok(None),
        }
    }

    /// Get all active strategies (Running or Closing status)
    pub async fn get_active_strategies(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        let rows = sqlx::query(
            r#"
            SELECT id, token_symbol, wallet_ids, longs_size, shorts_size, status, opened_at, updated_at, 
                   close_at, closed_at, realized_pnl, long_position_ids, short_position_ids
            FROM strategies WHERE status IN ('RUNNING', 'CLOSING') ORDER BY close_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let strategies = rows
            .iter()
            .map(|row| {
                Ok(StrategyMetadata {
                    id: row.try_get("id")?,
                    token_symbol: row.try_get("token_symbol")?,
                    wallet_ids: parse_wallet_ids(row.try_get("wallet_ids")?),
                    longs_size: Decimal::from_str(row.try_get("longs_size")?).unwrap(),
                    shorts_size: Decimal::from_str(row.try_get("shorts_size")?).unwrap(),
                    status: StrategyStatus::from_str(row.try_get("status")?).unwrap(),
                    opened_at: row.try_get("opened_at")?,
                    updated_at: row.try_get("updated_at")?,
                    close_at: row.try_get("close_at")?,
                    closed_at: row.try_get("closed_at")?,
                    realized_pnl: row
                        .try_get::<Option<String>, _>("realized_pnl")?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    long_position_ids: parse_position_ids(row.try_get("long_position_ids")?),
                    short_position_ids: parse_position_ids(row.try_get("short_position_ids")?),
                })
            })
            .collect::<Result<Vec<_>, TradingError>>()?;

        Ok(strategies)
    }

    /// Get strategies that should be closed (close_at <= now and status = Running)
    pub async fn get_strategies_to_close(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        let now = Utc::now();
        let rows = sqlx::query(
            r#"
            SELECT id, token_symbol, wallet_ids, longs_size, shorts_size, status, opened_at, updated_at, 
                   close_at, closed_at, realized_pnl, long_position_ids, short_position_ids
            FROM strategies WHERE status = 'RUNNING' AND close_at <= $1 ORDER BY close_at ASC
            "#,
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;

        let strategies = rows
            .iter()
            .map(|row| {
                Ok(StrategyMetadata {
                    id: row.try_get("id")?,
                    token_symbol: row.try_get("token_symbol")?,
                    wallet_ids: parse_wallet_ids(row.try_get("wallet_ids")?),
                    longs_size: Decimal::from_str(row.try_get("longs_size")?).unwrap(),
                    shorts_size: Decimal::from_str(row.try_get("shorts_size")?).unwrap(),
                    status: StrategyStatus::from_str(row.try_get("status")?).unwrap(),
                    opened_at: row.try_get("opened_at")?,
                    updated_at: row.try_get("updated_at")?,
                    close_at: row.try_get("close_at")?,
                    closed_at: row.try_get("closed_at")?,
                    realized_pnl: row
                        .try_get::<Option<String>, _>("realized_pnl")?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    long_position_ids: parse_position_ids(row.try_get("long_position_ids")?),
                    short_position_ids: parse_position_ids(row.try_get("short_position_ids")?),
                })
            })
            .collect::<Result<Vec<_>, TradingError>>()?;

        Ok(strategies)
    }

    /// Update strategy status and related fields
    pub async fn update_strategy_status(
        &self,
        id: &str,
        status: StrategyStatus,
        closed_at: Option<DateTime<Utc>>,
        realized_pnl: Option<Decimal>,
    ) -> Result<(), TradingError> {
        let updated_at = Utc::now();

        sqlx::query(
            r#"
            UPDATE strategies 
            SET status = $1, closed_at = $2, realized_pnl = $3, updated_at = $4
            WHERE id = $5
            "#,
        )
        .bind(status.to_string())
        .bind(closed_at)
        .bind(realized_pnl.map(|pnl| pnl.to_string()))
        .bind(updated_at)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all strategies
    pub async fn get_all_strategies(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        let rows = sqlx::query(
            r#"
            SELECT id, token_symbol, wallet_ids, longs_size, shorts_size, status, opened_at, updated_at, 
                   close_at, closed_at, realized_pnl, long_position_ids, short_position_ids
            FROM strategies ORDER BY opened_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let strategies = rows
            .iter()
            .map(|row| {
                Ok(StrategyMetadata {
                    id: row.try_get("id")?,
                    token_symbol: row.try_get("token_symbol")?,
                    wallet_ids: parse_wallet_ids(row.try_get("wallet_ids")?),
                    longs_size: Decimal::from_str(row.try_get("longs_size")?).unwrap(),
                    shorts_size: Decimal::from_str(row.try_get("shorts_size")?).unwrap(),
                    status: StrategyStatus::from_str(row.try_get("status")?).unwrap(),
                    opened_at: row.try_get("opened_at")?,
                    updated_at: row.try_get("updated_at")?,
                    close_at: row.try_get("close_at")?,
                    closed_at: row.try_get("closed_at")?,
                    realized_pnl: row
                        .try_get::<Option<String>, _>("realized_pnl")?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    long_position_ids: parse_position_ids(row.try_get("long_position_ids")?),
                    short_position_ids: parse_position_ids(row.try_get("short_position_ids")?),
                })
            })
            .collect::<Result<Vec<_>, TradingError>>()?;

        Ok(strategies)
    }
}

/// Lightweight strategy metadata without full position details
#[derive(Debug, Clone)]
pub struct StrategyMetadata {
    pub id: String,
    pub token_symbol: String,
    pub wallet_ids: Vec<u8>,
    pub longs_size: Decimal,
    pub shorts_size: Decimal,
    pub status: StrategyStatus,
    pub opened_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub close_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub realized_pnl: Option<Decimal>,
    pub long_position_ids: Vec<String>,
    pub short_position_ids: Vec<String>,
}

impl StrategyMetadata {
    /// Check if the strategy should be closed based on current time
    pub fn should_close(&self) -> bool {
        Utc::now() >= self.close_at && self.status == StrategyStatus::Running
    }

    /// Get all position IDs in this strategy
    pub fn get_all_position_ids(&self) -> Vec<String> {
        self.long_position_ids
            .iter()
            .chain(self.short_position_ids.iter())
            .cloned()
            .collect()
    }

    /// Check the current status of the strategy and determine if any action is needed
    /// Returns a tuple of (current_status, needs_action, action_reason)
    pub fn check_strategy_status(&self, current_time: DateTime<Utc>) -> (StrategyStatus, bool, Option<String>) {
        match self.status {
            StrategyStatus::Running => {
                // Check if strategy should be closed due to time
                if current_time >= self.close_at {
                    return (
                        StrategyStatus::Running,
                        true,
                        Some(format!("Strategy reached close time: {}", self.close_at))
                    );
                }
                
                // Check if strategy has been open for too long (safety check)
                let max_duration = chrono::Duration::hours(24); // 24 hours max
                if current_time - self.opened_at > max_duration {
                    return (
                        StrategyStatus::Running,
                        true,
                        Some("Strategy exceeded maximum duration (24 hours)".to_string())
                    );
                }
                
                // Strategy is running normally
                (StrategyStatus::Running, false, None)
            }
            
            StrategyStatus::Closing => {
                // If strategy is already closing, check if it's taking too long
                let closing_timeout = chrono::Duration::minutes(10); // 10 minutes max for closing
                if let Some(started_closing_at) = self.updated_at.checked_add_signed(closing_timeout) {
                    if current_time > started_closing_at {
                        return (
                            StrategyStatus::Closing,
                            true,
                            Some("Strategy closing is taking too long".to_string())
                        );
                    }
                }
                
                (StrategyStatus::Closing, false, None)
            }
            
            StrategyStatus::Closed | StrategyStatus::Failed => {
                // No action needed for completed strategies
                (self.status, false, None)
            }
        }
    }

    /// Check if the strategy is currently active (running or closing)
    pub fn is_active(&self) -> bool {
        matches!(self.status, StrategyStatus::Running | StrategyStatus::Closing)
    }

    /// Check if the strategy is completed (closed or failed)
    pub fn is_completed(&self) -> bool {
        matches!(self.status, StrategyStatus::Closed | StrategyStatus::Failed)
    }

    /// Check if the strategy should be force-closed due to emergency conditions
    pub fn should_force_close(&self, current_time: DateTime<Utc>) -> bool {
        // Force close if strategy has been in closing state for too long
        if self.status == StrategyStatus::Closing {
            let closing_timeout = chrono::Duration::minutes(15); // 15 minutes max
            if let Some(max_closing_time) = self.updated_at.checked_add_signed(closing_timeout) {
                return current_time > max_closing_time;
            }
        }
        
        false
    }

    /// Get time until strategy close
    pub fn time_until_close(&self, current_time: DateTime<Utc>) -> Option<chrono::Duration> {
        if current_time < self.close_at {
            Some(self.close_at - current_time)
        } else {
            None
        }
    }

    /// Format time until close as human-readable string
    pub fn format_time_until_close(&self, current_time: DateTime<Utc>) -> String {
        match self.time_until_close(current_time) {
            Some(duration) => {
                let total_seconds = duration.num_seconds();
                let hours = total_seconds / 3600;
                let minutes = (total_seconds % 3600) / 60;
                let seconds = total_seconds % 60;
                
                if hours > 0 {
                    format!("{}h {}m {}s", hours, minutes, seconds)
                } else if minutes > 0 {
                    format!("{}m {}s", minutes, seconds)
                } else {
                    format!("{}s", seconds)
                }
            }
            None => "CLOSED".to_string()
        }
    }

    /// Get the total position size (longs + shorts)
    pub fn total_position_size(&self) -> Decimal {
        self.longs_size + self.shorts_size
    }

    /// Get the net position size (longs - shorts)
    pub fn net_position_size(&self) -> Decimal {
        self.longs_size - self.shorts_size
    }

    /// Check if the strategy is market neutral (longs ≈ shorts)
    pub fn is_market_neutral(&self, tolerance: Option<Decimal>) -> bool {
        let tolerance = tolerance.unwrap_or(Decimal::from_f64(0.05).unwrap()); // 5% tolerance by default
        let net_size = self.net_position_size().abs();
        let total_size = self.total_position_size();
        
        if total_size.is_zero() {
            true
        } else {
            net_size / total_size <= tolerance
        }
    }

    /// Get strategy duration so far
    pub fn duration_so_far(&self, current_time: DateTime<Utc>) -> chrono::Duration {
        current_time - self.opened_at
    }

    /// Check if strategy has been open longer than specified duration
    pub fn has_exceeded_duration(&self, max_duration: chrono::Duration, current_time: DateTime<Utc>) -> bool {
        self.duration_so_far(current_time) > max_duration
    }

    /// Get strategy age in minutes
    pub fn age_minutes(&self, current_time: DateTime<Utc>) -> i64 {
        self.duration_so_far(current_time).num_minutes()
    }

    /// Get strategy age in hours
    pub fn age_hours(&self, current_time: DateTime<Utc>) -> i64 {
        self.duration_so_far(current_time).num_hours()
    }

    /// Check if strategy has realized PnL
    pub fn has_realized_pnl(&self) -> bool {
        self.realized_pnl.is_some()
    }

    /// Get realized PnL or zero if none
    pub fn realized_pnl_or_zero(&self) -> Decimal {
        self.realized_pnl.unwrap_or(Decimal::ZERO)
    }

    /// Check if strategy was profitable
    pub fn was_profitable(&self) -> Option<bool> {
        self.realized_pnl.map(|pnl| pnl > Decimal::ZERO)
    }

    /// Get the number of wallets in this strategy
    pub fn wallet_count(&self) -> usize {
        self.wallet_ids.len()
    }

    /// Get the number of long positions
    pub fn long_position_count(&self) -> usize {
        self.long_position_ids.len()
    }

    /// Get the number of short positions
    pub fn short_position_count(&self) -> usize {
        self.short_position_ids.len()
    }

    /// Get total number of positions
    pub fn total_position_count(&self) -> usize {
        self.long_position_count() + self.short_position_count()
    }

    /// Check if strategy has any positions
    pub fn has_positions(&self) -> bool {
        !self.long_position_ids.is_empty() || !self.short_position_ids.is_empty()
    }

    /// Check if strategy has both long and short positions
    pub fn has_both_sides(&self) -> bool {
        !self.long_position_ids.is_empty() && !self.short_position_ids.is_empty()
    }

    /// Get strategy efficiency ratio (min(longs, shorts) / max(longs, shorts))
    /// Higher values indicate better balance between long and short sides
    pub fn efficiency_ratio(&self) -> Decimal {
        if self.longs_size.is_zero() && self.shorts_size.is_zero() {
            return Decimal::ONE;
        }
        
        let min_size = self.longs_size.min(self.shorts_size);
        let max_size = self.longs_size.max(self.shorts_size);
        
        min_size / max_size
    }

    /// Create a simple string representation for logging
    pub fn to_log_string(&self, current_time: DateTime<Utc>) -> String {
        let time_until_close = self.format_time_until_close(current_time);
        let age_minutes = self.age_minutes(current_time);
        let efficiency = self.efficiency_ratio();
        
        format!(
            "{} [{}] | Token: {} | Wallets: {} | Positions: {}L/{}S | Size: {:.2}/{:.2} | Close in: {} | Age: {}min | Eff: {:.1}%",
            self.id,
            self.status,
            self.token_symbol,
            self.wallet_count(),
            self.long_position_count(),
            self.short_position_count(),
            self.longs_size,
            self.shorts_size,
            time_until_close,
            age_minutes,
            efficiency * Decimal::from(100)
        )
    }
}


/// Parse comma-separated position IDs
fn parse_position_ids(ids_str: &str) -> Vec<String> {
    if ids_str.is_empty() {
        Vec::new()
    } else {
        ids_str.split(',').map(|s| s.to_string()).collect()
    }
}

/// Parse comma-separated wallet IDs
fn parse_wallet_ids(ids_str: &str) -> Vec<u8> {
    if ids_str.is_empty() {
        Vec::new()
    } else {
        ids_str.split(',').filter_map(|s| s.parse::<u8>().ok()).collect()
    }
}
