use std::{fs::File, io::BufReader};
use async_trait::async_trait;
use reqwest::{header::HeaderMap, Method};
use rust_decimal::Decimal;
use urlencoding;
use crate::{error::TradingError, model::{balance::Balance, token::Token, Position, PositionSide}, perp::{lighter::models::{LighterAccount, LighterOrder}, PerpExchange}, request::Request, trader::wallet::Wallet};
use lighter_rust::{EthereumSigner};

pub struct LighterClient {
    wallet: Wallet,
    account_index: u32,
    base_url: String,
    api_key: String,
    signer: EthereumSigner
}

impl LighterClient {
    /// Creates a new `LighterClient` instance using the provided wallet credentials.
    ///
    /// # Arguments
    ///
    /// * `wallet` - Reference to the user's wallet struct containing API secrets for authentication.
    ///
    /// # Returns
    ///
    /// * `LighterClient` - A client instance ready to communicate with the Lighter API.
    pub async fn new(wallet: &Wallet) -> Result<Self, TradingError> {
        let base_url = "https://mainnet.zklighter.elliot.ai/api/v1".to_string();

        let (api_key, account_index) = if wallet.lighter_api_key.is_empty() || wallet.lighter_api_key == "" || wallet.lighter_account_index == 0 {
            info!("#{} | Lighter API key or account index not found, generating new one...", wallet.id);
            let account_index = Self::get_account_index(&base_url, wallet).await?;
            let api_key = Self::get_api_key(&base_url, account_index).await?;
            
            info!("#{} | Lighter API key generated: {}", wallet.id, api_key);
            Self::save_api_key_and_account_index(wallet, &api_key, account_index).await?;

            (api_key, account_index)
        } else {
            info!("#{} | Lighter API key found, using existing one...", wallet.id);
            (wallet.lighter_api_key.clone(), wallet.lighter_account_index)
        };

        let signer = EthereumSigner::from_private_key(&wallet.private_key)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        Ok(Self { 
            wallet: wallet.clone(),
            base_url,
            api_key,
            account_index,
            signer,
        })
    }


    pub async fn is_authenticated(&self) -> bool {
        self.get_account().await.is_ok()
    }

    
    pub async fn get_account(&self) -> Result<LighterAccount, TradingError> {
        let url = format!("{}/account?by=l1_address&value={}", self.base_url, self.wallet.address);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;

        match response["accounts"].as_array() {
            Some(accounts) => {
                if accounts.is_empty() {
                    return Err(TradingError::InvalidInput("No accounts found".to_string()));
                }

                let account = serde_json::from_value(accounts[0].clone())
                    .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

                Ok(account)
            },
            None => return Err(TradingError::InvalidInput("Accounts not found".to_string())),
        }
    }

    async fn get_api_key(base_url: &str, account_index: u32) -> Result<String, TradingError> {
        let url = format!("{}/apikeys?account_index={}", base_url, account_index);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;

        match response["api_keys"].as_array() {
            Some(api_keys) => {
                if api_keys.is_empty() {
                    return Err(TradingError::InvalidInput("No API keys found".to_string()));
                }

                match api_keys[0]["public_key"].as_str() {
                    Some(public_key) => Ok(public_key.to_string()),
                    None => return Err(TradingError::InvalidInput(format!("Public key not found in response: {:?}", response))),
                }
            },
            None => return Err(TradingError::InvalidInput(format!("API keys not found in response: {:?}", response))),
        }
    }

    async fn get_account_index(base_url: &str, wallet: &Wallet) -> Result<u32, TradingError> {
        let url = format!("{}/accountsByL1Address?l1_address={}", base_url, wallet.address);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;

        match response["sub_accounts"][0]["index"].as_u64() {
            Some(index) => Ok(index as u32),
            None => return Err(TradingError::InvalidInput(format!("Account index not found in response: {:?}", response))),
        }
    }


    async fn get_nonce(&self) -> Result<u64, TradingError> {
        let url = format!("{}/nextNonce?account_index={}&api_key_index=0", self.base_url, self.account_index);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;

        match response["nonce"].as_u64() {
            Some(nonce) => Ok(nonce),
            None => return Err(TradingError::InvalidInput(format!("Nonce not found in response: {:?}", response))),
        }
    }

    async fn execute_market_buy_order(
        &self,
        token: Token,
        side: PositionSide,
        base_amount: u64,
        price: u64,
    ) -> Result<String, TradingError> {
        let nonce = self.get_nonce().await?;

        let is_ask = match side {
            PositionSide::Long => 1,
            PositionSide::Short => 0,
        };

        info!("#{} | Executing market {} order for {:?} with price {} | nonce: {}", self.wallet.id, side, token, price, nonce);

        let mut order = LighterOrder::new(
            self.account_index,
            base_amount,
            price,
            is_ask,
            nonce,
        );

        let order_signature = order.tx_info.sign(&self.signer)?;
        order.tx_info.signature = order_signature;
        let order_id = self.send_order(&order).await?;
        
        Ok(order_id)

        // let order = client
        //     .orders()
        //     .create_order(
        //         symbol,
        //         lighter_rust::Side::Buy,
        //         lighter_rust::OrderType::Market,
        //         quantity,
        //         Some(&buy_price.to_string()),
        //         None,
        //         None,
        //         None,
        //         Some(true),
        //         None,
        //     )
        //     .await.map_err(|e| TradingError::OrderExecutionFailed(e.to_string()))?;
    
    }


    async fn send_order(&self, order: &LighterOrder) -> Result<String, TradingError> {
        let mut headers = self.get_headers();
        let url = format!("{}/sendTx", self.base_url);
        
        // Add Authorization header
        let auth_header = format!("{}:{}:0:{}", self.account_index, 0, self.api_key);
        headers.insert(
            http::header::HeaderName::from_static("authorization"),
            http::header::HeaderValue::from_str(&auth_header)
                .map_err(|e| TradingError::InvalidInput(e.to_string()))?,
        );
        
        let tx_info_json = serde_json::to_string(&order.tx_info)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        let tx_info_encoded = urlencoding::encode(&tx_info_json);
        
        // Create form-urlencoded body
        let body = format!(
            "tx_type={}&tx_info={}&price_protection={}",
            order.tx_type,
            tx_info_encoded,
            order.price_protection
        );
        
        println!("Order Body: {}", body);
        
        let response = Request::process_request(
            Method::POST, 
            url, 
            Some(headers), 
            Some(body), 
            None
        ).await?;

        println!("Response: {:#?}", response);
        
        // Return the response as string or extract order ID
        Ok(serde_json::to_string(&response).unwrap_or_else(|_| "Order sent successfully".to_string()))
    }

    /// Updates the lighter_api_key field for the wallet with the given id in api-keys.json.
    async fn save_api_key_and_account_index(wallet: &Wallet, api_key: &str, account_index: u32) -> Result<(), TradingError> {
        // Open the api-keys.json file and parse as serde_json Value
        let file = File::open("api-keys.json").map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        let reader = BufReader::new(file);
        let mut wallets_map: serde_json::Value = serde_json::from_reader(reader)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        // Prepare the id key as string, e.g. "1"
        let id_key = wallet.id.to_string();

        // Find the wallet object by id key and update lighter_api_key field
        if let serde_json::Value::Object(ref mut map) = wallets_map {
            if let Some(wallet_value) = map.get_mut(&id_key) {
                if let serde_json::Value::Object(ref mut wallet_obj) = wallet_value {
                    wallet_obj.insert(
                        "lighter_api_key".to_string(),
                        serde_json::Value::String(api_key.to_string()),
                    );
                    wallet_obj.insert(
                        "lighter_account_index".to_string(),
                        serde_json::Value::Number(account_index.into()),
                    );
                } else {
                    return Err(TradingError::InvalidInput(format!(
                        "Wallet entry for id '{}' is not an object",
                        id_key
                    )));
                }
            } else {
                return Err(TradingError::InvalidInput(format!(
                    "Wallet with id '{}' not found in api-keys.json",
                    id_key
                )));
            }
        } else {
            return Err(TradingError::InvalidInput(
                "api-keys.json does not contain a valid object".to_string(),
            ));
        }

        // Write the updated JSON back to the file
        let file = File::create("api-keys.json").map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        serde_json::to_writer_pretty(file, &wallets_map)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        info!("#{} | Lighter API key saved successfully...", wallet.id);
        Ok(())
    }

    fn get_headers(&self) -> HeaderMap {
        use http::header::{HeaderMap, HeaderName, HeaderValue};
        let mut headers = HeaderMap::new();

        headers.insert(HeaderName::from_static("connection"), HeaderValue::from_static("keep-alive"));
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/x-www-form-urlencoded;charset=UTF-8"),
        );
        headers.insert(
            HeaderName::from_static("origin"),
            HeaderValue::from_static("https://app.lighter.xyz"),
        );
        headers.insert(
            HeaderName::from_static("preferauthserver"),
            HeaderValue::from_static("true"),
        );
        headers.insert(
            HeaderName::from_static("referer"),
            HeaderValue::from_static("https://app.lighter.xyz/"),
        );
        headers.insert(
            HeaderName::from_static("sec-fetch-dest"),
            HeaderValue::from_static("empty"),
        );
        headers.insert(
            HeaderName::from_static("sec-fetch-mode"),
            HeaderValue::from_static("cors"),
        );
        headers.insert(
            HeaderName::from_static("sec-fetch-site"),
            HeaderValue::from_static("cross-site"),
        );
        headers.insert(
            HeaderName::from_static("user-agent"),
            HeaderValue::from_static(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
            ),
        );
        headers.insert(
            HeaderName::from_static("sec-ch-ua"),
            HeaderValue::from_static(r#"\"Google Chrome\";v=\"141\", \"Not?A_Brand\";v=\"8\", \"Chromium\";v=\"141\""#),
        );
        headers.insert(
            HeaderName::from_static("sec-ch-ua-mobile"),
            HeaderValue::from_static("?0"),
        );
        headers.insert(
            HeaderName::from_static("sec-ch-ua-platform"),
            HeaderValue::from_static(r#""macOS""#),
        );

        headers
    }
}

#[async_trait]
impl PerpExchange for LighterClient {
    fn name(&self) -> &str {
        "Lighter"
    }

    async fn health_check(&self) -> Result<bool, TradingError> {
        Ok(self.is_authenticated().await)
    }

    async fn get_balance(&self, _asset: &str) -> Result<Balance, TradingError> {
        let available_balance = self.get_account().await?
            .available_balance
            .parse::<Decimal>()
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        Ok(Balance {
            asset: "USDC".to_string(),
            free: available_balance,
            locked: Decimal::ZERO,
        })
    }

    async fn get_balances(&self) -> Result<Vec<Balance>, TradingError> {
        // TODO: Implement actual API call
        log::warn!("Lighter get_balances not fully implemented");
        Ok(vec![])
    }

    async fn open_position(&self, token: Token, side: PositionSide, _amount_usdc: Decimal) -> Result<Position, TradingError> {
        let trade_quantity = 10;
        let price = 0;
        let order = self.execute_market_buy_order(token, side, trade_quantity, price).await?;

        println!("Order ID: {:?}", order);
        todo!("Lighter open_position not fully implemented for {}", side);
    }


    async fn get_usdc_balance(&self) -> Result<Decimal, TradingError> {
        // TODO: Implement actual API call
        log::warn!("Lighter get_usdc_balance not fully implemented");
        Ok(Decimal::ZERO)
    }

    async fn close_position(&self, position: &Position) -> Result<Position, TradingError> {
        // TODO: Implement actual API call
        todo!("Lighter close_position not fully implemented for {}", position.side);
    }
}

