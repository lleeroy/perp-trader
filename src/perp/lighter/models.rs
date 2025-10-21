use base64::{engine::general_purpose, Engine};
use chrono::Utc;
use lighter_rust::{EthereumSigner, LighterError, Signer};
use serde::{Deserialize, Serialize};

use crate::error::TradingError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterAccount {
    pub account_type: u8,
    pub index: i64,
    pub l1_address: String,
    pub total_order_count: i64,
    pub total_isolated_order_count: i64,
    pub pending_order_count: i64,
    pub available_balance: String,
    pub status: u8,
    pub collateral: String,
    pub account_index: i64,
    pub name: String,
    pub description: String,
    pub can_invite: bool,
    pub total_asset_value: String,
    pub cross_asset_value: String,
    pub positions: Option<Vec<LighterPosition>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterPosition {
    pub market_id: u8,
    pub symbol: String,
    pub initial_margin_fraction: String,
    pub open_order_count: i64,
    pub pending_order_count: i64,
    pub position_tied_order_count: i64,
    pub sign: i32,
    pub position: String,
    pub avg_entry_price: String,
    pub position_value: String,
    pub unrealized_pnl: String,
    pub realized_pnl: String,
    pub liquidation_price: String,
    pub total_funding_paid_out: Option<String>,
    pub allocated_margin: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LighterOrderType {
    Market,
    Limit,
    StopLoss,
    TakeProfit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterOrder {
    pub tx_type: i32,
    pub tx_info: LighterOrderInfo,
    pub price_protection: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LighterOrderInfo {
    #[serde(rename = "AccountIndex")]
    pub account_index: u32,
    #[serde(rename = "ApiKeyIndex")]
    pub api_key_index: u32,
    #[serde(rename = "MarketIndex")]
    pub market_index: u32,
    #[serde(rename = "ClientOrderIndex")]
    pub client_order_index: u64,
    #[serde(rename = "BaseAmount")]
    pub base_amount: u64,
    pub price: u64,
    #[serde(rename = "IsAsk")]
    pub is_ask: i32,
    #[serde(rename = "Type")]
    pub type_field: u32,
    #[serde(rename = "TimeInForce")]
    pub time_in_force: i32,
    #[serde(rename = "ReduceOnly")]
    pub reduce_only: i32,
    #[serde(rename = "TriggerPrice")]
    pub trigger_price: u64,
    #[serde(rename = "ExpiredAt")]
    pub expired_at: i64,
    #[serde(rename = "OrderExpiry")]
    pub order_expiry: i64,
    pub nonce: u64,
    #[serde(rename = "Sig")]
    pub signature: String,
}

impl LighterOrder {
    pub fn new(
        account_index: u32, 
        base_amount: u64, 
        price: u64, 
        is_ask: i32, 
        nonce: u64,
    ) -> Self {
        let expired_at = Utc::now().timestamp() + 60;

        Self {
            tx_info: LighterOrderInfo {
                account_index,
                base_amount,
                price,
                is_ask,
                client_order_index: 0,
                market_index: 0,
                type_field: 1,
                time_in_force: 0,
                reduce_only: 0,
                api_key_index: 0,
                trigger_price: 0,
                order_expiry: 0,
                expired_at,
                nonce,
                signature: "".to_string(),
            },
            tx_type: 14,
            price_protection: false,
        }
    }
}


impl LighterOrderInfo {
    pub fn to_message(&self) -> String {
        use serde_json::json;

        let message = json!({
            "AccountIndex": self.account_index,
            "BaseAmount": self.base_amount,
            "Price": self.price,
            "Ask": self.is_ask,
            "ClientOrderIndex": self.client_order_index,
            "MarketIndex": self.market_index,
            "Type": self.type_field,
            "ApiKeyIndex": self.api_key_index,
            "TimeInForce": self.time_in_force,
            "ReduceOnly": self.reduce_only,
            "TriggerPrice": self.trigger_price,
            "ExpiredAt": self.expired_at,
            "OrderExpiry": self.order_expiry,
            "Nonce": self.nonce,
        });

        serde_json::to_string(&message).unwrap()
    }

    pub fn sign(&self, signer: &EthereumSigner) -> Result<String, TradingError> {
        let message = self.to_message();
        let sig_hex = signer.sign_message(&message).map_err(|e| TradingError::SigningError(e.to_string()))?;
        let sig_bytes = hex::decode(sig_hex.trim_start_matches("0x")).map_err(|e| TradingError::SigningError(e.to_string()))?;
        let sig_base64 = general_purpose::STANDARD.encode(&sig_bytes);
        Ok(sig_base64)
    }
}