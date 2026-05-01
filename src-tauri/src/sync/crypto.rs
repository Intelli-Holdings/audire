//! Crypto primitives for cloud sync.
//!
//! - **KEK** (key-encryption key): 32 bytes derived from the user's
//!   passphrase via Argon2id. Held only in RAM.
//! - **Identity keypair**: long-lived X25519 keypair. The private key is
//!   stored on disk *wrapped* with the KEK; the public key is uploaded
//!   so other users can wrap vault keys for this account.
//! - **Vault key**: 32-byte symmetric key (XChaCha20-Poly1305) created
//!   when a vault is made. Wrapped per-member using a sealed-box-style
//!   construction over the recipient's X25519 public key.
//! - **Recovery envelope**: a one-time recovery key wraps a copy of the
//!   KEK. If the user forgets the passphrase, they can use the recovery
//!   key to derive the KEK back.

use anyhow::{anyhow, Context};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use ring::rand::{SecureRandom, SystemRandom};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

/// Argon2id parameters tuned for ~250 ms on a desktop CPU. The salt is
/// stored alongside the wrapped material so re-derivation is
/// deterministic.
const ARGON2_M_COST: u32 = 64 * 1024; // 64 MiB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 1;

/// 32-byte symmetric key derived from a passphrase. Drops zero the
/// memory on `Drop`.
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct KekMaterial(pub [u8; 32]);

/// 32-byte symmetric vault key. Zeroes on drop.
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct VaultKey(pub [u8; 32]);

pub fn random_bytes(n: usize) -> anyhow::Result<Vec<u8>> {
    let rng = SystemRandom::new();
    let mut v = vec![0u8; n];
    rng.fill(&mut v)
        .map_err(|_| anyhow!("system rng fill failed"))?;
    Ok(v)
}

pub fn random_array<const N: usize>() -> anyhow::Result<[u8; N]> {
    let rng = SystemRandom::new();
    let mut v = [0u8; N];
    rng.fill(&mut v)
        .map_err(|_| anyhow!("system rng fill failed"))?;
    Ok(v)
}

/// Derive the KEK from a passphrase + salt with Argon2id.
pub fn derive_kek(passphrase: &str, salt: &[u8]) -> anyhow::Result<KekMaterial> {
    let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
        .map_err(|e| anyhow!("argon2 params: {e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|e| anyhow!("argon2 derive: {e}"))?;
    Ok(KekMaterial(out))
}

/// XChaCha20-Poly1305 encrypt with a fresh random nonce. Returns
/// `nonce || ciphertext`. The 24-byte nonce is large enough that random
/// generation is collision-safe.
pub fn aead_seal(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> anyhow::Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce_bytes: [u8; 24] = random_array()?;
    let nonce = XNonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(
            nonce,
            chacha20poly1305::aead::Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|e| anyhow!("aead seal failed: {e}"))?;
    let mut out = Vec::with_capacity(24 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Inverse of `aead_seal` — input must be `nonce || ciphertext`.
pub fn aead_open(key: &[u8; 32], blob: &[u8], aad: &[u8]) -> anyhow::Result<Vec<u8>> {
    if blob.len() < 24 + 16 {
        return Err(anyhow!("aead blob too short"));
    }
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XNonce::from_slice(&blob[..24]);
    cipher
        .decrypt(
            nonce,
            chacha20poly1305::aead::Payload {
                msg: &blob[24..],
                aad,
            },
        )
        .map_err(|e| anyhow!("aead open failed: {e}"))
}

/// Generate a new long-lived X25519 keypair for this account.
pub fn new_identity_keypair() -> anyhow::Result<(StaticSecret, PublicKey)> {
    let secret_bytes: [u8; 32] = random_array()?;
    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);
    Ok((secret, public))
}

/// Mint a vault key (the symmetric key used to encrypt op payloads).
pub fn new_vault_key() -> anyhow::Result<VaultKey> {
    Ok(VaultKey(random_array()?))
}

/// Sealed-box style: ephemeral X25519 + HKDF + XChaCha20-Poly1305.
///
/// Output layout: `eph_pub(32) || nonce(24) || ciphertext+tag`. The
/// recipient recovers the shared secret with their static private key,
/// derives the same KEK via HKDF-SHA256, and decrypts.
pub fn seal_for_recipient(recipient_pub: &PublicKey, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let eph_secret_bytes: [u8; 32] = random_array()?;
    let eph_secret = StaticSecret::from(eph_secret_bytes);
    let eph_public = PublicKey::from(&eph_secret);
    let shared = eph_secret.diffie_hellman(recipient_pub);

    let mut sym = [0u8; 32];
    let hk = Hkdf::<Sha256>::new(Some(eph_public.as_bytes()), shared.as_bytes());
    hk.expand(b"audire-sync wrap v1", &mut sym)
        .map_err(|_| anyhow!("hkdf expand"))?;

    let blob = aead_seal(&sym, plaintext, eph_public.as_bytes())?;
    sym.zeroize();

    let mut out = Vec::with_capacity(32 + blob.len());
    out.extend_from_slice(eph_public.as_bytes());
    out.extend_from_slice(&blob);
    Ok(out)
}

/// Inverse of `seal_for_recipient` using the recipient's static secret.
pub fn open_sealed(recipient_secret: &StaticSecret, blob: &[u8]) -> anyhow::Result<Vec<u8>> {
    if blob.len() < 32 + 24 + 16 {
        return Err(anyhow!("sealed blob too short"));
    }
    let mut eph_pub_bytes = [0u8; 32];
    eph_pub_bytes.copy_from_slice(&blob[..32]);
    let eph_pub = PublicKey::from(eph_pub_bytes);
    let shared = recipient_secret.diffie_hellman(&eph_pub);

    let mut sym = [0u8; 32];
    let hk = Hkdf::<Sha256>::new(Some(eph_pub.as_bytes()), shared.as_bytes());
    hk.expand(b"audire-sync wrap v1", &mut sym)
        .map_err(|_| anyhow!("hkdf expand"))?;
    let pt = aead_open(&sym, &blob[32..], eph_pub.as_bytes())?;
    sym.zeroize();
    Ok(pt)
}

/// Wrap the user's KEK under a freshly minted recovery key. Returned
/// tuple is `(recovery_key_hex, wrapped_kek_blob)`. The user is shown
/// the hex string exactly once and is told to store it offline.
pub fn mint_recovery_envelope(kek: &KekMaterial) -> anyhow::Result<(String, Vec<u8>)> {
    let recovery: [u8; 32] = random_array()?;
    let blob = aead_seal(&recovery, &kek.0, b"audire-recovery v1")?;
    let hex = hex::encode(recovery);
    // Drop the local copy of the recovery secret — the only person who
    // has it now is the user (it's about to be shown to them).
    Ok((hex, blob))
}

/// Recover the KEK using a recovery hex string + the wrapped blob.
pub fn open_recovery_envelope(recovery_hex: &str, blob: &[u8]) -> anyhow::Result<KekMaterial> {
    let bytes = hex::decode(recovery_hex.trim()).context("invalid recovery key hex")?;
    if bytes.len() != 32 {
        return Err(anyhow!("recovery key must be 32 bytes"));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    let pt = aead_open(&key, blob, b"audire-recovery v1")?;
    key.zeroize();
    if pt.len() != 32 {
        return Err(anyhow!("recovered KEK has wrong length"));
    }
    let mut kek = [0u8; 32];
    kek.copy_from_slice(&pt);
    Ok(KekMaterial(kek))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_round_trip() {
        let salt = random_bytes(16).unwrap();
        let a = derive_kek("hunter2-correct-horse", &salt).unwrap();
        let b = derive_kek("hunter2-correct-horse", &salt).unwrap();
        assert_eq!(a.0, b.0);
        let c = derive_kek("different", &salt).unwrap();
        assert_ne!(a.0, c.0);
    }

    #[test]
    fn aead_round_trip() {
        let key = random_array::<32>().unwrap();
        let blob = aead_seal(&key, b"hello world", b"meta").unwrap();
        let pt = aead_open(&key, &blob, b"meta").unwrap();
        assert_eq!(pt, b"hello world");
        assert!(aead_open(&key, &blob, b"different aad").is_err());
    }

    #[test]
    fn sealed_box_round_trip() {
        let (sk, pk) = new_identity_keypair().unwrap();
        let blob = seal_for_recipient(&pk, b"vault key bytes").unwrap();
        let opened = open_sealed(&sk, &blob).unwrap();
        assert_eq!(opened, b"vault key bytes");
    }

    #[test]
    fn recovery_round_trip() {
        let kek = KekMaterial(random_array::<32>().unwrap());
        let kek_copy = kek.0;
        let (hex, blob) = mint_recovery_envelope(&kek).unwrap();
        let recovered = open_recovery_envelope(&hex, &blob).unwrap();
        assert_eq!(recovered.0, kek_copy);
    }
}
