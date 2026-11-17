//! Small shared helpers.

use regex::Regex;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::sync::LazyLock;

/// Compile a constant pattern. The patterns passed here are literals checked by the test suite.
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(crate) fn const_regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("constant regex pattern is valid")
}

/// Lowercase hex SHA-256 of a string.
pub(crate) fn sha_hex(s: &str) -> String {
    let digest = Sha256::digest(s.as_bytes());
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

pub(crate) static RE_FOOTNOTE: LazyLock<Regex> = LazyLock::new(|| const_regex(r"\[\^(\d+)\]"));

/// Today's date as `YYYY-MM-DD` (UTC), or the `POST_KIT_DATE` override.
pub(crate) fn today() -> String {
    if let Ok(d) = std::env::var("POST_KIT_DATE") {
        if !d.is_empty() {
            return d;
        }
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    civil_from_days(i64::try_from(secs / 86_400).unwrap_or(0))
}

/// Days since 1970-01-01 to `YYYY-MM-DD` (Howard Hinnant's algorithm).
pub(crate) fn civil_from_days(z: i64) -> String {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Standard base64 (RFC 4648), used for an HTTP basic-auth header.
pub(crate) fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), "1970-01-01");
        assert_eq!(civil_from_days(20_758), "2026-11-01");
        assert_eq!(civil_from_days(19_782), "2024-02-29");
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn constant_patterns_compile() {
        let _ = &*RE_FOOTNOTE;
    }
}
