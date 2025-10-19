use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    pub symbol: String,
}

pub static BTC: LazyLock<Token> = LazyLock::new(|| Token { symbol: "BTC_USDC_PERP".to_owned() });
pub static ETH: LazyLock<Token> = LazyLock::new(|| Token { symbol: "ETH_USDC_PERP".to_owned() });
pub static SOL: LazyLock<Token> = LazyLock::new(|| Token { symbol: "SOL_USDC_PERP".to_owned() });

impl Token {
    pub fn new(symbol: String) -> Self {
        Self { symbol }
    }

    pub fn btc() -> Token {
        BTC.clone()
    }

    pub fn eth() -> Token {
        ETH.clone()
    }

    pub fn sol() -> Token {
        SOL.clone()
    }

    pub fn get_supported_tokens() -> Vec<Token> {
        vec![BTC.clone(), ETH.clone(), SOL.clone()]
    }
}
