#![allow(dead_code)]

use anyhow::{Result};
use log::warn;
use reqwest::{header::HeaderMap, Method, Proxy, StatusCode, Client};
use serde_json::Value;
use std::time::Duration;
use tokio::{sync::OnceCell, time::sleep};
use crate::error::RequestError;

/// Global HTTP client instance using OnceCell for lazy initialization  
static HTTP_CLIENT: OnceCell<Client> = OnceCell::const_new();

pub struct Request;

impl Request {
    /// Gets the global HTTP client instance, initializing it if necessary
    ///
    /// # Returns
    /// * `Result<&'static Client>` - Global HTTP client instance
    async fn get_client() -> Result<&'static Client, RequestError> {
        HTTP_CLIENT
            .get_or_try_init(|| async {
                Self::create_client(None)
            })
            .await
    }



    /// Creates an HTTP client with connection pooling for better performance
    fn create_client(proxy_url: Option<&str>) -> Result<Client, RequestError> {
        let mut client_builder = Client::builder()
            .timeout(Duration::from_secs(10))  // 10s timeout
            .connect_timeout(Duration::from_secs(10))  // Fast connection establishment
            .read_timeout(Duration::from_secs(10))     // Quick read timeout
            .pool_max_idle_per_host(200)  // Very high pool for massive concurrency (300+ tasks * multiple hosts)
            .pool_idle_timeout(Duration::from_secs(60))  // Longer idle timeout for connection reuse
            .tcp_keepalive(Duration::from_secs(30))
            .no_proxy();  // Disable system proxy detection to avoid macOS system-configuration issues

        if let Some(proxy) = proxy_url {
            // Proxy format must be a valid URI, e.g. http://user:pass@ip:port or http://ip:port
            // If the proxy string is in the form "ip:port:user:pass", convert it to "http://user:pass@ip:port"
            let formatted_proxy = {
                let parts: Vec<_> = proxy.splitn(4, ':').collect();
                if parts.len() == 4 {
                    // Format as http://user:pass@ip:port
                    let (ip, port, user, pass) = (parts[0], parts[1], parts[2], parts[3]);
                    format!("http://{}:{}@{}:{}", user, pass, ip, port)
                } else {
                    // Try to use as http://{proxy}
                    format!("http://{proxy}")
                }
            };

            let proxy = Proxy::all(&formatted_proxy)
                .map_err(|e| RequestError::ConnectionError(format!(
                    "Proxy string '{}' (parsed as '{}') error: {}",
                    proxy, formatted_proxy, e
                )))?;
            client_builder = client_builder.proxy(proxy);
        }

        Ok(client_builder.build().map_err(|e| RequestError::ConnectionError(e.to_string()))?)
    }

    /// Processes an HTTP request with the given method, URL, body, and headers.
    /// Retries the request if it fails, up to a maximum number of attempts.
    ///
    /// # Arguments
    ///
    /// * `method` - The HTTP method to use for the request (GET, POST, etc.).
    /// * `url` - The URL to send the request to.
    /// * `body` - An optional JSON body to include in the request (for POST requests).
    /// * `headers` - Optional HTTP headers to include in the request.
    ///
    /// # Returns
    ///
    /// A `Result` containing the JSON response body if the request
    /// is successful, or an `anyhow::Error` if it fails.
    ///
    /// # Errors
    ///
    /// This function returns an error if the input URL cannot be parsed or
    /// if the request method is not supported.
    ///
    /// If the request fails after the maximum number
    /// of attempts, an error is also returned.
    pub async fn process_request<S: AsRef<str>>(
        method: Method,
        url: S,
        headers: Option<HeaderMap>,
        body: Option<String>,
        proxy_url: Option<S>,
    ) -> Result<Value, RequestError> {
        let attempts_limit = 5;  // Quick retry: 1 retry for fast failure
        let mut attempt = 1;

        let wait_delay = Duration::from_secs_f64(0.5);  // Fast retry delay
        
        // Get global client instance (reuse existing connection pool)
        // For proxy requests, we need to create a new client since global client doesn't support dynamic proxies
        let client = match proxy_url.as_ref().map(|s| s.as_ref()) {
            Some(proxy) => Self::create_client(Some(proxy))?,
            None => Self::get_client().await?.clone(),
        };
        
        let url = reqwest::Url::parse(url.as_ref()).map_err(|e| RequestError::ApiError(e.to_string()))?;
        let headers = headers.unwrap_or_else(HeaderMap::new);

        while attempt <= attempts_limit {
            let request = match method.clone() {
                Method::GET => client
                    .request(method.clone(), url.clone())
                    .headers(headers.clone()),
                Method::POST => client
                    .request(method.clone(), url.clone())
                    .body(body.as_ref().unwrap().clone())
                    .headers(headers.clone()),
                _ => return Err(RequestError::MethodNotSupported(format!("The method <{}> is not supported.", method))),
            };

            match request.send().await {
                Ok(res) => match res.status() {
                    StatusCode::OK => {
                        let json: Value = res.json().await.map_err(|e| RequestError::ApiError(e.to_string()))?;
                        return Ok(json);
                    }

                    StatusCode::TOO_MANY_REQUESTS => {
                        sleep(wait_delay * attempt).await;
                        attempt += 1;
                        continue;
                    }

                    StatusCode::NOT_FOUND => {
                        return Err(RequestError::CantProcessRequest(
                            format!(
                                "Status: {} | Can't process request. Text: {}", 
                                res.status(), res.text().await.map_err(|e| RequestError::ApiError(e.to_string()))?)
                        ));
                    }

                    StatusCode::GATEWAY_TIMEOUT => {
                        return Err(RequestError::TimeoutError(format!(
                            "🚨 URL: {} Status: {} | Can't process request.",
                            res.url().to_string(),
                            res.status()
                        )));
                    }

                    StatusCode::INTERNAL_SERVER_ERROR => {
                        warn!(
                            "URL: {} | ISE: {:#?}",
                            res.url().to_string(),
                            res.text().await.map_err(|e| RequestError::ApiError(e.to_string()))?
                        );

                        let wait_delay = wait_delay * 3;
                        warn!("Sleeping {}s and trying again...", wait_delay.as_secs());
                        attempt += 1;
                        continue;
                    }

                    _ => {
                        return Err(RequestError::ApiError(format!(
                            "Critical response error. URL: {} Status: {} | {:#?}",
                            res.url().to_string(),
                            res.status(),
                            res.text().await.map_err(|e| RequestError::ApiError(e.to_string()))?
                        )));
                    }
                },
                Err(e) => {
                    // Handle specific timeout and connection errors
                    if e.is_timeout() {
                        warn!("Request timeout for URL: {} (attempt {}/{})", url.as_str(), attempt, attempts_limit);
                        if attempt < attempts_limit {
                            let backoff_delay = wait_delay * attempt;
                            warn!("Retrying after {:?} delay...", backoff_delay);
                            sleep(backoff_delay).await;
                            attempt += 1;
                            continue;
                        } else {
                            return Err(RequestError::TimeoutError(format!("Request timeout after {} attempts for URL: {}", attempts_limit, url.as_str())));
                        }
                    } else if e.is_connect() {
                        warn!("Connection error for URL: {} (attempt {}/{}): {}", url.as_str(), attempt, attempts_limit, e);
                        if attempt < attempts_limit {
                            let backoff_delay = wait_delay * attempt;
                            warn!("Retrying after {:?} delay...", backoff_delay);
                            sleep(backoff_delay).await;
                            attempt += 1;
                            continue;
                        } else {
                            return Err(RequestError::ConnectionError(format!("Connection failed after {} attempts for URL: {}: {}", attempts_limit, url.as_str(), e)));
                        }
                    } else {
                        // For other errors, fail immediately
                        return Err(RequestError::ApiError(format!("Request failed for URL: {}: {}", url.as_str(), e)));
                    }
                }
            }
        }

        return Err(RequestError::AttemptsReached(format!("Attempts reached. Check URL: {}", url.as_str())));
    }
}
