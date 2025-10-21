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

use std::str::FromStr;

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use crate::config::AppConfig;
use crate::model::token::Token;
use crate::model::{Exchange, PositionSide};
use crate::perp::PerpExchange;
use crate::storage::init_pool;
use crate::trader::client::TraderClient;


#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    info!("🚀 Starting perp-trader application...");

    let token = Token::btc();
    let wallet = trader::wallet::Wallet::load_from_json(1).unwrap();
    let lighter_client = perp::lighter::client::LighterClient::new(&wallet).await.unwrap();


    lighter_client.open_position(token, PositionSide::Long, Decimal::ZERO).await.unwrap();
    Ok(())
}
