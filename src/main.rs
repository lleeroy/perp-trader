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


use anyhow::{Result};
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

    lighter_client.open_position(token, PositionSide::Long, Decimal::ZERO).await.unwrap();
    Ok(())
}
