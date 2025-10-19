use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub trading: TradingConfig,
    pub monitoring: MonitoringConfig,
    pub exchanges: ExchangesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// SQLite database path (e.g., "sqlite://data.db")
    pub url: String,
    /// Maximum number of connections in the pool
    #[serde(default = "default_pool_size")]
    pub max_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    /// Minimum leverage multiplier (e.g., 2.0 for 2x)
    #[serde(default = "default_min_leverage")]
    pub min_leverage: f64,
    /// Maximum leverage multiplier (e.g., 3.0 for 3x)
    #[serde(default = "default_max_leverage")]
    pub max_leverage: f64,
    /// Minimum position duration in hours
    #[serde(default = "default_min_duration_hours")]
    pub min_duration_hours: u64,
    /// Maximum position duration in hours
    #[serde(default = "default_max_duration_hours")]
    pub max_duration_hours: u64,
    /// Minimum collateral ratio before alerting (e.g., 1.5 for 150%)
    #[serde(default = "default_min_collateral_ratio")]
    pub min_collateral_ratio: f64,
    /// Maximum PnL divergence threshold (percentage, e.g., 0.05 for 5%)
    #[serde(default = "default_max_pnl_divergence")]
    pub max_pnl_divergence: f64,
    /// Cooldown period between position openings (in seconds)
    #[serde(default = "default_cooldown_seconds")]
    pub cooldown_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// How often to check positions (in seconds)
    #[serde(default = "default_check_interval_seconds")]
    pub check_interval_seconds: u64,
    /// Timeout for exchange API calls (in seconds)
    #[serde(default = "default_api_timeout_seconds")]
    pub api_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangesConfig {
    pub backpack: ExchangeCredentials,
    pub hibachi: ExchangeCredentials,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeCredentials {
    pub api_key: String,
    pub api_secret: String,
    /// Base URL for the exchange API
    #[serde(default)]
    pub base_url: Option<String>,
    /// Whether this exchange is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

// Default values
fn default_pool_size() -> u32 {
    5
}

fn default_min_leverage() -> f64 {
    2.0
}

fn default_max_leverage() -> f64 {
    3.0
}

fn default_min_duration_hours() -> u64 {
    4
}

fn default_max_duration_hours() -> u64 {
    8
}

fn default_min_collateral_ratio() -> f64 {
    1.5
}

fn default_max_pnl_divergence() -> f64 {
    0.05
}

fn default_cooldown_seconds() -> u64 {
    300 // 5 minutes
}

fn default_check_interval_seconds() -> u64 {
    60
}

fn default_api_timeout_seconds() -> u64 {
    10
}

fn default_true() -> bool {
    true
}

impl AppConfig {
    /// Load configuration from environment variables and config files
    pub fn load() -> Result<Self> {
        // Load .env file if it exists
        dotenv::dotenv().ok();

        let config = config::Config::builder()
            // Set defaults
            .set_default("database.url", "sqlite://perp_trader.db")?
            .set_default("database.max_connections", default_pool_size())?
            .set_default("trading.min_leverage", default_min_leverage())?
            .set_default("trading.max_leverage", default_max_leverage())?
            .set_default("trading.min_duration_hours", default_min_duration_hours())?
            .set_default("trading.max_duration_hours", default_max_duration_hours())?
            .set_default("trading.min_collateral_ratio", default_min_collateral_ratio())?
            .set_default("trading.max_pnl_divergence", default_max_pnl_divergence())?
            .set_default("trading.cooldown_seconds", default_cooldown_seconds())?
            .set_default("monitoring.check_interval_seconds", default_check_interval_seconds())?
            .set_default("monitoring.api_timeout_seconds", default_api_timeout_seconds())?
            .set_default("exchanges.backpack.enabled", true)?
            .set_default("exchanges.hibachi.enabled", true)?
            // Try to load from config file (optional)
            .add_source(config::File::with_name("config").required(false))
            // Override with environment variables (prefix: APP_)
            .add_source(
                config::Environment::with_prefix("APP")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()
            .context("Failed to build configuration")?;

        let app_config: AppConfig = config
            .try_deserialize()
            .context("Failed to deserialize configuration")?;

        app_config.validate()?;

        Ok(app_config)
    }

    /// Validate configuration values
    fn validate(&self) -> Result<()> {
        if self.trading.min_leverage <= 1.0 || self.trading.max_leverage <= 1.0 {
            anyhow::bail!("Leverage must be greater than 1.0");
        }

        if self.trading.min_leverage > self.trading.max_leverage {
            anyhow::bail!("min_leverage cannot be greater than max_leverage");
        }

        if self.trading.min_duration_hours == 0 || self.trading.max_duration_hours == 0 {
            anyhow::bail!("Position duration must be greater than 0");
        }

        if self.trading.min_duration_hours > self.trading.max_duration_hours {
            anyhow::bail!("min_duration_hours cannot be greater than max_duration_hours");
        }

        if self.trading.min_collateral_ratio < 1.0 {
            anyhow::bail!("min_collateral_ratio must be at least 1.0 (100%)");
        }

        if self.trading.max_pnl_divergence < 0.0 || self.trading.max_pnl_divergence > 1.0 {
            anyhow::bail!("max_pnl_divergence must be between 0.0 and 1.0");
        }

        if self.exchanges.backpack.api_key.is_empty() || self.exchanges.backpack.api_secret.is_empty() {
            anyhow::bail!("Backpack API credentials are required");
        }

        if self.exchanges.hibachi.api_key.is_empty() || self.exchanges.hibachi.api_secret.is_empty() {
            anyhow::bail!("Hibachi API credentials are required");
        }

        Ok(())
    }

    /// Get monitoring interval as Duration
    pub fn monitoring_interval(&self) -> Duration {
        Duration::from_secs(self.monitoring.check_interval_seconds)
    }

    /// Get API timeout as Duration
    pub fn api_timeout(&self) -> Duration {
        Duration::from_secs(self.monitoring.api_timeout_seconds)
    }

    /// Get cooldown period as Duration
    pub fn cooldown_period(&self) -> Duration {
        Duration::from_secs(self.trading.cooldown_seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation() {
        // Valid config should pass
        let config = AppConfig {
            database: DatabaseConfig {
                url: "sqlite://test.db".to_string(),
                max_connections: 5,
            },
            trading: TradingConfig {
                min_leverage: 2.0,
                max_leverage: 3.0,
                min_duration_hours: 4,
                max_duration_hours: 8,
                min_collateral_ratio: 1.5,
                max_pnl_divergence: 0.05,
                cooldown_seconds: 300,
            },
            monitoring: MonitoringConfig {
                check_interval_seconds: 60,
                api_timeout_seconds: 10,
            },
            exchanges: ExchangesConfig {
                backpack: ExchangeCredentials {
                    api_key: "test".to_string(),
                    api_secret: "test".to_string(),
                    base_url: None,
                    enabled: true,
                },
                hibachi: ExchangeCredentials {
                    api_key: "test".to_string(),
                    api_secret: "test".to_string(),
                    base_url: None,
                    enabled: true,
                },
            },
        };

        assert!(config.validate().is_ok());
    }
}

