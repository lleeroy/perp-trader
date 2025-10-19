use crate::{error::TradingError, model::token::Token, trader::wallet::Wallet};

pub struct TraderClient {
    wallet: Wallet,
}

impl TraderClient {
    pub fn new_by_wallet_id(wallet_id: u8) -> Result<Self, TradingError> {
        let wallet = Wallet::load_from_json(wallet_id)?;

        Ok(Self { wallet })
    }

    pub fn get_supported_tokens(&self) -> Vec<Token> {
        Token::get_supported_tokens()
    }
}