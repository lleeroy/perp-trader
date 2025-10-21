#![allow(unused)]

pub mod storage_position;
pub mod storage_strategy;

use crate::config::AppConfig;
use crate::error::TradingError;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Initialize a PostgreSQL connection pool from config
pub async fn init_pool(config: &AppConfig) -> Result<PgPool, TradingError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await?;
    
    Ok(pool)
}
