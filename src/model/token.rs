#![allow(unused)]
use std::sync::LazyLock;
use crate::model::Exchange;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    pub symbol: SupportedToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SupportedToken {
    BTC,
    ETH,
    SOL,
}

impl std::fmt::Display for SupportedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupportedToken::BTC => write!(f, "BTC"),
            SupportedToken::ETH => write!(f, "ETH"),
            SupportedToken::SOL => write!(f, "SOL"),
        }
    }
}

impl Token {
    pub fn from_market_index(exchange: Exchange, market_index: i32) -> Self {
        match exchange {
            Exchange::Lighter | Exchange::Backpack => match market_index {
                1 => Self::btc(),
                0 => Self::eth(),
                2 => Self::sol(),
                _ => panic!("Invalid market index: {}", market_index),
            },
        }
    }

    pub fn new(symbol: SupportedToken) -> Self {
        Self { symbol }
    }

    pub fn btc() -> Token {
        Token::new(SupportedToken::BTC)
    }

    pub fn eth() -> Token {
        Token::new(SupportedToken::ETH)
    }

    pub fn sol() -> Token {
        Token::new(SupportedToken::SOL)
    }

    pub fn get_supported_tokens() -> Vec<Token> {
        vec![Self::btc(), Self::eth(), Self::sol()]
    }

    pub fn get_symbol_string(&self, exchange: Exchange) -> String {
        match self.symbol {
            SupportedToken::BTC => match exchange {
                Exchange::Lighter => "BTC_USDC_PERP".to_string(),
                Exchange::Backpack => "BTC_USDC_PERP".to_string(),
            },
            SupportedToken::ETH => match exchange {
                Exchange::Lighter => "ETH_USDC_PERP".to_string(),
                Exchange::Backpack => "ETH_USDC_PERP".to_string(),
            },
            SupportedToken::SOL => match exchange {
                Exchange::Lighter => "SOL_USDC_PERP".to_string(),
                Exchange::Backpack => "SOL_USDC_PERP".to_string(),
            },
        }
    }

    pub fn get_market_index(&self, exchange: Exchange) -> i32 {
        match self.symbol {
            SupportedToken::BTC => match exchange {
                Exchange::Lighter => 1,
                Exchange::Backpack => 1,
            },
            SupportedToken::ETH => match exchange {
                Exchange::Lighter => 0,
                Exchange::Backpack => 0,
            },
            SupportedToken::SOL => match exchange {
                Exchange::Lighter => 2,
                Exchange::Backpack => 2,
            },
        }
    }
}
