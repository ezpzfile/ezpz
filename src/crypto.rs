//! Password-based encryption: Argon2id -> BLAKE3 derive_key -> XChaCha20-Poly1305 (SPEC.md §12).

use crate::format::*;
use anyhow::{Result, anyhow, ensure};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

pub const CATALOG_COUNTER: u64 = u64::MAX;
pub const KEY_CONTEXT: &str = "ezpz v1 data encryption key";

pub const DEFAULT_M_KIB: u32 = 64 * 1024;
pub const DEFAULT_T: u32 = 3;
pub const DEFAULT_P: u32 = 1;

pub fn derive_key(password: &[u8], e: &Encryption) -> Result<[u8; 32]> {
    ensure!(
        e.cipher == CIPHER_XCHACHA20POLY1305,
        "unsupported cipher {}",
        e.cipher
    );
    ensure!(
        e.kdf == KDF_ARGON2ID,
        "unsupported key derivation {}",
        e.kdf
    );
    ensure!(
        (1..=16).contains(&e.p_lanes)
            && (1..=64).contains(&e.t_cost)
            && e.m_kib >= 8 * e.p_lanes
            && e.m_kib <= 4 * 1024 * 1024,
        "key-derivation parameters outside the allowed range"
    );
    let params =
        argon2::Params::new(e.m_kib, e.t_cost, e.p_lanes, Some(32)).map_err(|x| anyhow!("{x}"))?;
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut master = [0u8; 32];
    a.hash_password_into(password, &e.salt, &mut master)
        .map_err(|x| anyhow!("{x}"))?;
    Ok(blake3::derive_key(KEY_CONTEXT, &master))
}

fn nonce(archive_id: &[u8; 16], counter: u64) -> [u8; 24] {
    let mut n = [0u8; 24];
    n[..16].copy_from_slice(archive_id);
    n[16..].copy_from_slice(&counter.to_le_bytes());
    n
}

pub fn seal(
    key: &[u8; 32],
    archive_id: &[u8; 16],
    counter: u64,
    aad: &[u8],
    pt: &[u8],
) -> Result<Vec<u8>> {
    let c = XChaCha20Poly1305::new(key.into());
    let n = nonce(archive_id, counter);
    c.encrypt(XNonce::from_slice(&n), Payload { msg: pt, aad })
        .map_err(|_| anyhow!("encryption failed"))
}

pub fn open(
    key: &[u8; 32],
    archive_id: &[u8; 16],
    counter: u64,
    aad: &[u8],
    ct: &[u8],
) -> Result<Vec<u8>> {
    let c = XChaCha20Poly1305::new(key.into());
    let n = nonce(archive_id, counter);
    c.decrypt(XNonce::from_slice(&n), Payload { msg: ct, aad })
        .map_err(|_| anyhow!("decryption failed - wrong password or damaged data"))
}
