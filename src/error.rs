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
    #[error("Authentication failed")]
    AuthenticationFailed(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Insufficient balance: {0}")]
    InsufficientBalance(String),

    #[error("Position not found: {0}")]
    PositionNotFound(String),

    #[error("Exchange error: {0}")]
    ExchangeError(String),

    #[error("Order execution failed: {0}")]
    OrderExecutionFailed(String),

    #[error("Invalid nonce: {0}")]
    InvalidNonce(String),

    #[error("Position opening failed: {0}")]
    PositionOpeningFailed(String),

    #[error("Position closing failed: {0}")]
    PositionClosingFailed(String),

    #[error("Atomic operation failed: {0}")]
    AtomicOperationFailed(String),

    #[error("Market data unavailable: {0}")]
    MarketDataUnavailable(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Request error: {0}")]
    RequestError(#[from] RequestError),

    #[error("Storage error: {0}")]
    StorageError(#[from] sqlx::Error),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Internal error: {0}")]
    InternalError(#[from] anyhow::Error),

    #[error("Signing error: {0}")]
    SigningError(String),
}

