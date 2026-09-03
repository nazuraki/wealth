//! One-time setup: a SimpleFIN "setup token" is base64 of a claim URL. POSTing
//! to that URL once returns the permanent access URL, and the token is spent.

use anyhow::{bail, Context, Result};

pub fn is_access_url(value: &str) -> bool {
    let v = value.trim();
    v.starts_with("https://") || v.starts_with("http://")
}

/// Decode a setup token into its claim URL.
pub fn decode_setup_token(token: &str) -> Result<String> {
    let bytes = base64_decode(token.trim()).context("setup token is not valid base64")?;
    let url = String::from_utf8(bytes).context("setup token did not decode to text")?;
    if !is_access_url(&url) {
        bail!("setup token did not decode to a URL");
    }
    Ok(url)
}

/// Exchange a setup token for an access URL. Fails if the token was already used.
pub fn claim_access_url(token: &str) -> Result<String> {
    let claim_url = decode_setup_token(token)?;
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let resp = http
        .post(&claim_url)
        .header("content-length", "0")
        .send()
        .context("SimpleFIN claim request failed")?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if status.as_u16() == 403 {
        bail!("SimpleFIN rejected the setup token (403). Tokens are single-use; generate a new one.");
    }
    if !status.is_success() {
        bail!("SimpleFIN claim error {status}: {body}");
    }
    let access = body.trim().to_string();
    if !is_access_url(&access) {
        bail!("SimpleFIN claim did not return an access URL");
    }
    Ok(access)
}

/// Standard and URL-safe base64, padding optional. Kept local to avoid a dependency.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in input.as_bytes() {
        if c == b'=' || c == b'\n' || c == b'\r' {
            continue;
        }
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_setup_token_to_claim_url() {
        // base64("https://bridge.example/simplefin/claim/abc")
        let token = "aHR0cHM6Ly9icmlkZ2UuZXhhbXBsZS9zaW1wbGVmaW4vY2xhaW0vYWJj";
        assert_eq!(decode_setup_token(token).unwrap(), "https://bridge.example/simplefin/claim/abc");
        // URL-safe alphabet and trailing whitespace are accepted too.
        assert_eq!(decode_setup_token(&format!("{token}\n")).unwrap(), "https://bridge.example/simplefin/claim/abc");
    }

    #[test]
    fn rejects_garbage_tokens() {
        assert!(decode_setup_token("not base64!").is_err());
        assert!(decode_setup_token("aGVsbG8=").is_err()); // "hello", not a URL
    }

    #[test]
    fn base64_handles_padding_variants() {
        assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
        assert_eq!(base64_decode("aGk").unwrap(), b"hi");
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert!(base64_decode("a*b").is_none());
    }

    #[test]
    fn detects_access_urls() {
        assert!(is_access_url(" https://u:p@bridge.example/simplefin "));
        assert!(!is_access_url("aHR0cHM6Ly9icmlkZ2U"));
    }
}
