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
use perp::backpack::BackpackClient;
use crate::{model::{token::Token, PositionSide}, perp::PerpExchange, trader::{client::TraderClient, wallet::Wallet}};


#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    info!("🔌 Initializing exchange clients...");
    let wallet = Wallet::load_from_json(1).context("Failed to load wallet")?;
    let trader_client = TraderClient::new_by_wallet_id(1).context("Failed to create trader client")?;
    let backpack_client = BackpackClient::new(&wallet);

    let usdc_balance = backpack_client.get_usdc_balance().await.context("Failed to get USDC balance")?;
    info!("USDC balance: {}", usdc_balance);

    let supported_tokens = trader_client.get_supported_tokens();
    info!("Supported tokens: {:?}", supported_tokens);
    
    Ok(())
}
