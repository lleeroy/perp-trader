#[macro_use]
extern crate log;

mod config;
mod error;
mod model;
mod perp;
mod request;
mod trader;
mod storage;

use anyhow::{Context, Result};
use crate::config::AppConfig;
use crate::storage::init_pool;
use crate::trader::{client::TraderClient, wallet::Wallet};


#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    info!("🚀 Starting perp-trader application...");

    // Load configuration
    let config = AppConfig::load().context("Failed to load configuration")?;
    info!("✅ Configuration loaded successfully");

    // Initialize database connection pool
    info!("🔌 Connecting to database: {}", config.database.url);
    let pool = init_pool(&config).await.context("Failed to initialize database pool")?;
    info!("✅ Database connection pool initialized");

    // Initialize trader client
    info!("🔌 Initializing exchange clients...");
    let wallet = Wallet::load_from_json(1).context("Failed to load wallet")?;
    let trader_client = TraderClient::new(1, pool).await.context("Failed to create trader client")?;
    info!("✅ Trader client initialized");

    // Application ready
    info!("✅ Application initialized successfully");
    
    Ok(())
}
