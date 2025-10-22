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


use std::time::Duration;
use anyhow::{Result};
use chrono::Utc;
use rust_decimal::Decimal;
use crate::model::token::Token;
use crate::model::{PositionSide};
use crate::perp::PerpExchange;


#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    info!("🚀 Starting perp-trader application...");

    let token = Token::eth();
    let wallet = trader::wallet::Wallet::load_from_json(1).unwrap();
    let lighter_client = perp::lighter::client::LighterClient::new(&wallet).await.unwrap();

    // let close_at = Utc::now() + chrono::Duration::days(1);
    // let position = lighter_client.open_position(token, PositionSide::Short, close_at, Decimal::ZERO).await.unwrap();
    // println!("Position: {:#?}", position);

    // tokio::time::sleep(Duration::from_secs(10)).await;
    lighter_client.close_all_positions().await.unwrap();
    info!("All positions closed");

    Ok(())
}
