pub mod storage_position;
pub mod storage_strategy;
pub mod database;

use crate::error::TradingError;
use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Initialize a PostgreSQL connection pool from config
pub async fn init_pool() -> Result<PgPool, TradingError> {
    dotenv::dotenv().ok();

    let pool = PgPoolOptions::new()
        .max_connections(std::env::var("DATABASE_MAX_CONNECTIONS")
        .context("Failed to get DATABASE_MAX_CONNECTIONS from environment variables")?
        .parse::<u32>()
        .context("Failed to parse DATABASE_MAX_CONNECTIONS from environment variables")?)
        .connect(std::env::var("DATABASE_URL")
        .context("Failed to get DATABASE_URL from environment variables")?
        .as_str())
        .await?;
    
    Ok(pool)
}
