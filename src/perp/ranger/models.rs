use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct MarketOrderRequest {
    pub fee_payer: String,
    pub symbol: String,
    pub side: String,
    pub adjustment_type: String,
    pub size: f64,
    pub collateral: f64,
    pub size_denomination: String,
    pub collateral_denomination: String,
    pub evm_address: String,
    pub limit_price: Option<f64>,
    pub target_venues: Option<Vec<String>>,
    pub venue_type: String,
    pub slippage_bps: u32,
    pub trigger_price: Option<f64>,
    pub order_id: Option<String>,
    pub sub_account_id: Option<String>,
    pub is_stop_loss: Option<bool>,
}

impl Default for MarketOrderRequest {
    fn default() -> Self {
        Self {
            fee_payer: "".to_string(),
            symbol: "".to_string(),
            side: "".to_string(),
            adjustment_type: "Increase".to_string(),
            size: 0.0,
            collateral: 0.0,
            size_denomination: "".to_string(),
            collateral_denomination: "USDC".to_string(),
            evm_address: "".to_string(),
            limit_price: None,
            target_venues: None,
            venue_type: "All".to_string(),
            slippage_bps: 100,
            trigger_price: None,
            order_id: None,
            sub_account_id: None,
            is_stop_loss: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MarketOrderResponse {
    pub message: String,
    pub jito: bool,
    pub meta: Meta,
    pub hyperliquid_payload: HyperliquidPayload,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
    pub venues: Vec<Venue>,
    pub total_collateral: f64,
    pub total_size: f64,
    pub average_price: f64,
    pub execution_method: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Venue {
    pub venue_name: String,
    pub collateral: f64,
    pub size: f64,
    pub quote: Quote,
    pub order_available_liquidity: f64,
    pub venue_available_liquidity: f64,
    #[serde(rename = "venue_avaliable_liquidity_in_usd")]
    pub venue_available_liquidity_in_usd: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Quote {
    pub base: f64,
    pub total_fee_per_unit: f64,
    pub total_price_per_unit: f64,
    pub total_fees: f64,
    pub fee_breakdown: FeeBreakdown,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeeBreakdown {
    pub base_fee_per_unit: f64,
    pub open_fee: f64,
    pub spread_fee: f64,
    pub volatility_fee: f64,
    pub margin_fee: f64,
    pub close_fee: f64,
    pub other_fees: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HyperliquidPayload {
    pub calculated_leverage: f64,
    pub collateral_usd: f64,
    pub estimated_fee_usd: f64,
    pub place_order: PlaceOrder,
    pub total_price: f64,
    pub update_isolated_margin: Option<UpdateIsolatedMargin>,
    pub update_leverage: UpdateLeverage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaceOrder {
    pub action_payload: ActionPayload,
    pub digest_to_sign: String,
    pub eip712_chain_name: Option<String>,
    pub eip712_domain_chain_id: i64,
    pub is_l1_agent_signature: bool,
    pub metadata: Metadata,
    pub nonce: i64,
    pub vault_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateIsolatedMargin {
    pub action_payload: UpdateIsolatedMarginPayload,
    pub digest_to_sign: String,
    pub eip712_chain_name: Option<String>,
    pub eip712_domain_chain_id: i64,
    pub is_l1_agent_signature: bool,
    pub metadata: Metadata,
    pub nonce: i64,
    pub vault_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateLeverage {
    pub action_payload: UpdateLeveragePayload,
    pub digest_to_sign: String,
    pub eip712_chain_name: Option<String>,
    pub eip712_domain_chain_id: i64,
    pub is_l1_agent_signature: bool,
    pub metadata: Metadata,
    pub nonce: i64,
    pub vault_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActionPayload {
    pub builder: Builder,
    pub grouping: String,
    pub orders: Vec<Order>,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Builder {
    pub b: String,
    pub f: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Order {
    pub a: i64,
    pub b: bool,
    pub p: String,
    pub r: bool,
    pub s: String,
    pub t: OrderType,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OrderType {
    pub limit: Limit,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Limit {
    pub tif: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateIsolatedMarginPayload {
    pub asset: i64,
    #[serde(rename = "isBuy")]
    pub is_buy: bool,
    pub ntli: i64,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateLeveragePayload {
    pub asset: i64,
    #[serde(rename = "isCross")]
    pub is_cross: bool,
    pub leverage: i64,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub action_type: String,
    pub eip712_domain: Eip712Domain,
    pub prepared_at: String,
    pub signature_type: String,
    pub vault_address: Option<String>,
    pub venue: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Eip712Domain {
    pub chain_id: i64,
    pub chain_name: Option<String>,
}

// Authentication structs
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthRequest {
    pub message: String,
    pub signature: String,
    #[serde(rename = "walletClientType")]
    pub wallet_client_type: String,
    #[serde(rename = "connectorType")]
    pub connector_type: String,
    pub mode: String,
    pub message_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: User,
    // Add other fields as needed
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    // Add other user fields as needed
}
