
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Account {
    pub wallet_id: u32,
    pub wallet_key: String,
    pub wallet_address: String,
    pub okx_deposit_address: String,
    pub aptos_address: Option<String>,
    pub aptos_key: Option<String>,
    pub okx_aptos_address: Option<String>,
}