//! Secrets at rest for config. Provider API keys are sealed with XChaCha20-Poly1305
//! (AEAD) under a master key resolved from `OXIMY_MASTER_KEY` (base64, 32 bytes).
//! Plaintext is never persisted or logged. The sealed form is
//! `base64(nonce_24 || ciphertext_with_tag)` so it is a portable TEXT column.

use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, XChaCha20Poly1305, XNonce};

use crate::error::ConfigError;

/// 32-byte AEAD master key.
pub struct MasterKey {
    bytes: [u8; 32],
}

impl MasterKey {
    /// Build from raw 32 bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Parse a standard-base64 32-byte key (the `OXIMY_MASTER_KEY` form).
    pub fn from_base64(b64: &str) -> Result<Self, ConfigError> {
        let raw = b64_decode(b64).ok_or_else(|| ConfigError::Crypto {
            detail: "master key is not valid base64".into(),
        })?;
        let bytes: [u8; 32] = raw.try_into().map_err(|_| ConfigError::Crypto {
            detail: "master key must decode to exactly 32 bytes".into(),
        })?;
        Ok(Self { bytes })
    }

    /// Seal plaintext → portable `base64(nonce || ct+tag)`.
    pub fn seal(&self, plaintext: &str) -> Result<String, ConfigError> {
        let cipher = XChaCha20Poly1305::new((&self.bytes).into());
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ct = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| ConfigError::Crypto {
                detail: "seal failed".into(),
            })?;
        let mut combined = nonce.to_vec();
        combined.extend_from_slice(&ct);
        Ok(b64_encode(&combined))
    }

    /// Open a sealed value back to plaintext.
    pub fn open(&self, sealed: &str) -> Result<String, ConfigError> {
        let combined = b64_decode(sealed).ok_or_else(|| ConfigError::Crypto {
            detail: "sealed value not base64".into(),
        })?;
        if combined.len() < 24 {
            return Err(ConfigError::Crypto {
                detail: "sealed value too short".into(),
            });
        }
        let (nonce_bytes, ct) = combined.split_at(24);
        let nonce = XNonce::from_slice(nonce_bytes);
        let cipher = XChaCha20Poly1305::new((&self.bytes).into());
        let pt = cipher.decrypt(nonce, ct).map_err(|_| ConfigError::Crypto {
            detail: "open failed (bad key or tampered)".into(),
        })?;
        String::from_utf8(pt).map_err(|_| ConfigError::Crypto {
            detail: "plaintext not utf-8".into(),
        })
    }
}

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(B64_CHARS[(n >> 18 & 0x3f) as usize] as char);
        out.push(B64_CHARS[(n >> 12 & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64_CHARS[(n >> 6 & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_CHARS[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&c| c != b'=' && !c.is_ascii_whitespace())
        .collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        let mut bits = 0;
        for &c in chunk {
            n = (n << 6) | val(c)?;
            bits += 6;
        }
        n <<= 24 - bits;
        for i in 0..(bits / 8) {
            out.push((n >> (16 - i * 8) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> MasterKey {
        MasterKey::from_bytes([7u8; 32])
    }

    #[test]
    fn seal_open_roundtrip() {
        let k = key();
        let sealed = k.seal("sk-provider-secret").unwrap();
        assert_ne!(sealed, "sk-provider-secret"); // never plaintext
        assert_eq!(k.open(&sealed).unwrap(), "sk-provider-secret");
    }

    #[test]
    fn ciphertext_is_nondeterministic() {
        let k = key();
        // Fresh nonce each time → two seals of the same plaintext differ.
        assert_ne!(k.seal("same").unwrap(), k.seal("same").unwrap());
    }

    #[test]
    fn wrong_key_cannot_open() {
        let sealed = key().seal("secret").unwrap();
        let other = MasterKey::from_bytes([8u8; 32]);
        assert!(matches!(
            other.open(&sealed),
            Err(ConfigError::Crypto { .. })
        ));
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let k = key();
        let mut sealed = k.seal("secret").unwrap();
        // Flip the last base64 char.
        let last = sealed.pop().unwrap();
        sealed.push(if last == 'A' { 'B' } else { 'A' });
        assert!(k.open(&sealed).is_err());
    }

    #[test]
    fn from_base64_parses_32_bytes() {
        let b64 = b64_encode(&[9u8; 32]);
        let k = MasterKey::from_base64(&b64).unwrap();
        let sealed = k.seal("x").unwrap();
        assert_eq!(k.open(&sealed).unwrap(), "x");
    }

    #[test]
    fn from_base64_rejects_wrong_length() {
        let b64 = b64_encode(&[9u8; 16]);
        assert!(matches!(
            MasterKey::from_base64(&b64),
            Err(ConfigError::Crypto { .. })
        ));
    }
}
