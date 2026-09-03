//! SimpleFIN protocol client. See https://www.simplefin.org/protocol.html
//!
//! An access URL looks like `https://user:pass@bridge.example/simplefin`.
//! `GET {access_url}/accounts` returns every linked account with recent
//! transactions; `start-date` (unix seconds) widens the window.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AccountSet {
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub accounts: Vec<FeedAccount>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Org {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeedAccount {
    pub id: String,
    #[serde(default)]
    pub org: Org,
    pub name: String,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub balance: Option<String>,
    #[serde(rename = "available-balance", default)]
    pub available_balance: Option<String>,
    #[serde(rename = "balance-date", default)]
    pub balance_date: Option<i64>,
    #[serde(default)]
    pub transactions: Vec<FeedTransaction>,
}

impl FeedAccount {
    /// Institution label: org name, falling back to org domain, then "SimpleFIN".
    pub fn institution(&self) -> String {
        self.org
            .name
            .clone()
            .or_else(|| self.org.domain.clone())
            .unwrap_or_else(|| "SimpleFIN".to_string())
    }

    pub fn balance_f64(&self) -> Option<f64> {
        self.balance.as_deref().and_then(|b| b.trim().parse().ok())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeedTransaction {
    pub id: String,
    /// Unix seconds. Zero for transactions that have not posted.
    #[serde(default)]
    pub posted: i64,
    /// Signed decimal string; negative is money leaving the account.
    pub amount: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub payee: Option<String>,
    #[serde(default)]
    pub memo: Option<String>,
    #[serde(default)]
    pub pending: bool,
}

pub fn parse_account_set(json: &str) -> Result<AccountSet> {
    serde_json::from_str(json).context("invalid SimpleFIN account set")
}

/// Access URL split into its parts so credentials go in an Authorization
/// header rather than being left in the request URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessUrl {
    pub base: String,
    pub username: String,
    pub password: String,
}

pub fn parse_access_url(raw: &str) -> Result<AccessUrl> {
    let raw = raw.trim();
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| anyhow!("access URL must start with https://"))?;
    let (userinfo, host_and_path) = rest
        .rsplit_once('@')
        .ok_or_else(|| anyhow!("access URL must contain user:password@ credentials"))?;
    let (user, pass) = userinfo
        .split_once(':')
        .ok_or_else(|| anyhow!("access URL credentials must be user:password"))?;
    if user.is_empty() || pass.is_empty() {
        bail!("access URL credentials must be user:password");
    }
    Ok(AccessUrl {
        base: format!("{scheme}://{}", host_and_path.trim_end_matches('/')),
        username: percent_decode(user),
        password: percent_decode(pass),
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Source of account data. Implemented over HTTP for real use and by fixtures in tests.
pub trait Feed {
    fn fetch(&self, start_date: Option<i64>) -> Result<AccountSet>;
}

pub struct HttpFeed {
    access: AccessUrl,
    http: reqwest::blocking::Client,
}

impl HttpFeed {
    pub fn new(access_url: &str) -> Result<Self> {
        let access = parse_access_url(access_url)?;
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        Ok(Self { access, http })
    }
}

impl Feed for HttpFeed {
    fn fetch(&self, start_date: Option<i64>) -> Result<AccountSet> {
        let mut req = self
            .http
            .get(format!("{}/accounts", self.access.base))
            .basic_auth(&self.access.username, Some(&self.access.password));
        if let Some(start) = start_date {
            req = req.query(&[("start-date", start.to_string())]);
        }
        let resp = req.send().context("SimpleFIN request failed")?;
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        if status.as_u16() == 403 {
            bail!("SimpleFIN access denied (403). The access URL may have been revoked or already claimed elsewhere.");
        }
        if !status.is_success() {
            bail!("SimpleFIN error {status}: {body}");
        }
        parse_account_set(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_access_url_and_decodes_credentials() {
        let a = parse_access_url("https://ab%2Fc:p%40ss@bridge.example/simplefin/").unwrap();
        assert_eq!(a.base, "https://bridge.example/simplefin");
        assert_eq!(a.username, "ab/c");
        assert_eq!(a.password, "p@ss");
        assert!(parse_access_url("https://bridge.example/simplefin").is_err());
        assert!(parse_access_url("bridge.example").is_err());
        assert!(parse_access_url("https://user@bridge.example").is_err());
    }

    #[test]
    fn parses_account_set_with_defaults() {
        let json = r#"{"errors":["Connection to Ally needs attention"],"accounts":[{"org":{"domain":"ally.com","name":"Ally"},"id":"acct-1","name":"Online Savings","currency":"USD","balance":"1500.25","available-balance":"1500.25","balance-date":1780000000,"transactions":[{"id":"t1","posted":1779900000,"amount":"-12.34","description":"GITHUB","pending":false},{"id":"t2","posted":0,"amount":"5.00","description":"REFUND","pending":true}]}]}"#;
        let set = parse_account_set(json).unwrap();
        assert_eq!(set.errors.len(), 1);
        let acct = &set.accounts[0];
        assert_eq!(acct.institution(), "Ally");
        assert_eq!(acct.balance_f64(), Some(1500.25));
        assert_eq!(acct.transactions.len(), 2);
        assert!(acct.transactions[1].pending);
        assert!(acct.transactions[0].payee.is_none());
    }

    #[test]
    fn institution_falls_back_to_domain() {
        let json = r#"{"accounts":[{"org":{"domain":"chase.com"},"id":"a","name":"Visa","transactions":[]}]}"#;
        let set = parse_account_set(json).unwrap();
        assert_eq!(set.accounts[0].institution(), "chase.com");
        assert!(set.errors.is_empty());
    }
}
