#[macro_use]
extern crate log;

mod config;
mod error;
mod model;
mod perp;
mod request;
mod trader;

use anyhow::{bail, Context, Result};
use std::sync::Arc;

use config::AppConfig;
use perp::backpack::BackpackClient;

use crate::{perp::PerpExchange, trader::wallet::Wallet};


#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    pretty_env_logger::init();

    info!("🔧 Loading configuration...");
    let config = AppConfig::load().context("Failed to load configuration")?;
    let config = Arc::new(config);

    info!("🔌 Initializing exchange clients...");
    let wallet = Wallet::load_from_json(1).context("Failed to load wallet")?;
    let backpack_client = BackpackClient::new(&wallet);

    let balances = backpack_client.get_balances().await.context("Failed to get balances")?;
    info!("Balances: {:?}", balances);
    Ok(())
}
