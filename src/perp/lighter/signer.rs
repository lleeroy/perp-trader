#![allow(dead_code)]

//! # SignerClient - Rust implementation of the Lighter Protocol Signer
//!
//! This module provides a comprehensive Rust implementation of the Python SignerClient,
//! offering FFI bindings to the Go-based signing library for the Lighter Protocol.
//!
//! ## Features
//!
//! - **FFI Integration**: Seamless integration with Go signing library via FFI
//! - **Thread-Safe**: All FFI calls are serialized through a dedicated worker thread
//! - **Multi-API Key Support**: Manage multiple API keys with automatic nonce tracking
//! - **Transaction Signing**: Support for all Lighter transaction types
//! - **Platform Support**: macOS (ARM64), Linux (x86_64), Windows (x86_64)

use crate::error::TradingError;
use libloading::{Library, Symbol};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_longlong};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

// Transaction Types
pub const TX_TYPE_CHANGE_PUB_KEY: i32 = 8;
pub const TX_TYPE_CREATE_SUB_ACCOUNT: i32 = 9;
pub const TX_TYPE_CREATE_PUBLIC_POOL: i32 = 10;
pub const TX_TYPE_UPDATE_PUBLIC_POOL: i32 = 11;
pub const TX_TYPE_TRANSFER: i32 = 12;
pub const TX_TYPE_WITHDRAW: i32 = 13;
pub const TX_TYPE_CREATE_ORDER: i32 = 14;
pub const TX_TYPE_CANCEL_ORDER: i32 = 15;
pub const TX_TYPE_CANCEL_ALL_ORDERS: i32 = 16;
pub const TX_TYPE_MODIFY_ORDER: i32 = 17;
pub const TX_TYPE_MINT_SHARES: i32 = 18;
pub const TX_TYPE_BURN_SHARES: i32 = 19;
pub const TX_TYPE_UPDATE_LEVERAGE: i32 = 20;

// Order Types
pub const ORDER_TYPE_LIMIT: i32 = 0;
pub const ORDER_TYPE_MARKET: i32 = 1;
pub const ORDER_TYPE_STOP_LOSS: i32 = 2;
pub const ORDER_TYPE_STOP_LOSS_LIMIT: i32 = 3;
pub const ORDER_TYPE_TAKE_PROFIT: i32 = 4;
pub const ORDER_TYPE_TAKE_PROFIT_LIMIT: i32 = 5;
pub const ORDER_TYPE_TWAP: i32 = 6;

// Time in Force
pub const ORDER_TIME_IN_FORCE_IMMEDIATE_OR_CANCEL: i32 = 0;
pub const ORDER_TIME_IN_FORCE_GOOD_TILL_TIME: i32 = 1;
pub const ORDER_TIME_IN_FORCE_POST_ONLY: i32 = 2;

// Cancel All TIF
pub const CANCEL_ALL_TIF_IMMEDIATE: i32 = 0;
pub const CANCEL_ALL_TIF_SCHEDULED: i32 = 1;
pub const CANCEL_ALL_TIF_ABORT: i32 = 2;

// Margin Modes
pub const CROSS_MARGIN_MODE: i32 = 0;
pub const ISOLATED_MARGIN_MODE: i32 = 1;

// Defaults
pub const NIL_TRIGGER_PRICE: i32 = 0;
pub const DEFAULT_28_DAY_ORDER_EXPIRY: i64 = -1;
pub const DEFAULT_IOC_EXPIRY: i64 = 0;
pub const DEFAULT_10_MIN_AUTH_EXPIRY: i64 = -1;
pub const MINUTE: i64 = 60;
pub const USDC_TICKER_SCALE: i64 = 1_000_000;
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

// ============================================================================
// FFI Command Types
// ============================================================================

enum FfiCommand {
    CreateClient {
        url: String,
        private_key: String,
        chain_id: i32,
        api_key_index: i32,
        account_index: i64,
        response: Sender<Result<(), String>>,
    },
    GenerateApiKey {
        seed: String,
        response: Sender<Result<(String, String), String>>,
    },
    SignCreateOrder {
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
        response: Sender<Result<String, String>>,
    },
    SignCancelOrder {
        market_index: i32,
        order_index: i64,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignWithdraw {
        usdc_amount: i64,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignCreateSubAccount {
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignCancelAllOrders {
        time_in_force: i32,
        time: i64,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignModifyOrder {
        market_index: i32,
        order_index: i64,
        base_amount: i64,
        price: i64,
        trigger_price: i64,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignTransfer {
        to_account_index: i64,
        usdc_amount: i64,
        fee: i64,
        memo: String,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignCreatePublicPool {
        operator_fee: i64,
        initial_total_shares: i64,
        min_operator_share_rate: i64,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignUpdatePublicPool {
        public_pool_index: i64,
        status: i32,
        operator_fee: i64,
        min_operator_share_rate: i64,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignMintShares {
        public_pool_index: i64,
        share_amount: i64,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignBurnShares {
        public_pool_index: i64,
        share_amount: i64,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignUpdateLeverage {
        market_index: i32,
        fraction: i32,
        margin_mode: i32,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
    SignChangeApiKey {
        new_pubkey: String,
        nonce: i64,
        response: Sender<Result<String, String>>,
    },
}

// ============================================================================
// FFI Worker - Dedicated thread for all Go library interactions
// ============================================================================

struct FfiWorker {
    library: Library,
    command_rx: Receiver<FfiCommand>,
}

impl FfiWorker {
    fn new(library: Library, command_rx: Receiver<FfiCommand>) -> Self {
        Self { library, command_rx }
    }

    fn run(self) {
        loop {
            match self.command_rx.recv() {
                Ok(cmd) => self.handle_command(cmd),
                Err(_) => {
                    // Channel closed, exit thread
                    break;
                }
            }
        }
    }

    fn handle_command(&self, cmd: FfiCommand) {
        match cmd {
            FfiCommand::CreateClient { url, private_key, chain_id, api_key_index, account_index, response } => {
                let result = self.create_client_impl(&url, &private_key, chain_id, api_key_index, account_index);
                let _ = response.send(result);
            }
            FfiCommand::GenerateApiKey { seed, response } => {
                let result = self.generate_api_key_impl(&seed);
                let _ = response.send(result);
            }
            FfiCommand::SignCreateOrder { market_index, client_order_index, base_amount, price, is_ask, order_type, time_in_force, reduce_only, trigger_price, order_expiry, nonce, response } => {
                let result = self.sign_create_order_impl(market_index, client_order_index, base_amount, price, is_ask, order_type, time_in_force, reduce_only, trigger_price, order_expiry, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignCancelOrder { market_index, order_index, nonce, response } => {
                let result = self.sign_cancel_order_impl(market_index, order_index, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignWithdraw { usdc_amount, nonce, response } => {
                let result = self.sign_withdraw_impl(usdc_amount, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignCreateSubAccount { nonce, response } => {
                let result = self.sign_create_sub_account_impl(nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignCancelAllOrders { time_in_force, time, nonce, response } => {
                let result = self.sign_cancel_all_orders_impl(time_in_force, time, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignModifyOrder { market_index, order_index, base_amount, price, trigger_price, nonce, response } => {
                let result = self.sign_modify_order_impl(market_index, order_index, base_amount, price, trigger_price, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignTransfer { to_account_index, usdc_amount, fee, memo, nonce, response } => {
                let result = self.sign_transfer_impl(to_account_index, usdc_amount, fee, &memo, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignCreatePublicPool { operator_fee, initial_total_shares, min_operator_share_rate, nonce, response } => {
                let result = self.sign_create_public_pool_impl(operator_fee, initial_total_shares, min_operator_share_rate, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignUpdatePublicPool { public_pool_index, status, operator_fee, min_operator_share_rate, nonce, response } => {
                let result = self.sign_update_public_pool_impl(public_pool_index, status, operator_fee, min_operator_share_rate, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignMintShares { public_pool_index, share_amount, nonce, response } => {
                let result = self.sign_mint_shares_impl(public_pool_index, share_amount, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignBurnShares { public_pool_index, share_amount, nonce, response } => {
                let result = self.sign_burn_shares_impl(public_pool_index, share_amount, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignUpdateLeverage { market_index, fraction, margin_mode, nonce, response } => {
                let result = self.sign_update_leverage_impl(market_index, fraction, margin_mode, nonce);
                let _ = response.send(result);
            }
            FfiCommand::SignChangeApiKey { new_pubkey, nonce, response } => {
                let result = self.sign_change_api_key_impl(&new_pubkey, nonce);
                let _ = response.send(result);
            }
        }
    }

    // Implementation methods
    fn create_client_impl(&self, url: &str, private_key: &str, chain_id: i32, api_key_index: i32, account_index: i64) -> Result<(), String> {
        unsafe {
            let create_client_fn: Symbol<unsafe extern "C" fn(*const c_char, *const c_char, c_int, c_int, c_longlong) -> *mut c_char> =
                self.library.get(b"CreateClient").map_err(|e| e.to_string())?;

            let c_url = CString::new(url).map_err(|_| "Invalid URL".to_string())?;
            let c_key = CString::new(private_key).map_err(|_| "Invalid key".to_string())?;

            let result = create_client_fn(c_url.as_ptr(), c_key.as_ptr(), chain_id, api_key_index, account_index);

            if !result.is_null() {
                let error_str = CStr::from_ptr(result).to_string_lossy().to_string();
                libc::free(result as *mut libc::c_void);
                if !error_str.is_empty() {
                    return Err(format!("CreateClient failed: {}", error_str));
                }
            }

            Ok(())
        }
    }

    fn generate_api_key_impl(&self, seed: &str) -> Result<(String, String), String> {
        unsafe {
            let gen_fn: Symbol<unsafe extern "C" fn(*const c_char) -> ApiKeyResponse> =
                self.library.get(b"GenerateAPIKey").map_err(|e| e.to_string())?;

            let c_seed = CString::new(seed).map_err(|_| "Invalid seed".to_string())?;
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
                return Err(error);
            }

            Ok((private_key, public_key))
        }
    }

    fn sign_create_order_impl(&self, market_index: i32, client_order_index: i64, base_amount: i64, price: i64, is_ask: bool, order_type: i32, time_in_force: i32, reduce_only: bool, trigger_price: i64, order_expiry: i64, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_int, c_longlong, c_longlong, c_longlong, c_int, c_int, c_int, c_int, c_longlong, c_longlong, c_longlong) -> StrOrErr> =
                self.library.get(b"SignCreateOrder").map_err(|e| e.to_string())?;

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

    fn sign_cancel_order_impl(&self, market_index: i32, order_index: i64, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_int, c_longlong, c_longlong) -> StrOrErr> =
                self.library.get(b"SignCancelOrder").map_err(|e| e.to_string())?;

            let result = sign_fn(market_index, order_index, nonce);
            self.parse_result(result)
        }
    }

    fn sign_withdraw_impl(&self, usdc_amount: i64, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_longlong, c_longlong) -> StrOrErr> =
                self.library.get(b"SignWithdraw").map_err(|e| e.to_string())?;

            let result = sign_fn(usdc_amount, nonce);
            self.parse_result(result)
        }
    }

    fn sign_create_sub_account_impl(&self, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_longlong) -> StrOrErr> =
                self.library.get(b"SignCreateSubAccount").map_err(|e| e.to_string())?;

            let result = sign_fn(nonce);
            self.parse_result(result)
        }
    }

    fn sign_cancel_all_orders_impl(&self, time_in_force: i32, time: i64, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_int, c_longlong, c_longlong) -> StrOrErr> =
                self.library.get(b"SignCancelAllOrders").map_err(|e| e.to_string())?;

            let result = sign_fn(time_in_force, time, nonce);
            self.parse_result(result)
        }
    }

    fn sign_modify_order_impl(&self, market_index: i32, order_index: i64, base_amount: i64, price: i64, trigger_price: i64, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_int, c_longlong, c_longlong, c_longlong, c_longlong, c_longlong) -> StrOrErr> =
                self.library.get(b"SignModifyOrder").map_err(|e| e.to_string())?;

            let result = sign_fn(market_index, order_index, base_amount, price, trigger_price, nonce);
            self.parse_result(result)
        }
    }

    fn sign_transfer_impl(&self, to_account_index: i64, usdc_amount: i64, fee: i64, memo: &str, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_longlong, c_longlong, c_longlong, *const c_char, c_longlong) -> StrOrErr> =
                self.library.get(b"SignTransfer").map_err(|e| e.to_string())?;

            let c_memo = CString::new(memo).map_err(|_| "Invalid memo".to_string())?;
            let result = sign_fn(to_account_index, usdc_amount, fee, c_memo.as_ptr(), nonce);
            self.parse_result(result)
        }
    }

    fn sign_create_public_pool_impl(&self, operator_fee: i64, initial_total_shares: i64, min_operator_share_rate: i64, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_longlong, c_longlong, c_longlong, c_longlong) -> StrOrErr> =
                self.library.get(b"SignCreatePublicPool").map_err(|e| e.to_string())?;

            let result = sign_fn(operator_fee, initial_total_shares, min_operator_share_rate, nonce);
            self.parse_result(result)
        }
    }

    fn sign_update_public_pool_impl(&self, public_pool_index: i64, status: i32, operator_fee: i64, min_operator_share_rate: i64, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_longlong, c_int, c_longlong, c_longlong, c_longlong) -> StrOrErr> =
                self.library.get(b"SignUpdatePublicPool").map_err(|e| e.to_string())?;

            let result = sign_fn(public_pool_index, status, operator_fee, min_operator_share_rate, nonce);
            self.parse_result(result)
        }
    }

    fn sign_mint_shares_impl(&self, public_pool_index: i64, share_amount: i64, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_longlong, c_longlong, c_longlong) -> StrOrErr> =
                self.library.get(b"SignMintShares").map_err(|e| e.to_string())?;

            let result = sign_fn(public_pool_index, share_amount, nonce);
            self.parse_result(result)
        }
    }

    fn sign_burn_shares_impl(&self, public_pool_index: i64, share_amount: i64, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_longlong, c_longlong, c_longlong) -> StrOrErr> =
                self.library.get(b"SignBurnShares").map_err(|e| e.to_string())?;

            let result = sign_fn(public_pool_index, share_amount, nonce);
            self.parse_result(result)
        }
    }

    fn sign_update_leverage_impl(&self, market_index: i32, fraction: i32, margin_mode: i32, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(c_int, c_int, c_int, c_longlong) -> StrOrErr> =
                self.library.get(b"SignUpdateLeverage").map_err(|e| e.to_string())?;

            let result = sign_fn(market_index, fraction, margin_mode, nonce);
            self.parse_result(result)
        }
    }

    fn sign_change_api_key_impl(&self, new_pubkey: &str, nonce: i64) -> Result<String, String> {
        unsafe {
            let sign_fn: Symbol<unsafe extern "C" fn(*const c_char, c_longlong) -> StrOrErr> =
                self.library.get(b"SignChangePubKey").map_err(|e| e.to_string())?;

            let c_pubkey = CString::new(new_pubkey).map_err(|_| "Invalid pubkey".to_string())?;
            let result = sign_fn(c_pubkey.as_ptr(), nonce);
            self.parse_result(result)
        }
    }

    fn parse_result(&self, result: StrOrErr) -> Result<String, String> {
        unsafe {
            if !result.err.is_null() {
                let error_str = CStr::from_ptr(result.err).to_string_lossy().to_string();
                libc::free(result.err as *mut libc::c_void);
                if !result.str.is_null() {
                    libc::free(result.str as *mut libc::c_void);
                }
                return Err(error_str);
            }

            if result.str.is_null() {
                return Err("Null result".to_string());
            }

            let value_str = CStr::from_ptr(result.str).to_string_lossy().to_string();
            libc::free(result.str as *mut libc::c_void);

            Ok(value_str)
        }
    }
}

// ============================================================================
// Global FFI Worker Thread
// ============================================================================

static FFI_WORKER: Lazy<Sender<FfiCommand>> = Lazy::new(|| {
    let lib_path = get_library_path().expect("Failed to determine signer library path");
    let library = unsafe {
        Library::new(&lib_path).expect(&format!("Failed to load signer library at: {}", lib_path.display()))
    };

    let (tx, rx) = channel();

    thread::Builder::new()
        .name("ffi-worker".to_string())
        .spawn(move || {
            let worker = FfiWorker::new(library, rx);
            worker.run();
        })
        .expect("Failed to spawn FFI worker thread");

    tx
});

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
        return Err(TradingError::SigningError(format!(
            "Unsupported platform/architecture: {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
    };

    let lib_path = base_path.join("bin").join("signers").join(lib_name);

    if !lib_path.exists() {
        return Err(TradingError::SigningError(format!(
            "FFI library not found at path: {}. Please ensure the library is built.",
            lib_path.display()
        )));
    }

    Ok(lib_path)
}

// ============================================================================
// Nonce Manager
// ============================================================================

#[derive(Debug, Clone)]
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

// ============================================================================
// SignerClient
// ============================================================================

#[derive(Debug)]
pub struct SignerClient {
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
        let end_api_key = max_api_key_index.unwrap_or(api_key_index);

        // Build private keys map
        let mut private_keys = additional_private_keys.unwrap_or_default();
        private_keys.insert(api_key_index, clean_key.to_string());

        // Validate that we have all required keys
        if end_api_key > api_key_index {
            for key_idx in (api_key_index + 1)..=end_api_key {
                if !private_keys.contains_key(&key_idx) {
                    return Err(TradingError::InvalidInput(format!(
                        "Missing private key for API key index {}",
                        key_idx
                    )));
                }
            }
        }

        let client = Self {
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

    fn create_client_for_key(&self, api_key_index: i32) -> Result<(), TradingError> {
        let private_key = self
            .private_keys
            .get(&api_key_index)
            .ok_or_else(|| {
                TradingError::InvalidInput(format!(
                    "Private key not found for API key index {}",
                    api_key_index
                ))
            })?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::CreateClient {
                url: self.url.clone(),
                private_key: private_key.clone(),
                chain_id: self.chain_id,
                api_key_index,
                account_index: self.account_index,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    /// Generate a new API key pair
    pub fn create_api_key(seed: &str) -> Result<(String, String), TradingError> {
        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::GenerateApiKey {
                seed: seed.to_string(),
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

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
        // Ensure correct client state before signing
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignCreateOrder {
                market_index,
                client_order_index,
                base_amount,
                price,
                is_ask,
                order_type,
                time_in_force,
                reduce_only,
                trigger_price,
                order_expiry,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_cancel_order(
        &self,
        market_index: i32,
        order_index: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignCancelOrder {
                market_index,
                order_index,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_withdraw(&self, usdc_amount: i64, nonce: i64) -> Result<String, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignWithdraw {
                usdc_amount,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_create_sub_account(&self, nonce: i64) -> Result<String, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignCreateSubAccount {
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_cancel_all_orders(
        &self,
        time_in_force: i32,
        time: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignCancelAllOrders {
                time_in_force,
                time,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
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
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignModifyOrder {
                market_index,
                order_index,
                base_amount,
                price,
                trigger_price,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_transfer(
        &self,
        to_account_index: i64,
        usdc_amount: i64,
        fee: i64,
        memo: &str,
        nonce: i64,
    ) -> Result<String, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignTransfer {
                to_account_index,
                usdc_amount,
                fee,
                memo: memo.to_string(),
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_create_public_pool(
        &self,
        operator_fee: i64,
        initial_total_shares: i64,
        min_operator_share_rate: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignCreatePublicPool {
                operator_fee,
                initial_total_shares,
                min_operator_share_rate,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
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
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignUpdatePublicPool {
                public_pool_index,
                status,
                operator_fee,
                min_operator_share_rate,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_mint_shares(
        &self,
        public_pool_index: i64,
        share_amount: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignMintShares {
                public_pool_index,
                share_amount,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_burn_shares(
        &self,
        public_pool_index: i64,
        share_amount: i64,
        nonce: i64,
    ) -> Result<String, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignBurnShares {
                public_pool_index,
                share_amount,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_update_leverage(
        &self,
        market_index: i32,
        fraction: i32,
        margin_mode: i32,
        nonce: i64,
    ) -> Result<String, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignUpdateLeverage {
                market_index,
                fraction,
                margin_mode,
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        rx.recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))
    }

    pub fn sign_change_api_key(
        &self,
        new_pubkey: &str,
        nonce: i64,
    ) -> Result<TransactionInfo, TradingError> {
        self.create_client_for_key(self.api_key_index)?;

        let (tx, rx) = channel();

        FFI_WORKER
            .send(FfiCommand::SignChangeApiKey {
                new_pubkey: new_pubkey.to_string(),
                nonce,
                response: tx,
            })
            .map_err(|e| TradingError::SigningError(format!("Failed to send command: {}", e)))?;

        let tx_info_str = rx
            .recv()
            .map_err(|e| TradingError::SigningError(format!("Failed to receive response: {}", e)))?
            .map_err(|e| TradingError::SigningError(e))?;

        let tx_info: TransactionInfo = serde_json::from_str(&tx_info_str)
            .map_err(|e| TradingError::SigningError(format!("Failed to parse tx info: {}", e)))?;

        Ok(tx_info)
    }

    pub fn switch_api_key(&self, api_key: i32) -> Result<(), TradingError> {
        self.nonce_manager.lock().unwrap().switch_api_key(api_key);
        Ok(())
    }

    pub fn get_next_nonce(&self) -> (i32, i64) {
        self.nonce_manager.lock().unwrap().next_nonce()
    }

    pub fn get_nonce(&self, api_key_index: i32) -> i64 {
        self.nonce_manager.lock().unwrap().get_nonce(api_key_index)
    }

    pub fn set_nonce(&self, api_key_index: i32, nonce: i64) {
        self.nonce_manager
            .lock()
            .unwrap()
            .set_nonce(api_key_index, nonce);
    }

    pub fn acknowledge_failure(&self, api_key_index: i32) {
        self.nonce_manager
            .lock()
            .unwrap()
            .acknowledge_failure(api_key_index);
    }

    pub fn are_keys_equal(key1: &str, key2: &str) -> bool {
        let k1 = key1.trim_start_matches("0x");
        let k2 = key2.trim_start_matches("0x");
        k1 == k2
    }
}

impl Clone for SignerClient {
    fn clone(&self) -> Self {
        Self {
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