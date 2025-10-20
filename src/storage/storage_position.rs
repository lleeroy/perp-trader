#![allow(unused)]

use crate::error::TradingError;
use crate::model::exchange::Exchange;
use crate::model::position::{Position, PositionSide, PositionStatus};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::str::FromStr;

/// PostgreSQL-based storage for positions
pub struct PositionStorage {
    pool: PgPool,
}

impl PositionStorage {
    /// Create a new storage instance with a database pool
    pub async fn new(pool: PgPool) -> Result<Self, TradingError> {
        let storage = Self { pool };
        storage.init_schema().await?;
        Ok(storage)
    }

    /// Initialize the database schema
    async fn init_schema(&self) -> Result<(), TradingError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS positions (
                id TEXT PRIMARY KEY,
                wallet_id SMALLINT NOT NULL,
                strategy_id TEXT,
                exchange TEXT NOT NULL,
                symbol TEXT NOT NULL,
                side TEXT NOT NULL,
                size TEXT NOT NULL,
                status TEXT NOT NULL,
                opened_at TIMESTAMPTZ NOT NULL,
                close_at TIMESTAMPTZ NOT NULL,
                closed_at TIMESTAMPTZ,
                realized_pnl TEXT,
                updated_at TIMESTAMPTZ NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create an index for faster lookups
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_exchange_status 
            ON positions(exchange, status)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_strategy_id 
            ON positions(strategy_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Save or update a position
    pub async fn save_position(&self, position: &Position) -> Result<(), TradingError> {
        sqlx::query(
            r#"
            INSERT INTO positions 
            (id, wallet_id, strategy_id, exchange, symbol, side, size, status, opened_at, close_at, closed_at, realized_pnl, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (id) DO UPDATE SET
                wallet_id = EXCLUDED.wallet_id,
                strategy_id = EXCLUDED.strategy_id,
                exchange = EXCLUDED.exchange,
                symbol = EXCLUDED.symbol,
                side = EXCLUDED.side,
                size = EXCLUDED.size,
                status = EXCLUDED.status,
                opened_at = EXCLUDED.opened_at,
                close_at = EXCLUDED.close_at,
                closed_at = EXCLUDED.closed_at,
                realized_pnl = EXCLUDED.realized_pnl,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&position.id)
        .bind(position.wallet_id as i16)
        .bind(&position.strategy_id)
        .bind(position.exchange.to_string())
        .bind(&position.symbol)
        .bind(position.side.to_string())
        .bind(position.size.to_string())
        .bind(position.status.to_string())
        .bind(position.opened_at)
        .bind(position.close_at)
        .bind(position.closed_at)
        .bind(position.realized_pnl.map(|pnl| pnl.to_string()))
        .bind(position.updated_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a position by ID
    pub async fn get_position(&self, id: &str) -> Result<Option<Position>, TradingError> {
        let row = sqlx::query(
            r#"
            SELECT id, wallet_id, strategy_id, exchange, symbol, side, size, status, 
                   opened_at, close_at, closed_at, realized_pnl, updated_at
            FROM positions WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(Position {
                wallet_id: row.try_get::<i16, _>("wallet_id")? as u8,
                id: row.try_get("id")?,
                strategy_id: row.try_get("strategy_id")?,
                exchange: Exchange::from_str(row.try_get("exchange")?).unwrap(),
                symbol: row.try_get("symbol")?,
                side: PositionSide::from_str(row.try_get("side")?).unwrap(),
                size: Decimal::from_str(row.try_get("size")?).unwrap(),
                status: PositionStatus::from_str(row.try_get("status")?).unwrap(),
                opened_at: row.try_get("opened_at")?,
                close_at: row.try_get("close_at")?,
                closed_at: row.try_get("closed_at")?,
                realized_pnl: row
                    .try_get::<Option<String>, _>("realized_pnl")?
                    .and_then(|s| Decimal::from_str(&s).ok()),
                updated_at: row.try_get("updated_at")?,
            })),
            None => Ok(None),
        }
    }

    /// Get all positions
    pub async fn get_all_positions(&self) -> Result<Vec<Position>, TradingError> {
        let rows = sqlx::query(
            r#"
            SELECT id, wallet_id, strategy_id, exchange, symbol, side, size, status, 
                   opened_at, close_at, closed_at, realized_pnl, updated_at
            FROM positions ORDER BY opened_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let positions = rows
            .iter()
            .map(|row| {
                Ok(Position {
                    wallet_id: row.try_get::<i16, _>("wallet_id")? as u8,
                    id: row.try_get("id")?,
                    strategy_id: row.try_get("strategy_id")?,
                    exchange: Exchange::from_str(row.try_get("exchange")?).unwrap(),
                    symbol: row.try_get("symbol")?,
                    side: PositionSide::from_str(row.try_get("side")?).unwrap(),
                    size: Decimal::from_str(row.try_get("size")?).unwrap(),
                    status: PositionStatus::from_str(row.try_get("status")?).unwrap(),
                    opened_at: row.try_get("opened_at")?,
                    close_at: row.try_get("close_at")?,
                    closed_at: row.try_get("closed_at")?,
                    realized_pnl: row
                        .try_get::<Option<String>, _>("realized_pnl")?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect::<Result<Vec<_>, TradingError>>()?;

        Ok(positions)
    }

    /// Get positions by exchange
    pub async fn get_positions_by_exchange(
        &self,
        exchange: Exchange,
    ) -> Result<Vec<Position>, TradingError> {
        let rows = sqlx::query(
            r#"
            SELECT id, wallet_id, strategy_id, exchange, symbol, side, size, status, 
                   opened_at, close_at, closed_at, realized_pnl, updated_at
            FROM positions WHERE exchange = $1 ORDER BY opened_at DESC
            "#,
        )
        .bind(exchange.to_string())
        .fetch_all(&self.pool)
        .await?;

        let positions = rows
            .iter()
            .map(|row| {
                Ok(Position {
                    wallet_id: row.try_get::<i16, _>("wallet_id")? as u8,
                    id: row.try_get("id")?,
                    strategy_id: row.try_get("strategy_id")?,
                    exchange: Exchange::from_str(row.try_get("exchange")?).unwrap(),
                    symbol: row.try_get("symbol")?,
                    side: PositionSide::from_str(row.try_get("side")?).unwrap(),
                    size: Decimal::from_str(row.try_get("size")?).unwrap(),
                    status: PositionStatus::from_str(row.try_get("status")?).unwrap(),
                    opened_at: row.try_get("opened_at")?,
                    close_at: row.try_get("close_at")?,
                    closed_at: row.try_get("closed_at")?,
                    realized_pnl: row
                        .try_get::<Option<String>, _>("realized_pnl")?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect::<Result<Vec<_>, TradingError>>()?;

        Ok(positions)
    }

    /// Get active positions (Open or Closing status)
    pub async fn get_active_positions(&self) -> Result<Vec<Position>, TradingError> {
        let rows = sqlx::query(
            r#"
            SELECT id, wallet_id, strategy_id, exchange, symbol, side, size, status, 
                   opened_at, close_at, closed_at, realized_pnl, updated_at
            FROM positions WHERE status IN ('OPEN', 'CLOSING') ORDER BY opened_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let positions = rows
            .iter()
            .map(|row| {
                Ok(Position {
                    wallet_id: row.try_get::<i16, _>("wallet_id")? as u8,
                    id: row.try_get("id")?,
                    strategy_id: row.try_get("strategy_id")?,
                    exchange: Exchange::from_str(row.try_get("exchange")?).unwrap(),
                    symbol: row.try_get("symbol")?,
                    side: PositionSide::from_str(row.try_get("side")?).unwrap(),
                    size: Decimal::from_str(row.try_get("size")?).unwrap(),
                    status: PositionStatus::from_str(row.try_get("status")?).unwrap(),
                    opened_at: row.try_get("opened_at")?,
                    close_at: row.try_get("close_at")?,
                    closed_at: row.try_get("closed_at")?,
                    realized_pnl: row
                        .try_get::<Option<String>, _>("realized_pnl")?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect::<Result<Vec<_>, TradingError>>()?;

        Ok(positions)
    }

    /// Update position status and related fields
    pub async fn update_position_status(
        &self,
        id: &str,
        status: PositionStatus,
        closed_at: Option<DateTime<Utc>>,
        realized_pnl: Option<Decimal>,
    ) -> Result<(), TradingError> {
        let updated_at = Utc::now();

        sqlx::query(
            r#"
            UPDATE positions 
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

    /// Delete a position by ID (use sparingly - prefer status updates)
    pub async fn delete_position(&self, id: &str) -> Result<(), TradingError> {
        sqlx::query("DELETE FROM positions WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
