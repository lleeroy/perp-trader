#![allow(unused)]

use crate::error::TradingError;
use crate::model::exchange::Exchange;
use crate::model::position::{Position, PositionSide, PositionStatus};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;
use std::path::Path;
use std::str::FromStr;

/// Simple SQLite-based storage for positions
pub struct PositionStorage {
    conn: Connection,
}

impl PositionStorage {
    /// Create a new storage instance with a database file
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, TradingError> {
        let conn = Connection::open(db_path)?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<(), TradingError> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS positions (
                id TEXT PRIMARY KEY,
                strategy_id TEXT,
                exchange TEXT NOT NULL,
                symbol TEXT NOT NULL,
                side TEXT NOT NULL,
                size TEXT NOT NULL,
                status TEXT NOT NULL,
                opened_at TEXT NOT NULL,
                close_at TEXT NOT NULL,
                closed_at TEXT,
                realized_pnl TEXT,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create an index for faster lookups
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_exchange_status 
             ON positions(exchange, status)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_strategy_id 
             ON positions(strategy_id)",
            [],
        )?;

        Ok(())
    }

    /// Save or update a position
    pub fn save_position(&self, position: &Position) -> Result<(), TradingError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO positions 
             (id, strategy_id, exchange, symbol, side, size, status, opened_at, close_at, closed_at, realized_pnl, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                position.id,
                position.strategy_id,
                position.exchange.to_string(),
                position.symbol,
                position.side.to_string(),
                position.size.to_string(),
                position.status.to_string(),
                position.opened_at.to_rfc3339(),
                position.close_at.to_rfc3339(),
                position.closed_at.map(|dt| dt.to_rfc3339()),
                position.realized_pnl.map(|pnl| pnl.to_string()),
                position.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Get a position by ID
    pub fn get_position(&self, id: &str) -> Result<Option<Position>, TradingError> {
        let position = self
            .conn
            .query_row(
                "SELECT id, strategy_id, exchange, symbol, side, size, status, opened_at, close_at, closed_at, realized_pnl, updated_at
                 FROM positions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Position {
                        id: row.get(0)?,
                        strategy_id: row.get(1)?,
                        exchange: Exchange::from_str(&row.get::<_, String>(2)?).unwrap(),
                        symbol: row.get(3)?,
                        side: PositionSide::from_str(&row.get::<_, String>(4)?).unwrap(),
                        size: Decimal::from_str(&row.get::<_, String>(5)?).unwrap(),
                        status: PositionStatus::from_str(&row.get::<_, String>(6)?).unwrap(),
                        opened_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                            .unwrap()
                            .with_timezone(&Utc),
                        close_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                            .unwrap()
                            .with_timezone(&Utc),
                        closed_at: row
                            .get::<_, Option<String>>(9)?
                            .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                        realized_pnl: row
                            .get::<_, Option<String>>(10)?
                            .and_then(|s| Decimal::from_str(&s).ok()),
                        updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?)
                            .unwrap()
                            .with_timezone(&Utc),
                    })
                },
            )
            .optional()?;

        Ok(position)
    }

    /// Get all positions
    pub fn get_all_positions(&self) -> Result<Vec<Position>, TradingError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, strategy_id, exchange, symbol, side, size, status, opened_at, close_at, closed_at, realized_pnl, updated_at
             FROM positions ORDER BY opened_at DESC",
        )?;

        let positions = stmt
            .query_map([], |row| {
                Ok(Position {
                    id: row.get(0)?,
                    strategy_id: row.get(1)?,
                    exchange: Exchange::from_str(&row.get::<_, String>(2)?).unwrap(),
                    symbol: row.get(3)?,
                    side: PositionSide::from_str(&row.get::<_, String>(4)?).unwrap(),
                    size: Decimal::from_str(&row.get::<_, String>(5)?).unwrap(),
                    status: PositionStatus::from_str(&row.get::<_, String>(6)?).unwrap(),
                    opened_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    close_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    closed_at: row
                        .get::<_, Option<String>>(9)?
                        .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                    realized_pnl: row
                        .get::<_, Option<String>>(10)?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(positions)
    }

    /// Get positions by exchange
    pub fn get_positions_by_exchange(&self, exchange: Exchange) -> Result<Vec<Position>, TradingError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, strategy_id, exchange, symbol, side, size, status, opened_at, close_at, closed_at, realized_pnl, updated_at
             FROM positions WHERE exchange = ?1 ORDER BY opened_at DESC",
        )?;

        let positions = stmt
            .query_map(params![exchange.to_string()], |row| {
                Ok(Position {
                    id: row.get(0)?,
                    strategy_id: row.get(1)?,
                    exchange: Exchange::from_str(&row.get::<_, String>(2)?).unwrap(),
                    symbol: row.get(3)?,
                    side: PositionSide::from_str(&row.get::<_, String>(4)?).unwrap(),
                    size: Decimal::from_str(&row.get::<_, String>(5)?).unwrap(),
                    status: PositionStatus::from_str(&row.get::<_, String>(6)?).unwrap(),
                    opened_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    close_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    closed_at: row
                        .get::<_, Option<String>>(9)?
                        .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                    realized_pnl: row
                        .get::<_, Option<String>>(10)?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(positions)
    }

    /// Get active positions (Open or Closing status)
    pub fn get_active_positions(&self) -> Result<Vec<Position>, TradingError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, strategy_id, exchange, symbol, side, size, status, opened_at, close_at, closed_at, realized_pnl, updated_at
             FROM positions WHERE status IN ('OPEN', 'CLOSING') ORDER BY opened_at DESC",
        )?;

        let positions = stmt
            .query_map([], |row| {
                Ok(Position {
                    id: row.get(0)?,
                    strategy_id: row.get(1)?,
                    exchange: Exchange::from_str(&row.get::<_, String>(2)?).unwrap(),
                    symbol: row.get(3)?,
                    side: PositionSide::from_str(&row.get::<_, String>(4)?).unwrap(),
                    size: Decimal::from_str(&row.get::<_, String>(5)?).unwrap(),
                    status: PositionStatus::from_str(&row.get::<_, String>(6)?).unwrap(),
                    opened_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    close_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    closed_at: row
                        .get::<_, Option<String>>(9)?
                        .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                    realized_pnl: row
                        .get::<_, Option<String>>(10)?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(positions)
    }

    /// Update position status and related fields
    pub fn update_position_status(
        &self,
        id: &str,
        status: PositionStatus,
        closed_at: Option<DateTime<Utc>>,
        realized_pnl: Option<Decimal>,
    ) -> Result<(), TradingError> {
        let updated_at = Utc::now();

        self.conn.execute(
            "UPDATE positions 
             SET status = ?1, closed_at = ?2, realized_pnl = ?3, updated_at = ?4
             WHERE id = ?5",
            params![
                status.to_string(),
                closed_at.map(|dt| dt.to_rfc3339()),
                realized_pnl.map(|pnl| pnl.to_string()),
                updated_at.to_rfc3339(),
                id,
            ],
        )?;

        Ok(())
    }

    /// Delete a position by ID (use sparingly - prefer status updates)
    pub fn delete_position(&self, id: &str) -> Result<(), TradingError> {
        self.conn.execute("DELETE FROM positions WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::exchange::Exchange;
    use rust_decimal_macros::dec;

    #[test]
    fn test_storage_lifecycle() {
        // Use in-memory database for testing
        let storage = PositionStorage::new(":memory:").unwrap();

        // Create a test position
        let position = Position {
            id: "test-123".to_string(),
            strategy_id: Some("strategy-abc".to_string()),
            exchange: Exchange::Backpack,
            symbol: "SOL-PERP".to_string(),
            side: PositionSide::Long,
            size: dec!(10.5),
            status: PositionStatus::Open,
            opened_at: Utc::now(),
            close_at: Utc::now(),
            closed_at: None,
            realized_pnl: None,
            updated_at: Utc::now(),
        };

        // Save position
        storage.save_position(&position).unwrap();

        // Retrieve position
        let retrieved = storage.get_position("test-123").unwrap().unwrap();
        assert_eq!(retrieved.id, position.id);
        assert_eq!(retrieved.symbol, position.symbol);

        // Update status
        storage
            .update_position_status(
                "test-123",
                PositionStatus::Closed,
                Some(Utc::now()),
                Some(dec!(50.25)),
            )
            .unwrap();

        // Verify update
        let updated = storage.get_position("test-123").unwrap().unwrap();
        assert_eq!(updated.status, PositionStatus::Closed);
        assert!(updated.closed_at.is_some());
        assert!(updated.realized_pnl.is_some());
    }
}

