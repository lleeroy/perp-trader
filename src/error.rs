use thiserror::Error;

#[allow(dead_code)]
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