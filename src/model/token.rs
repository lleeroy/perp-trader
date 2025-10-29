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
    ZK,
    DYDX,
    PENGU,
    TON,
    EDEN,
    GMX,
    GRASS
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
            SupportedToken::ZK => write!(f, "ZK"),
            SupportedToken::DYDX => write!(f, "DYDX"),
            SupportedToken::PENGU => write!(f, "PENGU"),
            SupportedToken::TON => write!(f, "TON"),
            SupportedToken::EDEN => write!(f, "EDEN"),
            SupportedToken::GMX => write!(f, "GMX"),
            SupportedToken::GRASS => write!(f, "GRASS"),
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
                12 => Self::ton(),
                24 => Self::hype(),
                25 => Self::bnb(),
                27 => Self::aave(),
                29 => Self::ena(),
                47 => Self::pengu(),
                52 => Self::grass(),
                56 => Self::zk(),
                61 => Self::gmx(),
                62 => Self::dydx(),
                89 => Self::eden(),
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

    pub fn zk() -> Token {
        Token::new(SupportedToken::ZK)
    }

    pub fn dydx() -> Token {
        Token::new(SupportedToken::DYDX)
    }

    pub fn pengu() -> Token {
        Token::new(SupportedToken::PENGU)
    }

    pub fn ton() -> Token {
        Token::new(SupportedToken::TON)
    }

    pub fn eden() -> Token {
        Token::new(SupportedToken::EDEN)
    }

    pub fn gmx() -> Token {
        Token::new(SupportedToken::GMX)
    }

    pub fn grass() -> Token {
        Token::new(SupportedToken::GRASS)
    }

    pub fn get_supported_tokens() -> Vec<Token> {
        vec![
            Self::zk(), 
            Self::dydx(), 
            Self::pengu(), 
            Self::ton(), 
            Self::eden(), 
            Self::gmx(), 
            Self::grass()
        ]
    }

    /// Returns the price denomination (how much to multiply the price by)
    pub fn get_price_denomination(&self) -> f64 {
        match self.symbol {
            SupportedToken::ETH => 100.0,   
            SupportedToken::SOL => 1_000.0,      
            SupportedToken::BNB => 10_000.0,      
            SupportedToken::HYPE => 10_000.0,
            SupportedToken::XRP => 1_000_000.0,
            SupportedToken::AAVE => 1_000.0,
            SupportedToken::ENA => 100_000.0,
            SupportedToken::ZK => 1_000_000.0,
            SupportedToken::DYDX => 100_000.0,
            SupportedToken::PENGU => 1_000_000.0,
            SupportedToken::TON => 100_000.0,
            SupportedToken::EDEN => 100_000.0,
            SupportedToken::GMX => 10_000.0,
            SupportedToken::GRASS => 100_000.0,
        }
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
            SupportedToken::ZK => match exchange {
                Exchange::Lighter => "ZK_USDC_PERP".to_string(),
                Exchange::Backpack => "ZK_USDC_PERP".to_string(),
            },
            SupportedToken::DYDX => match exchange {
                Exchange::Lighter => "DYDX_USDC_PERP".to_string(),
                Exchange::Backpack => "DYDX_USDC_PERP".to_string(),
            },
            SupportedToken::PENGU => match exchange {
                Exchange::Lighter => "PENGU_USDC_PERP".to_string(),
                Exchange::Backpack => "PENGU_USDC_PERP".to_string(),
            },
            SupportedToken::TON => match exchange {
                Exchange::Lighter => "TON_USDC_PERP".to_string(),
                Exchange::Backpack => "TON_USDC_PERP".to_string(),
            },
            SupportedToken::EDEN => match exchange {
                Exchange::Lighter => "EDEN_USDC_PERP".to_string(),
                Exchange::Backpack => "EDEN_USDC_PERP".to_string(),
            },
            SupportedToken::GMX => match exchange {
                Exchange::Lighter => "GMX_USDC_PERP".to_string(),
                Exchange::Backpack => "GMX_USDC_PERP".to_string(),
            },
            SupportedToken::GRASS => match exchange {
                Exchange::Lighter => "GRASS_USDC_PERP".to_string(),
                Exchange::Backpack => "GRASS_USDC_PERP".to_string(),
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
            SupportedToken::TON => match exchange {
                Exchange::Lighter => 12,
                Exchange::Backpack => 12,
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
            SupportedToken::ZK => match exchange {
                Exchange::Lighter => 56,
                Exchange::Backpack => 56,
            },
            SupportedToken::DYDX => match exchange {
                Exchange::Lighter => 62,
                Exchange::Backpack => 62,
            },
            SupportedToken::PENGU => match exchange {
                Exchange::Lighter => 47,
                Exchange::Backpack => 47,
            },
            SupportedToken::EDEN => match exchange {
                Exchange::Lighter => 89,
                Exchange::Backpack => 89,
            },
            SupportedToken::GMX => match exchange {
                Exchange::Lighter => 61,
                Exchange::Backpack => 61,
            },
            SupportedToken::GRASS => match exchange {
                Exchange::Lighter => 52,
                Exchange::Backpack => 52,
            },
        }
    }
}
