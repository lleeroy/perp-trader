//! # SignerClient - Rust implementation of the Lighter Protocol Signer
//!
//! This module provides a comprehensive Rust implementation of the Python SignerClient,
//! offering FFI bindings to the Go-based signing library for the Lighter Protocol.
//!
//! ## Features
//!
//! - **FFI Integration**: Seamless integration with Go signing library via FFI
//! - **Multi-API Key Support**: Manage multiple API keys with automatic nonce tracking
//! - **Transaction Signing**: Support for all Lighter transaction types:
//!   - Create/Cancel/Modify Orders
//!   - Withdraw funds
//!   - Transfer between accounts
//!   - Manage public pools
//!   - Update leverage
//!   - And more...
//! - **Thread-Safe**: Built with Arc and Mutex for safe concurrent usage
//! - **Platform Support**: macOS (ARM64), Linux (x86_64), Windows (x86_64)
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use perp_trader::perp::lighter::signer::SignerClient;
//! use std::collections::HashMap;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a new signer client
//! let client = SignerClient::new(
//!     "https://mainnet.zklighter.elliot.ai/api/v1",
//!     "your_private_key_here",
//!     0,  // api_key_index
//!     123, // account_index
//!     None, // max_api_key_index
//!     None, // additional_private_keys
//! )?;
//!
//! // Get next nonce
//! let (api_key, nonce) = client.get_next_nonce();
//!
//! // Sign a create order transaction
//! let tx_info = client.sign_create_order(
//!     0,      // market_index
//!     1,      // client_order_index
//!     1000,   // base_amount
//!     50000,  // price
//!     true,   // is_ask
//!     0,      // order_type (LIMIT)
//!     1,      // time_in_force (GTT)
//!     false,  // reduce_only
//!     0,      // trigger_price
//!     -1,     // order_expiry
//!     nonce,
//! )?;
//!
//! println!("Signed transaction: {}", tx_info);
//! # Ok(())
//! # }
//! ```
//!
//! ## Multi-API Key Setup
//!
//! ```rust,no_run
//! # use perp_trader::perp::lighter::signer::SignerClient;
//! # use std::collections::HashMap;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut additional_keys = HashMap::new();
//! additional_keys.insert(1, "private_key_1".to_string());
//! additional_keys.insert(2, "private_key_2".to_string());
//!
//! let client = SignerClient::new(
//!     "https://mainnet.zklighter.elliot.ai/api/v1",
//!     "private_key_0",
//!     0,  // start API key
//!     123,
//!     Some(2), // end API key
//!     Some(additional_keys),
//! )?;
//!
//! // Switch between API keys
//! client.switch_api_key(1)?;
//! # Ok(())
//! # }
//! ```

use crate::error::TradingError;
use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_longlong};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// Transaction Types
#[allow(dead_code)]
pub const TX_TYPE_CHANGE_PUB_KEY: i32 = 8;
#[allow(dead_code)]
pub const TX_TYPE_CREATE_SUB_ACCOUNT: i32 = 9;
#[allow(dead_code)]
pub const TX_TYPE_CREATE_PUBLIC_POOL: i32 = 10;
#[allow(dead_code)]
pub const TX_TYPE_UPDATE_PUBLIC_POOL: i32 = 11;
#[allow(dead_code)]
pub const TX_TYPE_TRANSFER: i32 = 12;
#[allow(dead_code)]
pub const TX_TYPE_WITHDRAW: i32 = 13;
#[allow(dead_code)]
pub const TX_TYPE_CREATE_ORDER: i32 = 14;
#[allow(dead_code)]
pub const TX_TYPE_CANCEL_ORDER: i32 = 15;
#[allow(dead_code)]
pub const TX_TYPE_CANCEL_ALL_ORDERS: i32 = 16;
#[allow(dead_code)]
pub const TX_TYPE_MODIFY_ORDER: i32 = 17;
#[allow(dead_code)]
pub const TX_TYPE_MINT_SHARES: i32 = 18;
#[allow(dead_code)]
pub const TX_TYPE_BURN_SHARES: i32 = 19;
#[allow(dead_code)]
pub const TX_TYPE_UPDATE_LEVERAGE: i32 = 20;

// Order Types
#[allow(dead_code)]
pub const ORDER_TYPE_LIMIT: i32 = 0;
#[allow(dead_code)]
pub const ORDER_TYPE_MARKET: i32 = 1;
#[allow(dead_code)]
pub const ORDER_TYPE_STOP_LOSS: i32 = 2;
#[allow(dead_code)]
pub const ORDER_TYPE_STOP_LOSS_LIMIT: i32 = 3;
#[allow(dead_code)]
pub const ORDER_TYPE_TAKE_PROFIT: i32 = 4;
#[allow(dead_code)]
pub const ORDER_TYPE_TAKE_PROFIT_LIMIT: i32 = 5;
#[allow(dead_code)]
pub const ORDER_TYPE_TWAP: i32 = 6;

// Time in Force
#[allow(dead_code)]
pub const ORDER_TIME_IN_FORCE_IMMEDIATE_OR_CANCEL: i32 = 0;
#[allow(dead_code)]
pub const ORDER_TIME_IN_FORCE_GOOD_TILL_TIME: i32 = 1;
#[allow(dead_code)]
pub const ORDER_TIME_IN_FORCE_POST_ONLY: i32 = 2;

// Cancel All TIF
#[allow(dead_code)]
pub const CANCEL_ALL_TIF_IMMEDIATE: i32 = 0;
#[allow(dead_code)]
pub const CANCEL_ALL_TIF_SCHEDULED: i32 = 1;
#[allow(dead_code)]
pub const CANCEL_ALL_TIF_ABORT: i32 = 2;

// Margin Modes
#[allow(dead_code)]
pub const CROSS_MARGIN_MODE: i32 = 0;
#[allow(dead_code)]
pub const ISOLATED_MARGIN_MODE: i32 = 1;

// Defaults
#[allow(dead_code)]
pub const NIL_TRIGGER_PRICE: i32 = 0;
#[allow(dead_code)]
pub const DEFAULT_28_DAY_ORDER_EXPIRY: i64 = -1;
#[allow(dead_code)]
pub const DEFAULT_IOC_EXPIRY: i64 = 0;
#[allow(dead_code)]
pub const DEFAULT_10_MIN_AUTH_EXPIRY: i64 = -1;
#[allow(dead_code)]
pub const MINUTE: i64 = 60;

#[allow(dead_code)]
pub const USDC_TICKER_SCALE: i64 = 1_000_000;
#[allow(dead_code)]
pub const CODE_OK: i32 = 200;

#[derive(Debug)]
#[repr(C)]
pub struct ApiKeyResponse {
    pub private_key: *mut c_char,
    pub public_key: *mut c_char,
    pub err: *mut c_char,
}

#[derive(Debug)]
#[repr(C)]
pub struct StrOrErr {
    pub str: *mut c_char,
    pub err: *mut c_char,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxResponse {
    pub code: i32,
    pub tx_hash: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TransactionInfo {
    #[serde(rename = "MessageToSign", skip_serializing_if = "Option::is_none")]
    pub message_to_sign: Option<String>,
    #[serde(rename = "L1Sig", skip_serializing_if = "Option::is_none")]
    pub l1_sig: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

/// Nonce manager to track nonces for each API key
struct NonceManager {
    nonces: HashMap<i32, i64>,
    current_api_key: i32,
}

impl NonceManager {
    fn new(start_api_key: i32) -> Self {
        Self {
            nonces: HashMap::new(),
            current_api_key: start_api_key,
        }
    }

    fn next_nonce(&mut self) -> (i32, i64) {
        let nonce = self.nonces.entry(self.current_api_key).or_insert(0);
        *nonce += 1;
        (self.current_api_key, *nonce)
    }

    fn get_nonce(&self, api_key_index: i32) -> i64 {
        *self.nonces.get(&api_key_index).unwrap_or(&0)
    }

    fn set_nonce(&mut self, api_key_index: i32, nonce: i64) {
        self.nonces.insert(api_key_index, nonce);
    }

    fn acknowledge_failure(&mut self, api_key_index: i32) {
        if let Some(nonce) = self.nonces.get_mut(&api_key_index) {
            *nonce = nonce.saturating_sub(1);
        }
    }

    fn switch_api_key(&mut self, new_api_key: i32) {
        self.current_api_key = new_api_key;
    }
}

/// Main SignerClient for interacting with Lighter protocol
pub struct SignerClient {
    library: Arc<Library>,
    url: String,
    private_keys: HashMap<i32, String>,
    chain_id: i32,
    api_key_index: i32,
    account_index: i64,
    start_api_key: i32,
    end_api_key: i32,
    nonce_manager: Arc<Mutex<NonceManager>>,
}

impl SignerClient {
    /// Create a new SignerClient
    pub fn new(
        url: &str,
        private_key: &str,
        api_key_index: i32,
        account_index: i64,
        max_api_key_index: Option<i32>,
        additional_private_keys: Option<HashMap<i32, String>>,
    ) -> Result<Self, TradingError> {
        let chain_id = if url.contains("mainnet") { 304 } else { 300 };
        
        let clean_key = private_key.trim_start_matches("0x");
        let lib_path = Self::get_library_path()?;
        
        let library = unsafe {
            Library::new(&lib_path)
                .map_err(|e| TradingError::SigningError(e.to_string()))?
        };

        let end_api_key = max_api_key_index.unwrap_or(api_key_index);
        
        // Build private keys map
        let mut private_keys = additional_private_keys.unwrap_or_default();
        private_keys.insert(api_key_index, clean_key.to_string());
        
        // Validate that we have all required keys
        if end_api_key > api_key_index {
            for key_idx in (api_key_index + 1)..=end_api_key {
                if !private_keys.contains_key(&key_idx) {
                    return Err(TradingError::InvalidInput(
                        format!("Missing private key for API key index {}", key_idx)
                    ));
                }
            }
        }

        let client = Self {
            library: Arc::new(library),
            url: url.to_string(),
            private_keys,
            chain_id,
            api_key_index,
            account_index,
            start_api_key: api_key_index,
            end_api_key,
            nonce_manager: Arc::new(Mutex::new(NonceManager::new(api_key_index))),
        };

        // Create client for each API key
        for api_key in api_key_index..=end_api_key {
            client.create_client_for_key(api_key)?;
        }

        Ok(client)
    }

    fn get_library_path() -> Result<PathBuf, TradingError> {
        let mut base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        
        let lighter_rust_path = base_path.join("lighter-rust");
        if lighter_rust_path.exists() {
            base_path = lighter_rust_path;
        }

        let lib_name = if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            "signer-arm64.dylib"
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            "signer-amd64.so"
        } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
            "signer-amd64.dll"
        } else {
            return Err(TradingError::SigningError(
                format!("Unsupported platform/architecture: {}/{}", 
                    std::env::consts::OS, 
                    std::env::consts::ARCH)
            ));
        };

        let lib_path = base_path.join("bin").join("signers").join(lib_name);
        
        if !lib_path.exists() {
            return Err(TradingError::SigningError(
                format!("FFI library not found at path: {}. Please ensure the library is built.", 
                    lib_path.display())
            ));
        }
        
        Ok(lib_path)
    }

    fn create_client_for_key(&self, api_key_index: i32) -> Result<(), TradingError> {
        unsafe {
            let create_client_fn: Symbol<
                unsafe extern "C" fn(*const c_char, *const c_char, c_int, c_int, c_longlong) -> *mut c_char,
            > = self
                .library
                .get(b"CreateClient")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let private_key = self.private_keys.get(&api_key_index)
                .ok_or_else(|| TradingError::InvalidInput(
                    format!("Private key not found for API key index {}", api_key_index)
                ))?;

            let c_url = CString::new(self.url.as_str())
                .map_err(|_| TradingError::SigningError("Invalid URL".to_string()))?;
            let c_key = CString::new(private_key.as_str())
                .map_err(|_| TradingError::SigningError("Invalid key".to_string()))?;

            let result = create_client_fn(
                c_url.as_ptr(),
                c_key.as_ptr(),
                self.chain_id,
                api_key_index,
                self.account_index,
            );

            if !result.is_null() {
                let error_str = CStr::from_ptr(result).to_string_lossy().to_string();
                libc::free(result as *mut libc::c_void);
                if !error_str.is_empty() {
                    return Err(TradingError::SigningError(
                        format!("CreateClient failed: {}", error_str)
                    ));
                }
            }

            Ok(())
        }
    }

    /// Check if the client's API key matches the one on Lighter
    pub fn check_client(&self) -> Result<(), TradingError> {
        unsafe {
            let check_fn: Symbol<
                unsafe extern "C" fn(c_int, c_longlong) -> *mut c_char,
            > = self
                .library
                .get(b"CheckClient")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            for api_key in self.start_api_key..=self.end_api_key {
                let result = check_fn(api_key, self.account_index);
                
                if !result.is_null() {
                    let error_str = CStr::from_ptr(result).to_string_lossy().to_string();
                    libc::free(result as *mut libc::c_void);
                    if !error_str.is_empty() {
                        return Err(TradingError::SigningError(
                            format!("Check failed for API key {}: {}", api_key, error_str)
                        ));
                    }
                }
            }

            Ok(())
        }
    }

    /// Switch to a different API key
    pub fn switch_api_key(&self, api_key: i32) -> Result<(), TradingError> {
        unsafe {
            let switch_fn: Symbol<unsafe extern "C" fn(c_int) -> *mut c_char> = self
                .library
                .get(b"SwitchAPIKey")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = switch_fn(api_key);

            if !result.is_null() {
                let error_str = CStr::from_ptr(result).to_string_lossy().to_string();
                libc::free(result as *mut libc::c_void);
                if !error_str.is_empty() {
                    return Err(TradingError::SigningError(error_str));
                }
            }

            self.nonce_manager.lock().unwrap().switch_api_key(api_key);
            Ok(())
        }
    }

    fn parse_result(&self, result: StrOrErr) -> Result<String, TradingError> {
        unsafe {
            if !result.err.is_null() {
                let error_str = CStr::from_ptr(result.err).to_string_lossy().to_string();
                libc::free(result.err as *mut libc::c_void);
                if !result.str.is_null() {
                    libc::free(result.str as *mut libc::c_void);
                }
                return Err(TradingError::SigningError(error_str));
            }

            if result.str.is_null() {
                return Err(TradingError::SigningError("Null result".to_string()));
            }

            let value_str = CStr::from_ptr(result.str).to_string_lossy().to_string();
            libc::free(result.str as *mut libc::c_void);

            Ok(value_str)
        }
    }

    /// Generate a new API key pair
    pub fn create_api_key(seed: &str) -> Result<(String, String), TradingError> {
        let lib_path = Self::get_library_path()?;
        let library = unsafe {
            Library::new(&lib_path)
                .map_err(|e| TradingError::SigningError(e.to_string()))?
        };

        unsafe {
            let gen_fn: Symbol<
                unsafe extern "C" fn(*const c_char) -> ApiKeyResponse,
            > = library
                .get(b"GenerateAPIKey")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let c_seed = CString::new(seed)
                .map_err(|_| TradingError::SigningError("Invalid seed".to_string()))?;

            let result = gen_fn(c_seed.as_ptr());

            let private_key = if !result.private_key.is_null() {
                let key = CStr::from_ptr(result.private_key).to_string_lossy().to_string();
                libc::free(result.private_key as *mut libc::c_void);
                key
            } else {
                String::new()
            };

            let public_key = if !result.public_key.is_null() {
                let key = CStr::from_ptr(result.public_key).to_string_lossy().to_string();
                libc::free(result.public_key as *mut libc::c_void);
                key
            } else {
                String::new()
            };

            if !result.err.is_null() {
                let error = CStr::from_ptr(result.err).to_string_lossy().to_string();
                libc::free(result.err as *mut libc::c_void);
                return Err(TradingError::SigningError(error));
            }

            Ok((private_key, public_key))
        }
    }

    /// Create an auth token with expiry
    pub fn create_auth_token(&self, deadline: Option<i64>) -> Result<String, TradingError> {
        let deadline = deadline.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                + 10 * MINUTE
        });

        unsafe {
            let auth_fn: Symbol<unsafe extern "C" fn(c_longlong) -> StrOrErr> = self
                .library
                .get(b"CreateAuthToken")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = auth_fn(deadline);
            self.parse_result(result)
        }
    }

    // ============ Signing Methods ============

    #[allow(clippy::too_many_arguments)]
    pub fn sign_create_order(
        &self,
        market_index: i32,
        client_order_index: i64,
        base_amount: i64,
        price: i64,
        is_ask: bool,
        order_type: i32,
        time_in_force: i32,
        reduce_only: bool,
        trigger_price: i64,
        order_expiry: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(
                    c_int,
                    c_longlong,
                    c_longlong,
                    c_longlong,
                    c_int,
                    c_int,
                    c_int,
                    c_int,
                    c_longlong,
                    c_longlong,
                    c_longlong,
                ) -> StrOrErr,
            > = self
                .library
                .get(b"SignCreateOrder")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(
                market_index,
                client_order_index,
                base_amount,
                price,
                if is_ask { 1 } else { 0 },
                order_type,
                time_in_force,
                if reduce_only { 1 } else { 0 },
                trigger_price,
                order_expiry,
                nonce,
            );

            self.parse_result(result)
        }
    }

    pub fn sign_cancel_order(
        &self,
        market_index: i32,
        order_index: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(c_int, c_longlong, c_longlong) -> StrOrErr,
            > = self
                .library
                .get(b"SignCancelOrder")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(market_index, order_index, nonce);
            self.parse_result(result)
        }
    }

    pub fn sign_withdraw(
        &self,
        usdc_amount: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(c_longlong, c_longlong) -> StrOrErr,
            > = self
                .library
                .get(b"SignWithdraw")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(usdc_amount, nonce);
            self.parse_result(result)
        }
    }

    pub fn sign_create_sub_account(&self, nonce: i64) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_longlong) -> StrOrErr> = self
                .library
                .get(b"SignCreateSubAccount")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(nonce);
            self.parse_result(result)
        }
    }

    pub fn sign_cancel_all_orders(
        &self,
        time_in_force: i32,
        time: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(c_int, c_longlong, c_longlong) -> StrOrErr,
            > = self
                .library
                .get(b"SignCancelAllOrders")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(time_in_force, time, nonce);
            self.parse_result(result)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sign_modify_order(
        &self,
        market_index: i32,
        order_index: i64,
        base_amount: i64,
        price: i64,
        trigger_price: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(
                    c_int,
                    c_longlong,
                    c_longlong,
                    c_longlong,
                    c_longlong,
                    c_longlong,
                ) -> StrOrErr,
            > = self
                .library
                .get(b"SignModifyOrder")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(
                market_index,
                order_index,
                base_amount,
                price,
                trigger_price,
                nonce,
            );
            self.parse_result(result)
        }
    }

    pub fn sign_transfer(
        &self,
        to_account_index: i64,
        usdc_amount: i64,
        fee: i64,
        memo: &str,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(
                    c_longlong,
                    c_longlong,
                    c_longlong,
                    *const c_char,
                    c_longlong,
                ) -> StrOrErr,
            > = self
                .library
                .get(b"SignTransfer")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let c_memo = CString::new(memo)
                .map_err(|_| TradingError::SigningError("Invalid memo".to_string()))?;

            let result = sign_fn(to_account_index, usdc_amount, fee, c_memo.as_ptr(), nonce);
            self.parse_result(result)
        }
    }

    pub fn sign_create_public_pool(
        &self,
        operator_fee: i64,
        initial_total_shares: i64,
        min_operator_share_rate: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(c_longlong, c_longlong, c_longlong, c_longlong) -> StrOrErr,
            > = self
                .library
                .get(b"SignCreatePublicPool")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(
                operator_fee,
                initial_total_shares,
                min_operator_share_rate,
                nonce,
            );
            self.parse_result(result)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sign_update_public_pool(
        &self,
        public_pool_index: i64,
        status: i32,
        operator_fee: i64,
        min_operator_share_rate: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(
                    c_longlong,
                    c_int,
                    c_longlong,
                    c_longlong,
                    c_longlong,
                ) -> StrOrErr,
            > = self
                .library
                .get(b"SignUpdatePublicPool")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(
                public_pool_index,
                status,
                operator_fee,
                min_operator_share_rate,
                nonce,
            );
            self.parse_result(result)
        }
    }

    pub fn sign_mint_shares(
        &self,
        public_pool_index: i64,
        share_amount: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(c_longlong, c_longlong, c_longlong) -> StrOrErr,
            > = self
                .library
                .get(b"SignMintShares")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(public_pool_index, share_amount, nonce);
            self.parse_result(result)
        }
    }

    pub fn sign_burn_shares(
        &self,
        public_pool_index: i64,
        share_amount: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(c_longlong, c_longlong, c_longlong) -> StrOrErr,
            > = self
                .library
                .get(b"SignBurnShares")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(public_pool_index, share_amount, nonce);
            self.parse_result(result)
        }
    }

    pub fn sign_update_leverage(
        &self,
        market_index: i32,
        fraction: i32,
        margin_mode: i32,
        nonce: i64,
    ) -> Result<String, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(c_int, c_int, c_int, c_longlong) -> StrOrErr,
            > = self
                .library
                .get(b"SignUpdateLeverage")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let result = sign_fn(market_index, fraction, margin_mode, nonce);
            self.parse_result(result)
        }
    }

    /// Sign a change API key transaction (requires Ethereum signature)
    /// Returns the transaction info with MessageToSign that needs to be signed by Ethereum wallet
    pub fn sign_change_api_key(
        &self,
        new_pubkey: &str,
        nonce: i64,
    ) -> Result<TransactionInfo, TradingError> {
        unsafe {
            let sign_fn: Symbol<
                unsafe extern "C" fn(*const c_char, c_longlong) -> StrOrErr,
            > = self
                .library
                .get(b"SignChangePubKey")
                .map_err(|e| TradingError::SigningError(e.to_string()))?;

            let c_pubkey = CString::new(new_pubkey)
                .map_err(|_| TradingError::SigningError("Invalid pubkey".to_string()))?;

            let result = sign_fn(c_pubkey.as_ptr(), nonce);
            let tx_info_str = self.parse_result(result)?;
            
            // Parse the transaction info
            let tx_info: TransactionInfo = serde_json::from_str(&tx_info_str)
                .map_err(|e| TradingError::SigningError(format!("Failed to parse tx info: {}", e)))?;
            
            Ok(tx_info)
        }
    }

    // ============ Helper Methods ============

    /// Get next nonce and API key
    pub fn get_next_nonce(&self) -> (i32, i64) {
        self.nonce_manager.lock().unwrap().next_nonce()
    }

    /// Get current nonce for a specific API key
    pub fn get_nonce(&self, api_key_index: i32) -> i64 {
        self.nonce_manager.lock().unwrap().get_nonce(api_key_index)
    }

    /// Set nonce for a specific API key
    pub fn set_nonce(&self, api_key_index: i32, nonce: i64) {
        self.nonce_manager.lock().unwrap().set_nonce(api_key_index, nonce);
    }

    /// Acknowledge transaction failure (rollback nonce)
    pub fn acknowledge_failure(&self, api_key_index: i32) {
        self.nonce_manager.lock().unwrap().acknowledge_failure(api_key_index);
    }

    /// Compare two private keys (handles 0x prefix)
    pub fn are_keys_equal(key1: &str, key2: &str) -> bool {
        let k1 = key1.trim_start_matches("0x");
        let k2 = key2.trim_start_matches("0x");
        k1 == k2
    }
}

// Thread-safe clone implementation
impl Clone for SignerClient {
    fn clone(&self) -> Self {
        Self {
            library: Arc::clone(&self.library),
            url: self.url.clone(),
            private_keys: self.private_keys.clone(),
            chain_id: self.chain_id,
            api_key_index: self.api_key_index,
            account_index: self.account_index,
            start_api_key: self.start_api_key,
            end_api_key: self.end_api_key,
            nonce_manager: Arc::clone(&self.nonce_manager),
        }
    }
}
