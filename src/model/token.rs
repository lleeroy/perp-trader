use crate::model::Exchange;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    pub symbol: SupportedToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SupportedToken {
    ETH,
    SOL,
    HYPE,
    BNB,
    XRP,
    AAVE,
    ENA,
    PUMP
}

impl std::fmt::Display for SupportedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupportedToken::ETH => write!(f, "ETH"),
            SupportedToken::SOL => write!(f, "SOL"),
            SupportedToken::HYPE => write!(f, "HYPE"),
            SupportedToken::BNB => write!(f, "BNB"),
            SupportedToken::XRP => write!(f, "XRP"),
            SupportedToken::AAVE => write!(f, "AAVE"),
            SupportedToken::ENA => write!(f, "ENA"),
            SupportedToken::PUMP => write!(f, "PUMP"),
        }
    }
}

impl Token {
    pub fn from_market_index(exchange: Exchange, market_index: i32) -> Self {
        match exchange {
            Exchange::Lighter | Exchange::Backpack => match market_index {
                0 => Self::eth(),
                2 => Self::sol(),
                7 => Self::xrp(),
                24 => Self::hype(),
                25 => Self::bnb(),
                27 => Self::aave(),
                29 => Self::ena(),
                45 => Self::pump(),
                _ => panic!("Invalid market index: {}", market_index),
            },
        }
    }

    pub fn new(symbol: SupportedToken) -> Self {
        Self { symbol }
    }

    pub fn eth() -> Token {
        Token::new(SupportedToken::ETH)
    }

    pub fn sol() -> Token {
        Token::new(SupportedToken::SOL)
    }

    pub fn hype() -> Token {
        Token::new(SupportedToken::HYPE)
    }

    pub fn bnb() -> Token {
        Token::new(SupportedToken::BNB)
    }

    pub fn xrp() -> Token {
        Token::new(SupportedToken::XRP)
    }

    pub fn aave() -> Token {
        Token::new(SupportedToken::AAVE)
    }

    pub fn ena() -> Token {
        Token::new(SupportedToken::ENA)
    }

    pub fn pump() -> Token {
        Token::new(SupportedToken::PUMP)
    }

    pub fn get_supported_tokens() -> Vec<Token> {
        vec![Self::eth(), Self::sol(), Self::hype(), Self::bnb(), Self::xrp(), Self::aave(), Self::ena(), Self::pump()]
    }

    pub fn get_symbol_string(&self, exchange: Exchange) -> String {
        match self.symbol {
            SupportedToken::ETH => match exchange {
                Exchange::Lighter => "ETH_USDC_PERP".to_string(),
                Exchange::Backpack => "ETH_USDC_PERP".to_string(),
            },
            SupportedToken::SOL => match exchange {
                Exchange::Lighter => "SOL_USDC_PERP".to_string(),
                Exchange::Backpack => "SOL_USDC_PERP".to_string(),
            },
            SupportedToken::HYPE => match exchange {
                Exchange::Lighter => "HYPE_USDC_PERP".to_string(),
                Exchange::Backpack => "HYPE_USDC_PERP".to_string(),
            },
            SupportedToken::BNB => match exchange {
                Exchange::Lighter => "BNB_USDC_PERP".to_string(),
                Exchange::Backpack => "BNB_USDC_PERP".to_string(),
            },
            SupportedToken::XRP => match exchange {
                Exchange::Lighter => "XRP_USDC_PERP".to_string(),
                Exchange::Backpack => "XRP_USDC_PERP".to_string(),
            },
            SupportedToken::AAVE => match exchange {
                Exchange::Lighter => "AAVE_USDC_PERP".to_string(),
                Exchange::Backpack => "AAVE_USDC_PERP".to_string(),
            },
            SupportedToken::ENA => match exchange {
                Exchange::Lighter => "ENA_USDC_PERP".to_string(),
                Exchange::Backpack => "ENA_USDC_PERP".to_string(),
            },
            SupportedToken::PUMP => match exchange {
                Exchange::Lighter => "PUMP_USDC_PERP".to_string(),
                Exchange::Backpack => "PUMP_USDC_PERP".to_string(),
            },
        }
    }

    pub fn get_market_index(&self, exchange: Exchange) -> i32 {
        match self.symbol {
            SupportedToken::ETH => match exchange {
                Exchange::Lighter => 0,
                Exchange::Backpack => 0,
            },
            SupportedToken::SOL => match exchange {
                Exchange::Lighter => 2,
                Exchange::Backpack => 2,
            },
            SupportedToken::XRP => match exchange {
                Exchange::Lighter => 7,
                Exchange::Backpack => 7,
            },
            SupportedToken::HYPE => match exchange {
                Exchange::Lighter => 24,
                Exchange::Backpack => 24,
            },
            SupportedToken::BNB => match exchange {
                Exchange::Lighter => 25,
                Exchange::Backpack => 25,
            },
            SupportedToken::AAVE => match exchange {
                Exchange::Lighter => 27,
                Exchange::Backpack => 27,
            },
            SupportedToken::ENA => match exchange {
                Exchange::Lighter => 29,
                Exchange::Backpack => 29,
            },
            SupportedToken::PUMP => match exchange {
                Exchange::Lighter => 45,
                Exchange::Backpack => 45,
            },
        }
    }
}
