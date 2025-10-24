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
use inquire::{Select, Confirm};
use rand::Rng;
use crate::config::AppConfig;
use crate::perp::PerpExchange;
use crate::trader::client::TraderClient;
use colored::*;
use std::io::Write;


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



#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger with custom formatter
    env_logger::Builder::from_default_env()
        .format(|buf, record| {
            let level = record.level();
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            
            let level_string = match level {
                log::Level::Error => format!(" {} ", level).on_red().black().to_string(),
                log::Level::Warn => format!(" {}  ", level).on_yellow().black().to_string(),
                log::Level::Info => format!(" {}  ", level).to_string(),
                log::Level::Debug => format!(" {} ", level).to_string(),
                log::Level::Trace => format!(" {} ", level).to_string(),
            };
            
            writeln!(
                buf,
                "{} {}: {}",
                timestamp.to_string().dimmed(),
                level_string,
                record.args()
            )
        })
    .init();

    info!("🚀 Starting perp-trader application...");

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
    
    // Generate random duration between 1-60 minutes
    let mut rng = rand::thread_rng();
    let duration_minutes = rng.gen_range(1..=3);


    // Execute selected strategy
    if is_backpack {
        trader_client.farm_points_on_backpack_from_multiple_wallets(duration_minutes).await?
    } else {
        trader_client.farm_points_on_lighter_from_multiple_wallets(duration_minutes).await?
    };

    let active_strategies = trader_client.get_active_strategies().await?;

    if !active_strategies.is_empty() {
        trader_client.monitor_and_close_strategies(active_strategies).await?;
    }

    Ok(())
}
