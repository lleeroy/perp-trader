#![allow(unused)]

use async_trait::async_trait;
use bpx_api_client::types::order::{ExecuteOrderPayload, Order, OrderType, Side};
use chrono::{DateTime, Duration};
use rust_decimal::Decimal;
use crate::error::TradingError;
use crate::model::token::Token;
use crate::model::{position::{Position, PositionSide, PositionStatus}};
use crate::model::{balance::Balance, Exchange};
use crate::perp::PerpExchange;
use crate::trader::wallet::Wallet;
use bpx_api_client::{BACKPACK_API_BASE_URL, BpxClient};

/// Backpack exchange client for interacting with the Backpack perpetual futures exchange.
pub struct BackpackClient {
    client: BpxClient,
    wallet: Wallet,
}

impl BackpackClient {
    /// Creates a new `BackpackClient` instance using the provided wallet credentials.
    ///
    /// # Arguments
    ///
    /// * `wallet` - Reference to the user's wallet struct containing API secrets for authentication.
    ///
    /// # Returns
    ///
    /// * `BackpackClient` - A client instance ready to communicate with the Backpack API.
    pub fn new(wallet: &Wallet) -> Self {
        let client = BpxClient::init(
            BACKPACK_API_BASE_URL.to_string(),
            &wallet.backpack_api_secret, 
            None
        ).unwrap();

        Self {
            client,
            wallet: wallet.clone(),
        }
    }

    /// Checks if the client is currently authenticated with the Backpack API.
    ///
    /// # Returns
    ///
    /// * `bool` - Returns `true` if authentication is successful, else `false`.
    pub async fn is_authenticated(&self) -> bool {
        self.client.get_account().await.is_ok()
    }

    /// Builds the payload for the execute order request.
    ///
    /// # Arguments
    ///
    /// * `token` - The token to trade.
    /// * `side` - The side of the position to open.
    /// * `amount_usdc` - The amount of USDC to trade.
    ///
    /// # Returns
    ///
    /// * `ExecuteOrderPayload` - The payload for the execute order request.
    fn build_payload(&self, 
        token: &Token, 
        side: PositionSide, 
        amount_usdc: Decimal,
    ) -> ExecuteOrderPayload {
        ExecuteOrderPayload {
            symbol: token.symbol.clone(),
            auto_lend: Some(true),
            auto_lend_redeem: Some(true),
            auto_borrow: Some(true),
            auto_borrow_repay: Some(true),
            order_type: OrderType::Market,
            quote_quantity: Some(amount_usdc.round_dp(2)),
            side: match side {
                PositionSide::Long => Side::Ask,
                PositionSide::Short => Side::Bid,
            },
            ..Default::default()
        }
    }
}

#[async_trait]
impl PerpExchange for BackpackClient {
    /// Returns the name of the exchange ("Backpack").
    fn name(&self) -> &str {
        "Backpack"
    }

    /// Checks the health of the Backpack exchange client.
    ///
    /// # Returns
    ///
    /// * `Ok(true)` if authentication check passes, otherwise `Ok(false)` or an error.
    async fn health_check(&self) -> Result<bool, TradingError> {
        Ok(self.is_authenticated().await)
    }

    /// Fetches the balance for a specific asset from the Backpack exchange.
    ///
    /// # Arguments
    ///
    /// * `asset` - The asset symbol (e.g., "USDC") for which to fetch the balance.
    ///
    /// # Returns
    ///
    /// * `Ok(Balance)` containing free and locked balances for the asset, or a `TradingError`.
    async fn get_balance(&self, asset: &str) -> Result<Balance, TradingError> {
        let balances = self.client.get_balances()
            .await.map_err(|e| TradingError::ExchangeError(e.to_string()))?;

        let balance = balances.iter().find(|b| 
            b.0.to_lowercase() == asset.to_lowercase())
            .ok_or(TradingError::ExchangeError(format!("Asset {} not found", asset)))?;
        
        Ok(Balance {
            asset: balance.0.clone(),
            free: balance.1.available,
            locked: balance.1.locked,
        })
    }

    /// Fetches all available balances from the Backpack exchange.
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<Balance>)` - A vector of balances for all supported assets, or a `TradingError`.
    async fn get_balances(&self) -> Result<Vec<Balance>, TradingError> {
        let balances = self.client.get_balances()
            .await.map_err(|e| TradingError::ExchangeError(e.to_string()))?;
        
        Ok(balances.iter().map(|b| Balance {
            asset: b.0.clone(),
            free: b.1.available,
            locked: b.1.locked,
        }).collect())
    }

    /// Opens a new position on the Backpack exchange.
    ///
    /// # Arguments
    ///
    /// * `side` - The side of the position to open (Long or Short).
    ///
    /// # Returns
    ///
    /// * `Ok(Position)` containing the details of the opened position, or a `TradingError`.
    async fn open_position(&self, token: Token, side: PositionSide, amount_usdc: Decimal) -> Result<Position, TradingError> {
        info!("#{} | <{}> opening position on {} with amount {:.2}USDC", self.wallet.id, side, token.symbol, amount_usdc);
        let payload = self.build_payload(&token, side, amount_usdc);

        let order = self.client.execute_order(payload)
            .await
            .map_err(|e| TradingError::OrderExecutionFailed(e.to_string()))?;
        
        match order {
            Order::Market(market_order) => {
                let size = market_order.quantity
                    .or(Some(market_order.executed_quantity))
                    .ok_or_else(|| TradingError::OrderExecutionFailed(
                        "Order quantity not available".to_string()
                ))?;

                let opened_at = DateTime::from_timestamp_millis(market_order.created_at as i64)
                    .ok_or_else(|| TradingError::OrderExecutionFailed(
                        "Invalid timestamp from order".to_string()
                ))?;

                let close_at = opened_at + Duration::days(1);
                let updated_at = opened_at;

                info!("#{} | 🟢 <{}> position opened on {} | Size: {} | Spent: {:.2} USDC", 
                    self.wallet.id, side, token.symbol, size, market_order.executed_quote_quantity);
                
                Ok(Position {
                    wallet_id: self.wallet.id,
                    id: market_order.id.clone(),
                    strategy_id: None,
                    exchange: Exchange::Backpack,
                    symbol: token.symbol,
                    side,
                    size,
                    status: PositionStatus::Open,
                    opened_at,
                    close_at,
                    closed_at: None,
                    realized_pnl: None,
                    updated_at,
                })
            }
            _ => Err(TradingError::OrderExecutionFailed(
                "Expected market order but received different order type".to_string()
            )),
        }
    }

    /// Fetches the USDC balance for the account from the Backpack exchange.
    ///
    /// # Returns
    ///
    /// * `Ok(Decimal)` containing the USDC balance, or a `TradingError`.
    async fn get_usdc_balance(&self) -> Result<Decimal, TradingError> {
        let balance = self.get_balance("USDC").await?;
        Ok(balance.free)
    }
}
