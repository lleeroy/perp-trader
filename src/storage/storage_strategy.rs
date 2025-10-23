use crate::error::TradingError;
use crate::trader::strategy::{StrategyStatus, TradingStrategy};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
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
