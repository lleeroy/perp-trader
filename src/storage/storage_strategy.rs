#![allow(unused)]

use crate::error::TradingError;
use crate::trader::strategy::{TradingStrategy, StrategyStatus};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;
use std::path::Path;
use std::str::FromStr;

/// SQLite-based storage for trading strategies
pub struct StrategyStorage {
    conn: Connection,
}

impl StrategyStorage {
    /// Create a new storage instance with a database file
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, TradingError> {
        let conn = Connection::open(db_path)?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<(), TradingError> {
        // Create strategies table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS strategies (
                id TEXT PRIMARY KEY,
                token_symbol TEXT NOT NULL,
                longs_size TEXT NOT NULL,
                shorts_size TEXT NOT NULL,
                status TEXT NOT NULL,
                opened_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                close_at TEXT NOT NULL,
                closed_at TEXT,
                realized_pnl TEXT,
                long_position_ids TEXT NOT NULL,
                short_position_ids TEXT NOT NULL
            )",
            [],
        )?;

        // Create index for faster lookups
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_strategy_status 
             ON strategies(status)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_strategy_close_at 
             ON strategies(close_at)",
            [],
        )?;

        Ok(())
    }

    /// Save or update a strategy
    pub fn save_strategy(&self, strategy: &TradingStrategy) -> Result<(), TradingError> {
        let long_position_ids = strategy.longs
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>()
            .join(",");
        
        let short_position_ids = strategy.shorts
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>()
            .join(",");

        self.conn.execute(
            "INSERT OR REPLACE INTO strategies 
             (id, token_symbol, longs_size, shorts_size, status, opened_at, updated_at, close_at, closed_at, realized_pnl, long_position_ids, short_position_ids)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                strategy.id,
                strategy.token_symbol,
                strategy.longs_size.to_string(),
                strategy.shorts_size.to_string(),
                strategy.status.to_string(),
                strategy.opened_at.to_rfc3339(),
                strategy.updated_at.to_rfc3339(),
                strategy.close_at.to_rfc3339(),
                strategy.closed_at.map(|dt| dt.to_rfc3339()),
                strategy.realized_pnl.map(|pnl| pnl.to_string()),
                long_position_ids,
                short_position_ids,
            ],
        )?;
        Ok(())
    }

    /// Get a strategy by ID (without loading full position details)
    pub fn get_strategy_metadata(&self, id: &str) -> Result<Option<StrategyMetadata>, TradingError> {
        let metadata = self
            .conn
            .query_row(
                "SELECT id, token_symbol, longs_size, shorts_size, status, opened_at, updated_at, close_at, closed_at, realized_pnl, long_position_ids, short_position_ids
                 FROM strategies WHERE id = ?1",
                params![id],
                |row| {
                    Ok(StrategyMetadata {
                        id: row.get(0)?,
                        token_symbol: row.get(1)?,
                        longs_size: Decimal::from_str(&row.get::<_, String>(2)?).unwrap(),
                        shorts_size: Decimal::from_str(&row.get::<_, String>(3)?).unwrap(),
                        status: StrategyStatus::from_str(&row.get::<_, String>(4)?).unwrap(),
                        opened_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                            .unwrap()
                            .with_timezone(&Utc),
                        updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                            .unwrap()
                            .with_timezone(&Utc),
                        close_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                            .unwrap()
                            .with_timezone(&Utc),
                        closed_at: row
                            .get::<_, Option<String>>(8)?
                            .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                        realized_pnl: row
                            .get::<_, Option<String>>(9)?
                            .and_then(|s| Decimal::from_str(&s).ok()),
                        long_position_ids: parse_position_ids(&row.get::<_, String>(10)?),
                        short_position_ids: parse_position_ids(&row.get::<_, String>(11)?),
                    })
                },
            )
            .optional()?;

        Ok(metadata)
    }

    /// Get all active strategies (Running or Closing status)
    pub fn get_active_strategies(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, token_symbol, longs_size, shorts_size, status, opened_at, updated_at, close_at, closed_at, realized_pnl, long_position_ids, short_position_ids
             FROM strategies WHERE status IN ('RUNNING', 'CLOSING') ORDER BY close_at ASC",
        )?;

        let strategies = stmt
            .query_map([], |row| {
                Ok(StrategyMetadata {
                    id: row.get(0)?,
                    token_symbol: row.get(1)?,
                    longs_size: Decimal::from_str(&row.get::<_, String>(2)?).unwrap(),
                    shorts_size: Decimal::from_str(&row.get::<_, String>(3)?).unwrap(),
                    status: StrategyStatus::from_str(&row.get::<_, String>(4)?).unwrap(),
                    opened_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    close_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    closed_at: row
                        .get::<_, Option<String>>(8)?
                        .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                    realized_pnl: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    long_position_ids: parse_position_ids(&row.get::<_, String>(10)?),
                    short_position_ids: parse_position_ids(&row.get::<_, String>(11)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(strategies)
    }

    /// Get strategies that should be closed (close_at <= now and status = Running)
    pub fn get_strategies_to_close(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT id, token_symbol, longs_size, shorts_size, status, opened_at, updated_at, close_at, closed_at, realized_pnl, long_position_ids, short_position_ids
             FROM strategies WHERE status = 'RUNNING' AND close_at <= ?1 ORDER BY close_at ASC",
        )?;

        let strategies = stmt
            .query_map(params![now], |row| {
                Ok(StrategyMetadata {
                    id: row.get(0)?,
                    token_symbol: row.get(1)?,
                    longs_size: Decimal::from_str(&row.get::<_, String>(2)?).unwrap(),
                    shorts_size: Decimal::from_str(&row.get::<_, String>(3)?).unwrap(),
                    status: StrategyStatus::from_str(&row.get::<_, String>(4)?).unwrap(),
                    opened_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    close_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    closed_at: row
                        .get::<_, Option<String>>(8)?
                        .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                    realized_pnl: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    long_position_ids: parse_position_ids(&row.get::<_, String>(10)?),
                    short_position_ids: parse_position_ids(&row.get::<_, String>(11)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(strategies)
    }

    /// Update strategy status and related fields
    pub fn update_strategy_status(
        &self,
        id: &str,
        status: StrategyStatus,
        closed_at: Option<DateTime<Utc>>,
        realized_pnl: Option<Decimal>,
    ) -> Result<(), TradingError> {
        let updated_at = Utc::now();

        self.conn.execute(
            "UPDATE strategies 
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

    /// Get all strategies
    pub fn get_all_strategies(&self) -> Result<Vec<StrategyMetadata>, TradingError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, token_symbol, longs_size, shorts_size, status, opened_at, updated_at, close_at, closed_at, realized_pnl, long_position_ids, short_position_ids
             FROM strategies ORDER BY opened_at DESC",
        )?;

        let strategies = stmt
            .query_map([], |row| {
                Ok(StrategyMetadata {
                    id: row.get(0)?,
                    token_symbol: row.get(1)?,
                    longs_size: Decimal::from_str(&row.get::<_, String>(2)?).unwrap(),
                    shorts_size: Decimal::from_str(&row.get::<_, String>(3)?).unwrap(),
                    status: StrategyStatus::from_str(&row.get::<_, String>(4)?).unwrap(),
                    opened_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    close_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    closed_at: row
                        .get::<_, Option<String>>(8)?
                        .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                    realized_pnl: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| Decimal::from_str(&s).ok()),
                    long_position_ids: parse_position_ids(&row.get::<_, String>(10)?),
                    short_position_ids: parse_position_ids(&row.get::<_, String>(11)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(strategies)
    }
}

/// Lightweight strategy metadata without full position details
#[derive(Debug, Clone)]
pub struct StrategyMetadata {
    pub id: String,
    pub token_symbol: String,
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

