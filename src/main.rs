#[macro_use]
extern crate log;

mod config;
mod error;
mod model;
mod perp;
mod request;
mod trader;
mod storage;
mod helpers;

use std::fs::File;
use std::io::BufReader;
use anyhow::{Result, Context};
use bpx_api_client::types::trade;
use inquire::{Select, Confirm};
use rand::Rng;
use crate::config::AppConfig;
use crate::trader::client::TraderClient;
use crate::trader::strategy::TradingStrategy;


/// Load all available wallet IDs from api-keys.json
fn load_all_wallet_ids() -> Result<Vec<u8>> {
    let file = File::open("api-keys.json")
        .context("Failed to open api-keys.json")?;
    let reader = BufReader::new(file);
    
    let wallets_map: serde_json::Value = serde_json::from_reader(reader)
        .context("Failed to parse api-keys.json")?;
    
    let mut wallet_ids = Vec::new();
    if let Some(obj) = wallets_map.as_object() {
        for (key, _) in obj.iter() {
            if let Ok(id) = key.parse::<u8>() {
                wallet_ids.push(id);
            }
        }
    }
    
    wallet_ids.sort();
    Ok(wallet_ids)
}


/// Display strategy details in a formatted way
fn display_strategy_result(strategy: &TradingStrategy) {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                  STRATEGY EXECUTED                           ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!("\n✅ Strategy ID: {}", strategy.id);
    println!("🪙 Token: {}", strategy.token_symbol);
    println!("👛 Wallets: {:?}", strategy.wallet_ids);
    println!("📈 Status: {}", strategy.status);
    println!("\n📅 Schedule:");
    println!("   • Opened at:  {}", strategy.opened_at.format("%Y-%m-%d %H:%M:%S UTC"));
    println!("   • Close at:   {}", strategy.close_at.format("%Y-%m-%d %H:%M:%S UTC"));
    
    println!("\n📊 LONG Positions ({}):", strategy.longs.len());
    println!("   Total Size: {} tokens", strategy.longs_size);
    for (i, pos) in strategy.longs.iter().enumerate() {
        println!("   {}. Wallet #{} - Size: {} tokens - Status: {}", 
            i + 1, pos.wallet_id, pos.size, pos.status);
    }
    
    println!("\n📉 SHORT Positions ({}):", strategy.shorts.len());
    println!("   Total Size: {} tokens", strategy.shorts_size);
    for (i, pos) in strategy.shorts.iter().enumerate() {
        println!("   {}. Wallet #{} - Size: {} tokens - Status: {}", 
            i + 1, pos.wallet_id, pos.size, pos.status);
    }
    
    println!("\n═══════════════════════════════════════════════════════════════\n");
}


#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    info!("🚀 Starting perp-trader application...");

    // let token = model::token::Token::eth();
    // let wallet = trader::wallet::Wallet::load_from_json(1)?;
    // let client = crate::perp::lighter::client::LighterClient::new(&wallet).await?;
    // let close_at = chrono::Utc::now() + chrono::Duration::hours(1);
    // perp::PerpExchange::close_all_positions(&client).await?;
    // loop {};

    // Load configuration
    let config = AppConfig::load()?;
    
    // Load all available wallets
    let wallet_ids = load_all_wallet_ids()?;
    
    if wallet_ids.is_empty() {
        error!("❌ No wallets found in api-keys.json");
        return Ok(());
    }
    
    if wallet_ids.len() < 3 {
        error!("❌ At least 3 wallets are required. Found: {}", wallet_ids.len());
        return Ok(());
    }
    
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║              PERPETUAL FUTURES POINT FARMING                ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!("\n✅ Connected Wallets: {}", wallet_ids.len());
    println!("   Wallet IDs: {:?}\n", wallet_ids);

    // Interactive menu for exchange selection
    let options = vec![
        "🎒 Farm points on Backpack",
        "💡 Farm points on Lighter",
    ];

    let selection = Select::new("Select farming strategy:", options.clone())
        .prompt()
        .context("Failed to get user selection")?;

    let (_, is_backpack) = match selection {
        s if s == options[0] => ("Backpack", true),
        s if s == options[1] => ("Lighter", false),
        _ => unreachable!(),
    };

    // Confirm before proceeding
    let should_continue = Confirm::new("Do you want to continue?")
        .with_default(false)
        .prompt()
        .context("Failed to get confirmation")?;

    if !should_continue {
        warn!("\n❌ Operation cancelled by user.");
        return Ok(());
    }

    // Create database connection pool
    info!("🔌 Connecting to database...");
    let pool = storage::init_pool(&config).await?;
    info!("✅ Database connected successfully");

    // Create trader client
    info!("🔧 Initializing trader client with {} wallets...", wallet_ids.len());
    let trader_client = TraderClient::new(wallet_ids.clone(), pool)
        .await
        .context("Failed to create trader client")?;
    
    info!("✅ Trader client initialized");
    
    // Generate random duration between 4-8 hours
    let mut rng = rand::thread_rng();
    let duration_hours = rng.gen_range(1..=1);

    trader_client.close_all_active_strategies().await?;

    // // Execute selected strategy
    // let strategy = if is_backpack {
    //     trader_client.farm_points_on_backpack_from_multiple_wallets(duration_hours).await?
    // } else {
    //     trader_client.farm_points_on_lighter_from_multiple_wallets(duration_hours).await?
    // };

    // let active_strategies = trader_client.get_active_strategies().await?;

    // if !active_strategies.is_empty() {
    //     trader_client.monitor_and_close_strategies(active_strategies).await?;
    // }

    Ok(())
}
