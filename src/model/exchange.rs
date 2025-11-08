use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Exchange {
    Lighter,
    Ranger
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exchange::Lighter => write!(f, "Lighter"),
            Exchange::Ranger => write!(f, "Ranger"),
        }
    }
}

impl FromStr for Exchange {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "lighter" => Ok(Exchange::Lighter),
            "ranger" => Ok(Exchange::Ranger),
            _ => Err(anyhow::anyhow!("Unknown exchange: {}", s)),
        }
    }
}

impl TryFrom<String> for Exchange {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Exchange::from_str(&value)
    }
}