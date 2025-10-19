#[macro_use]
extern crate log;

mod config;
mod error;
mod model;
mod perp;
mod request;
mod trader;

use anyhow::{Context, Result};
use std::sync::Arc;

use config::AppConfig;
use perp::backpack::BackpackClient;

use crate::trader::wallet::Wallet;


#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    pretty_env_logger::init();

    info!("🔧 Loading configuration...");
    let config = AppConfig::load().context("Failed to load configuration")?;
    let config = Arc::new(config);

    info!("🔌 Initializing exchange clients...");
    let wallet = Wallet::load_from_json(1).context("Failed to load wallet")?;
    let backpack_client: Arc<dyn perp::PerpExchange> = Arc::new(BackpackClient::new(&wallet));

    // Health check exchanges
    info!("🏥 Performing health checks...");
    match backpack_client.health_check().await {
        Ok(true) => info!("✓ Backpack is healthy"),
        Ok(false) => warn!("⚠️  Backpack health check returned false"),
        Err(e) => error!("✗ Backpack health check failed: {}", e),
    }


    info!("🚀 Starting automated trading system...");
    info!("Configuration:");
    info!("  - Leverage range: {:.1}x - {:.1}x", config.trading.min_leverage, config.trading.max_leverage);
    info!("  - Duration range: {}h - {}h", config.trading.min_duration_hours, config.trading.max_duration_hours);
    info!("  - Monitoring interval: {}s", config.monitoring.check_interval_seconds);
    info!("  - Cooldown period: {}s", config.trading.cooldown_seconds);
    info!("  - Min collateral ratio: {:.0}%", config.trading.min_collateral_ratio * 100.0);
    info!("  - Max PnL divergence: {:.1}%", config.trading.max_pnl_divergence * 100.0);

    Ok(())
}
