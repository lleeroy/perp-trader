#[macro_use]
extern crate log;

mod config;
mod error;
mod model;
mod perp;
mod request;
mod trader;

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

    // let position = backpack_client.open_position(
    //     Token::sol(), 
    //     PositionSide::Long, 
    //     usdc_balance
    // ).await?;

    // trader_client.save_position(&position)?;

    // Demonstrate retrieving from storage
    let stored_positions = trader_client.get_all_positions()?;
    info!("📂 Retrieved {} positions from local storage", stored_positions.len());

    // Get active positions only
    let active_positions = trader_client.get_active_positions()?;
    info!("✅ Found {} active positions", active_positions.len());

    // Print details
    for pos in &stored_positions {
        info!(
            "Position: {} | {} | {} | {} | Status: {}",
            pos.id, pos.exchange, pos.symbol, pos.side, pos.status
        );
    }

    Ok(())
}
