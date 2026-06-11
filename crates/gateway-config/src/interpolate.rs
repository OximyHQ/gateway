//! `${ENV}` interpolation. Secrets live in the environment, not the config file;
//! at load time `${NAME}` is replaced from a resolver. A missing variable is a
//! hard error (fail-closed) — we never silently leave a `${...}` literal in a
//! credential or fall back to empty.

use std::collections::HashMap;

use crate::error::ConfigError;

/// Resolve every `${NAME}` in `input` via `lookup`. Errors on an unresolved name
/// or an unterminated `${`.
pub fn interpolate(
    input: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<String, ConfigError> {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let start = i + 2;
            let end = input[start..].find('}').map(|p| start + p).ok_or_else(|| {
                ConfigError::Interpolation {
                    detail: "unterminated ${".into(),
                }
            })?;
            let name = &input[start..end];
            let val = lookup(name).ok_or_else(|| ConfigError::Interpolation {
                detail: format!("env var {name} is not set"),
            })?;
            out.push_str(&val);
            i = end + 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(out)
}

/// Convenience resolver backed by a map (for tests + non-env sources).
pub fn map_lookup(map: &HashMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
    move |name| map.get(name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn replaces_known_vars() {
        let m = env(&[("OPENAI_API_KEY", "sk-live")]);
        let out = interpolate("${OPENAI_API_KEY}", &map_lookup(&m)).unwrap();
        assert_eq!(out, "sk-live");
    }

    #[test]
    fn replaces_inline_and_leaves_plain_text() {
        let m = env(&[("HOST", "api.example.com")]);
        let out = interpolate("https://${HOST}/v1", &map_lookup(&m)).unwrap();
        assert_eq!(out, "https://api.example.com/v1");
    }

    #[test]
    fn missing_var_is_fail_closed() {
        let m = env(&[]);
        assert!(matches!(
            interpolate("${NOPE}", &map_lookup(&m)),
            Err(ConfigError::Interpolation { .. })
        ));
    }

    #[test]
    fn unterminated_brace_errors() {
        let m = env(&[]);
        assert!(interpolate("${UNCLOSED", &map_lookup(&m)).is_err());
    }
}
