#![allow(unused)]
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RequestError {
    #[error("API error: {0}")]
    ApiError(String),

    #[error("Request timeout: {0}")]
    TimeoutError(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Method not supported: {0}")]
    MethodNotSupported(String),

    #[error("Can't process request: {0}")]
    CantProcessRequest(String),

    #[error("Attempts reached: {0}")]
    AttemptsReached(String),
}

#[derive(Error, Debug)]
pub enum TradingError {
    #[error("Insufficient balance: {0}")]
    InsufficientBalance(String),

    #[error("Position not found: {0}")]
    PositionNotFound(String),

    #[error("Exchange error: {0}")]
    ExchangeError(String),

    #[error("Invalid leverage: {0}")]
    InvalidLeverage(String),

    #[error("Position opening failed: {0}")]
    PositionOpeningFailed(String),

    #[error("Position closing failed: {0}")]
    PositionClosingFailed(String),

    #[error("Atomic operation failed: {0}")]
    AtomicOperationFailed(String),

    #[error("Risk check failed: {0}")]
    RiskCheckFailed(String),

    #[error("Market data unavailable: {0}")]
    MarketDataUnavailable(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Request error: {0}")]
    RequestError(#[from] RequestError),

    #[error("Internal error: {0}")]
    InternalError(#[from] anyhow::Error),
}