//! A cryptographically random secret generator for the admin API's key creation
//! endpoint. Mirrors `oximy-gateway::firstboot::generate_secret` so that the
//! `gateway-control` crate does not depend on the binary crate.

/// Generate a cryptographically random `ogw_`-prefixed secret (44 chars total:
/// 4-char prefix + 40 chars of base-62 entropy).  Mirrors the binary's
/// `firstboot::generate_secret`.
pub fn generate_secret() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let body: String = (0..40)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    format!("ogw_{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_secret_has_ogw_prefix() {
        let s = generate_secret();
        assert!(s.starts_with("ogw_"), "must start with ogw_");
        assert_eq!(s.len(), 44, "ogw_ (4) + 40 entropy chars");
    }

    #[test]
    fn secrets_are_unique() {
        let a = generate_secret();
        let b = generate_secret();
        assert_ne!(a, b);
    }
}
