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

use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::time::Duration;
use anyhow::{Result, Context};
use inquire::{Select, Confirm};
use rand::Rng;
use rust_decimal::Decimal;
use tokio::time;
use serde_json::Value;

use crate::{perp::lighter::{client::LighterClient}, trader::client::TraderClient, storage::database::Database, helpers::encode::encrypt_private_key};
use colored::*;

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

/// Clear all lighter_api_key fields in api-keys.json
fn clear_all_lighter_api_keys() -> Result<()> {
    let file_path = "api-keys.json";
    // Open and parse the JSON data
    let file = File::open(file_path)
        .context("Failed to open api-keys.json")?;
    let mut wallets_map: serde_json::Value = serde_json::from_reader(BufReader::new(file))
        .context("Failed to parse api-keys.json")?;
    let obj = wallets_map.as_object_mut()
        .context("api-keys.json is not a JSON object")?;
    let mut changed = false;
    for (_wallet_id, entry) in obj.iter_mut() {
        if let Some(map) = entry.as_object_mut() {
            if map.contains_key("lighter_api_key") {
                if let Some(key_val) = map.get_mut("lighter_api_key") {
                    if !key_val.is_string() || key_val.as_str().unwrap_or("").len() > 0 {
                        *key_val = serde_json::Value::String("".to_owned());
                        changed = true;
                    }
                }
            }
        }
    }
    if changed {
        // Write changes back to file
        let mut file = OpenOptions::new().write(true).truncate(true).open(file_path)
            .context("Failed to open api-keys.json for writing")?;
        let pretty = serde_json::to_string_pretty(&wallets_map)
            .context("Failed to serialize updated api-keys.json")?;
        file.write_all(pretty.as_bytes()).context("Failed to write cleaned api-keys.json")?;
        file.flush()?;
        info!("✅ All lighter_api_key fields have been cleared from api-keys.json");
    } else {
        info!("No lighter_api_key fields needed to be cleared.");
    }
    Ok(())
}

/// Checks private keys in api-keys.json and fills empty ones from MongoDB
/// 
/// For each entry in api-keys.json:
/// - If private_key is empty, connects to MongoDB
/// - Gets the private key from account.wallet_key
/// - Encodes it using encrypt_private_key from helpers
/// - Writes the encoded value back to api-keys.json
async fn fill_empty_private_keys() -> Result<()> {
    dotenv::dotenv().ok();
    
    let password = std::env::var("WALLETS_PASSWORD")
        .context("Failed to get WALLETS_PASSWORD from environment variables")?;
    
    let file_path = "api-keys.json";
    
    // Read api-keys.json
    let file = File::open(file_path)
        .context("Failed to open api-keys.json")?;
    let reader = BufReader::new(file);
    let mut wallets_map: Value = serde_json::from_reader(reader)
        .context("Failed to parse api-keys.json")?;
    
    let obj = wallets_map.as_object_mut()
        .context("api-keys.json is not a JSON object")?;
    
    // Connect to MongoDB
    let database = Database::get_instance().await
        .context("Failed to connect to MongoDB")?;
    
    let mut changed = false;
    
    // Iterate through each wallet entry
    for (wallet_id_str, entry) in obj.iter_mut() {
        if let Some(map) = entry.as_object_mut() {
            // Check if private_key exists and is empty
            let private_key_empty = map.get("private_key")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().is_empty())
                .unwrap_or(true); // If field doesn't exist, treat as empty
            
            if private_key_empty {
                // Parse wallet_id
                let wallet_id: u32 = wallet_id_str.parse()
                    .with_context(|| format!("Failed to parse wallet_id: {}", wallet_id_str))?;
                
                // Get account from MongoDB
                let account = database.get_account_by_id(wallet_id).await
                    .map_err(|e| anyhow::anyhow!("MongoDB error: {}", e))
                    .with_context(|| format!("Failed to get account for wallet_id: {}", wallet_id))?;
                
                match account {
                    Some(acc) => {
                        if acc.wallet_key.is_empty() {
                            warn!("Wallet {} has empty wallet_key in database, skipping", wallet_id);
                            continue;
                        }
                        
                        // Encode the private key
                        let encoded_key = encrypt_private_key(&acc.wallet_key, &password)
                            .with_context(|| format!("Failed to encode private key for wallet_id: {}", wallet_id))?;
                        
                        // Update the JSON
                        map.insert("private_key".to_string(), Value::String(encoded_key));
                        changed = true;
                        
                        info!("✅ Filled private_key for wallet_id: {}", wallet_id);
                    }
                    None => {
                        warn!("Account not found in database for wallet_id: {}, skipping", wallet_id);
                    }
                }
            }
        }
    }
    
    // Write changes back to file if any were made
    if changed {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(file_path)
            .context("Failed to open api-keys.json for writing")?;
        
        let pretty = serde_json::to_string_pretty(&wallets_map)
            .context("Failed to serialize updated api-keys.json")?;
        
        file.write_all(pretty.as_bytes())
            .context("Failed to write updated api-keys.json")?;
        file.flush()?;
        
        info!("✅ Successfully updated api-keys.json with encoded private keys");
    } else {
        info!("No empty private keys found, api-keys.json unchanged");
    }
    
    Ok(())
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
        FarmLighter,
        CloseAllStrategies,
        CloseAllPositions,
        ShowAllWalletsBalances,
        ShowAllWalletsPoints,
        ClearAllLighterApiKeys,
        FillEmptyPrivateKeys,
    }

    // Determine action based on environment
    let action = if is_running_on_flyio() {
        info!("🪰 Detected Fly.io environment - auto-starting Lighter farming");
        Action::FarmLighter
    } else {
        // Interactive menu for local development
        println!("\n{}", "FARMING STRATEGIES:".bold());
        let options = vec![
            "💡 Farm points on Lighter",
            "✋ Close all active strategies",
            "💸 Close all active positions",
            "💰 Show all wallets balances",
            "🏆 Show all wallets points",
            "🧹 Clear all lighter_api_keys from JSON",
            "🔑 Fill empty private keys from MongoDB",
        ];

        let selection = Select::new("Select operation:", options.clone())
            .prompt()
            .context("Failed to get user selection")?;

        let selected_action = match &*selection {
            s if s == options[0] => Action::FarmLighter,
            s if s == options[1] => Action::CloseAllStrategies,
            s if s == options[2] => Action::CloseAllPositions,
            s if s == options[3] => Action::ShowAllWalletsBalances,
            s if s == options[4] => Action::ShowAllWalletsPoints,
            s if s == options[5] => Action::ClearAllLighterApiKeys,
            s if s == options[6] => Action::FillEmptyPrivateKeys,
            _ => {
                warn!("Invalid selection");
                return Ok(());
            }
        };

        // Confirm before proceeding
        let confirmation_message = match selected_action {
            Action::FarmLighter => "Start farming on Lighter?",
            Action::CloseAllStrategies => "Close all active strategies?",
            Action::CloseAllPositions => "Close all open positions?",
            Action::ShowAllWalletsBalances => "Show all wallets balances?",
            Action::ShowAllWalletsPoints => "Show all wallets points?",
            Action::ClearAllLighterApiKeys => "Clear all lighter_api_key fields in api-keys.json?",
            Action::FillEmptyPrivateKeys => "Fill empty private keys from MongoDB?",
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

    if let Action::ClearAllLighterApiKeys = action {
        clear_all_lighter_api_keys()?;
        return Ok(());
    }

    if let Action::FillEmptyPrivateKeys = action {
        fill_empty_private_keys().await?;
        return Ok(());
    }

    // Initialize trader client
    info!("Initializing trader client with {} wallets...", wallet_ids.len());
    let trader_client = TraderClient::new(wallet_ids.clone(), pool.clone())
        .await
        .context("Failed to create trader client")?;

    match action {
        Action::CloseAllStrategies => {
            info!("Closing all active strategies...");
            trader_client.close_all_active_strategies().await?;
            info!("✅ All strategies closed");
        }
        Action::CloseAllPositions => {
            info!("Closing all open positions...");
            trader_client.close_all_positions_on_lighter_for_all_wallets().await?;
            info!("✅ All positions closed");
        }
        Action::FarmLighter => {            
            let mut rng = rand::thread_rng();
            let mut i = 0;

            
            loop {
                // Check for and retry any failed strategies from previous runs
                info!("🔍 Checking for failed strategies from previous runs...");
                trader_client.retry_failed_strategies().await?;
                
                let loop_sleep_minutes = rng.gen_range(30..=80);
                info!("#{} | Strategy starting...", i);

                trader_client.farm_points_on_lighter_from_multiple_wallets().await?;

                let active_strategies = trader_client.get_active_strategies().await?;

                if !active_strategies.is_empty() {
                    trader_client.monitor_and_close_strategies(active_strategies).await?;
                }

                i += 1;
                info!("#{} | Sleeping for {} minutes", i, loop_sleep_minutes);
                time::sleep(Duration::from_secs(loop_sleep_minutes * 60)).await;
            }
        }
        Action::ShowAllWalletsBalances => {
            let wallet_balances = trader_client.fetch_wallet_balances_on_lighter().await?;
            for i in 0..wallet_balances.len() {
                info!("#{}: {:.2}$", i, wallet_balances[i].1);
            }

            let total_balance = wallet_balances.iter().map(|(_, balance)| balance).sum::<Decimal>();
            info!("Total balance: {:.2}$", total_balance);
        }
        Action::ShowAllWalletsPoints => {
            use futures::future;

            // Run all async blocks in parallel and collect results (both successes and failures)
            let results = future::join_all(
                trader_client.wallets.iter().map(|wallet| {
                    let wallet_id = wallet.id;
                    async move {
                        (wallet_id, async {
                            let lighter_client = LighterClient::new(&wallet).await?;
                            lighter_client.get_account_points().await
                        }.await)
                    }
                })
            ).await;

            let mut successful_points = Vec::new();
            let mut failed_count = 0;

            // Process each result separately
            for (wallet_id, result) in results {
                match result {
                    Ok(points) => {
                        successful_points.push((wallet_id, points));
                    }
                    Err(e) => {
                        error!("Wallet #{}: Failed to fetch points - {}", wallet_id, e);
                        failed_count += 1;
                    }
                }
            }

            // Display successful results
            for (wallet_id, points) in &successful_points {
                info!("#{:>3}: total: {:.2} points | last week: {:.2}", 
                    wallet_id, points.user_total_points, points.user_last_week_points);
            }

            // Show summary
            if !successful_points.is_empty() {
                let last_week_points = successful_points.iter().map(|(_, points)| points.user_last_week_points).sum::<f64>();
                let total_points = successful_points.iter().map(|(_, points)| points.user_total_points).sum::<f64>();
                info!("Total points: {:.2} | Last week: {:.2} (from {} successful wallets)", 
                    total_points, last_week_points, successful_points.len());
            }

            if failed_count > 0 {
                warn!("Failed to fetch points from {} wallet(s)", failed_count);
            }
        }
        Action::ClearAllLighterApiKeys => {
            unreachable!();
        }
        Action::FillEmptyPrivateKeys => {
            unreachable!();
        }
    }

    Ok(())
}