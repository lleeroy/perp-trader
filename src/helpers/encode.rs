#![allow(unused)]
#![allow(deprecated)]

use anyhow::{anyhow, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use rand_core::{OsRng, RngCore};
use zeroize::Zeroize;

// Format versioning for future-proofing
const ENCODING_VERSION: u8 = 1; // bump if layout changes

// Payload layout (all concatenated then base64-no-pad):
// [1B version][16B argon2 salt][12B aes-gcm nonce][var ciphertext+tag]

/// Derives a 32-byte encryption key from a password using Argon2id.
/// 
/// # Arguments
///
/// * `password` - The password from which to derive the encryption key.
/// * `salt` - The salt value used in the Argon2 algorithm.
///
/// # Returns
/// 
/// * `Result<[u8; 32]>` - The derived 32-byte encryption key.
/// 
/// # Errors
///
/// Returns an error if Argon2id key derivation fails.
fn derive_key_from_password(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
	// Argon2id with reasonable interactive params; tune as needed
	let params = Params::new(19_456, 2, 1, Some(32))?; // ~19MB mem, 2 iters, 1 lane
	let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
	let mut derived = [0u8; 32];
	argon2.hash_password_into(password.as_bytes(), salt, &mut derived)?;
	Ok(derived)
}

/// Encrypts an EVM private key with a password using Argon2id key derivation and AES-256-GCM.
///
/// The output is a base64-no-padding encoded string containing version, salt, nonce, and ciphertext.
/// 
/// # Arguments
///
/// * `private_key_hex` - The EVM private key as a hex string (with or without "0x" prefix), 32 bytes required.
/// * `password` - The password used for key encryption.
///
/// # Returns
///
/// * `Result<String>` - The base64 encoded, protected ciphertext containing all cryptographic metadata.
///
/// # Errors
///
/// Returns an error if the input key is invalid, encryption fails, or other cryptographic errors occur.
pub fn encrypt_private_key(private_key_hex: &str, password: &str) -> Result<String> {
	// Normalize hex: allow optional 0x prefix
	let pk_hex = private_key_hex.strip_prefix("0x").unwrap_or(private_key_hex);
	let mut private_key_bytes = hex::decode(pk_hex)
		.map_err(|e| anyhow!("invalid private key hex: {e}"))?;
	if private_key_bytes.len() != 32 {
		return Err(anyhow!("EVM private key must be 32 bytes"));
	}

	let mut salt = [0u8; 16];
	OsRng.fill_bytes(&mut salt);
	let key = derive_key_from_password(password, &salt)?;
	let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow!("cipher init failed"))?;

	// 96-bit nonce for AES-GCM
	let mut nonce_bytes = [0u8; 12];
	rand_core::RngCore::fill_bytes(&mut OsRng, &mut nonce_bytes);
	let nonce = Nonce::from(nonce_bytes);

	let ciphertext = cipher
		.encrypt(&nonce, private_key_bytes.as_slice())
		.map_err(|_| anyhow!("encryption failed"))?;

	// zeroize sensitive in-memory data ASAP
	private_key_bytes.zeroize();
	// key is stack array; ensure it's zeroized on drop by shadowing and zeroizing
	{
		let mut key_mut = key;
		key_mut.zeroize();
	}

	let mut payload = Vec::with_capacity(1 + 16 + 12 + ciphertext.len());
	payload.push(ENCODING_VERSION);
	payload.extend_from_slice(&salt); // 16 bytes
	payload.extend_from_slice(&nonce_bytes); // 12 bytes
	payload.extend_from_slice(&ciphertext);

	Ok(STANDARD_NO_PAD.encode(payload))
}

/// Decrypts a password-protected private key, previously encrypted by `encrypt_private_key`.
///
/// This expects a base64-no-padding encoding containing the version, salt, nonce, and ciphertext.
/// 
/// # Arguments
///
/// * `encoded` - The base64-no-padding encrypted private key string.
/// * `password` - The password for decryption.
///
/// # Returns
///
/// * `Result<String>` - The decrypted EVM private key as a hex string prefixed with "0x".
///
/// # Errors
///
/// Returns an error if decoding, decryption, authentication, or password validation fails.
pub fn decrypt_private_key(encoded: &str, password: &str) -> Result<String> {
	let payload = STANDARD_NO_PAD
		.decode(encoded)
		.map_err(|e| anyhow!("base64 decode failed: {e}"))?;
	if payload.len() < 1 + 16 + 12 + 16 {
		return Err(anyhow!("payload too short"));
	}
	let version = payload[0];
	if version != ENCODING_VERSION {
		return Err(anyhow!("unsupported version: {version}"));
	}
	let salt = &payload[1..1 + 16];
	let nonce_bytes = &payload[1 + 16..1 + 16 + 12];
	let ciphertext = &payload[1 + 16 + 12..];

	let key = derive_key_from_password(password, salt)?;
	let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow!("cipher init failed"))?;
	let nonce = Nonce::from_slice(nonce_bytes);
	let mut plaintext = cipher
		.decrypt(&nonce, ciphertext)
		.map_err(|_| anyhow!("decryption failed (bad password or corrupted data)"))?;

	if plaintext.len() != 32 {
		return Err(anyhow!("unexpected key length after decrypt"));
	}
	let hex_str = format!("0x{}", hex::encode(plaintext.as_slice()));
	plaintext.zeroize();
	{
		let mut key_mut = key;
		key_mut.zeroize();
	}
	Ok(hex_str)
}

