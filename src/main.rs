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
mod alert;
mod test;

use std::fs::File;
use std::io::BufReader;
use std::time::Duration;
use anyhow::{Result, Context};
use inquire::{Select, Confirm};
use rand::Rng;
use tokio::time;
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

/// Detect if running on Fly.io
fn is_running_on_flyio() -> bool {
    std::env::var("FLY_APP_NAME").is_ok() || 
    std::env::var("FLY_ALLOC_ID").is_ok()
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
    println!("║              PERPETUAL FUTURES POINT FARMING                 ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!("\n✅ Connected Wallets: {}", wallet_ids.len());
    println!("   Wallet IDs: {:?}\n", wallet_ids);

    // Create database connection pool
    info!("🔌 Connecting to database...");
    let pool = storage::init_pool().await?;
    info!("✅ Database connected successfully");

    enum Action {
        FarmBackpack,
        FarmLighter,
        CloseAll,
    }

    // Determine action based on environment
    let action = if is_running_on_flyio() {
        info!("🪰 Detected Fly.io environment - auto-starting Lighter farming");
        Action::FarmLighter
    } else {
        // Interactive menu for local development
        println!("\n{}", "FARMING STRATEGIES:".bold());
        let options = vec![
            "🎒 Farm points on Backpack",
            "💡 Farm points on Lighter",
            "🛑 Close all active strategies",
        ];

        let selection = Select::new("Select operation:", options.clone())
            .prompt()
            .context("Failed to get user selection")?;

        let selected_action = match selection {
            s if s == options[0] => Action::FarmBackpack,
            s if s == options[1] => Action::FarmLighter,
            s if s == options[2] => Action::CloseAll,
            _ => {
                warn!("Invalid selection");
                return Ok(());
            }
        };

        // Confirm before proceeding
        let confirmation_message = match selected_action {
            Action::FarmBackpack => "Start farming on Backpack?",
            Action::FarmLighter => "Start farming on Lighter?",
            Action::CloseAll => "Close all active strategies?",
        };

        let should_continue = Confirm::new(confirmation_message)
            .with_default(false)
            .prompt()
            .context("Failed to get confirmation")?;

        if !should_continue {
            warn!("\n❌ Operation cancelled by user.");
            return Ok(());
        }

        selected_action
    };

    // Initialize trader client
    info!("Initializing trader client with {} wallets...", wallet_ids.len());
    let trader_client = TraderClient::new(wallet_ids.clone(), pool.clone())
        .await
        .context("Failed to create trader client")?;

    // Execute selected action
    match action {
        Action::CloseAll => {
            info!("Closing all active strategies...");
            trader_client.close_all_active_strategies().await?;
            info!("✅ All strategies closed");
        }
        Action::FarmBackpack | Action::FarmLighter => {
            let is_backpack = matches!(action, Action::FarmBackpack);
            let mut rng = rand::thread_rng();
            let mut i = 0;
            
            loop {
                let loop_sleep_minutes = rng.gen_range(5..=20);
                let duration_minutes = rng.gen_range(60..=240);
                info!("#{} | Duration set to: {} minutes", i, duration_minutes);
                info!("#{} | Strategy starting...", i);

                if is_backpack {
                    trader_client.farm_points_on_backpack_from_multiple_wallets(duration_minutes).await?;
                } else {
                    trader_client.farm_points_on_lighter_from_multiple_wallets(duration_minutes).await?;
                }

                let active_strategies = trader_client.get_active_strategies().await?;

                if !active_strategies.is_empty() {
                    trader_client.monitor_and_close_strategies(active_strategies).await?;
                }

                i += 1;
                info!("#{} | Sleeping for {} minutes", i, loop_sleep_minutes);
                time::sleep(Duration::from_secs(loop_sleep_minutes * 60)).await;
            }
        }
    }

    Ok(())
}