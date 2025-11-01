use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use crate::helpers::deserialize_decimal_from_string;

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
pub struct LighterPoints {
    pub user_total_points: f64,
    pub user_last_week_points: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterPosition {
    pub market_id: i32,
    pub symbol: String,
    pub initial_margin_fraction: String,
    pub open_order_count: i64,
    pub pending_order_count: i64,
    pub position_tied_order_count: i64,
    pub sign: i32,
    #[serde(deserialize_with = "deserialize_decimal_from_string")]
    pub position: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_string")]
    pub avg_entry_price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_string")]
    pub position_value: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_string")]
    pub unrealized_pnl: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_string")]
    pub realized_pnl: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_string")]
    pub liquidation_price: Decimal,
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
pub struct LighterTx {
    pub code: i32,
    pub status: i64,
    pub executed_at: i64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LighterOrderInfo {
    #[serde(rename = "AccountIndex")]
    pub account_index: u32,
    #[serde(rename = "ApiKeyIndex")]
    pub api_key_index: u32,
    #[serde(rename = "MarketIndex")]
    pub market_index: i32,
    #[serde(rename = "ClientOrderIndex")]
    pub client_order_index: i64,
    #[serde(rename = "BaseAmount")]
    pub base_amount: i64,
    pub price: i64,
    #[serde(rename = "IsAsk")]
    pub is_ask: bool,
    #[serde(rename = "Type")]
    pub type_field: i32,
    #[serde(rename = "TimeInForce")]
    pub time_in_force: i32,
    #[serde(rename = "ReduceOnly")]
    pub reduce_only: bool,
    #[serde(rename = "TriggerPrice")]
    pub trigger_price: i64,
    #[serde(rename = "ExpiredAt")]
    pub expired_at: i64,
    #[serde(rename = "OrderExpiry")]
    pub order_expiry: i64,
    pub nonce: i64,
    #[serde(rename = "Sig")]
    pub signature: String,
}

impl LighterOrder {
    pub fn new(
        account_index: u32, 
        market_index: i32,
        base_amount: i64, 
        price: i64, 
        is_ask: bool, 
        reduce_only: bool,
        nonce: i64,
    ) -> Self {
        let expired_at = Utc::now().timestamp_millis() + 60000;

        Self {
            tx_info: LighterOrderInfo {
                account_index,
                market_index,
                base_amount,
                price,
                is_ask,
                client_order_index: 0,
                type_field: 1,
                time_in_force: 0,
                reduce_only,
                api_key_index: 0,
                trigger_price: 0,
                order_expiry: 0,
                expired_at,
                nonce,
                signature: "".to_string(),
            },
            tx_type: super::signer::TX_TYPE_CREATE_ORDER,
            price_protection: false,
        }
    }
}


impl LighterPosition {
    #[allow(unused)]
    pub fn get_percentage_to_liquidation(&self) -> Decimal {
        let liq_price = self.liquidation_price;
        let entry_price = self.avg_entry_price;
        
        if entry_price == Decimal::ZERO || liq_price == Decimal::ZERO {
            return Decimal::ZERO;
        }
        
        match self.sign {
            1 => { // Long position
                if entry_price <= liq_price {
                    Decimal::ZERO
                } else {
                    (entry_price - liq_price) / entry_price * dec!(100)
                }
            },
            -1 => { // Short position
                if entry_price >= liq_price {
                    Decimal::ZERO
                } else {
                    (liq_price - entry_price) / entry_price * dec!(100)
                }
            },
            _ => Decimal::ZERO
        }
    }
}
