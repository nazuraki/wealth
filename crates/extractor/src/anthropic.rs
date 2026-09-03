//! HTTP client for the Claude Messages API. Rust has no official SDK, so this
//! calls `POST /v1/messages` directly with structured outputs.

use crate::{ClaudeClient, ExtractionResult};
use anyhow::Result;
use serde::Deserialize;

pub struct AnthropicClient {
    api_key: String,
    base_url: String,
    http: reqwest::blocking::Client,
}

impl AnthropicClient {
    pub const DEFAULT_ENDPOINT: &'static str = "https://api.anthropic.com/v1/messages";
    /// Model for statement extraction (long output, needs the strongest reader).
    pub const EXTRACT_MODEL: &'static str = "claude-opus-4-7";
    /// Model for batch categorization of short descriptions.
    pub const CATEGORIZE_MODEL: &'static str = "claude-opus-5";

    pub fn with_config(api_key: String, base_url: String) -> Self {
        // The blocking client defaults to a 30s timeout, but extracting a busy
        // statement (many transactions → thousands of output tokens) routinely
        // takes 35-60s. Give it generous headroom so long extractions don't fail
        // with "operation timed out".
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("failed to build HTTP client");
        Self { api_key, base_url, http }
    }

    pub fn new(api_key: String) -> Self {
        Self::with_config(api_key, Self::DEFAULT_ENDPOINT.to_string())
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;
        Ok(Self::new(api_key))
    }

    /// POST a Messages request and return the first text block, after checking
    /// for HTTP failures and safety refusals.
    fn send(&self, body: &serde_json::Value) -> Result<String> {
        let resp = self
            .http
            .post(&self.base_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(body)
            .send()?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            anyhow::bail!("Anthropic API error {status}: {body}");
        }

        let msg: MessageResponse = resp.json()?;
        if msg.stop_reason.as_deref() == Some("refusal") {
            anyhow::bail!("Anthropic API declined the request (stop_reason = refusal)");
        }
        msg.content
            .into_iter()
            .find(|b| b.kind == "text")
            .and_then(|b| b.text)
            .ok_or_else(|| anyhow::anyhow!("No text block in Anthropic response"))
    }
}

#[derive(Deserialize)]
struct MessageResponse {
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

// ── Schemas ───────────────────────────────────────────────────────────────────

fn account_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "institution": { "type": "string" },
            "account_number_last4": { "type": "string" },
            "account_type": {
                "anyOf": [
                    { "type": "string", "enum": ["credit_card", "checking", "savings"] },
                    { "type": "null" }
                ]
            },
            "statement_period": { "type": "string", "pattern": "^\\d{4}-\\d{2}$" },
            "opening_balance": { "type": ["number", "null"] },
            "closing_balance": { "type": ["number", "null"] }
        },
        "required": ["institution", "account_number_last4", "account_type",
                     "statement_period", "opening_balance", "closing_balance"],
        "additionalProperties": false
    })
}

fn transaction_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "date": { "type": "string" },
                "description": { "type": "string" },
                "category": { "type": "string" },
                "amount": { "type": "number" },
                "type": { "type": "string", "enum": ["debit", "credit"] }
            },
            "required": ["date", "description", "category", "amount", "type"],
            "additionalProperties": false
        }
    })
}

fn summary_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "total_debits": { "type": "number" },
            "total_credits": { "type": "number" },
            "transaction_count": { "type": "integer" }
        },
        "required": ["total_debits", "total_credits", "transaction_count"],
        "additionalProperties": false
    })
}

fn extraction_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "accounts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "account": account_schema(),
                        "transactions": transaction_schema(),
                        "summary": summary_schema()
                    },
                    "required": ["account", "transactions", "summary"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["accounts"],
        "additionalProperties": false
    })
}

const EXTRACT_SYSTEM: &str = "You are a financial data extraction assistant. Extract structured transaction data from bank and credit card statements. \
A single PDF may contain multiple accounts (e.g. a combined checking + savings statement) — return one entry per account in the accounts array. \
For amounts: debits (purchases, payments, fees) are positive numbers with type 'debit'. \
Credits (deposits, refunds, payments received) are positive numbers with type 'credit'. \
Infer a category for each transaction (e.g. Groceries, Dining, Travel, Utilities, Income, Transfer, Fee). \
For account_type: use 'credit_card' for credit card statements, 'checking' for checking accounts, 'savings' for savings accounts, or null if unclear. \
statement_period must be in YYYY-MM format using the statement end date (e.g. a statement ending May 27 2025 → '2025-05'). \
Each transaction date must be in YYYY-MM-DD format. If the statement only prints MM/DD without a year (common on credit-card statements), infer the year from the statement_period: use the period year for transactions in the period month or earlier in the calendar year, and the prior year for transactions whose month is greater than the period month (e.g. statement_period 2026-01 with date 12/28 → 2025-12-28). \
If a field cannot be determined from the text, use null for nullable fields or an empty string for strings.";

const DEFAULT_CATEGORIES: &str = "Groceries, Dining, Travel, Utilities, Income, Transfer, Fee, Shopping, Entertainment, Health, Insurance, Software, Cloud, AI APIs, Domains, Other";

impl ClaudeClient for AnthropicClient {
    fn extract_from_statement(&self, text: &str, label: &str) -> Result<ExtractionResult> {
        let body = serde_json::json!({
            "model": Self::EXTRACT_MODEL,
            "max_tokens": 16384,
            "output_config": { "format": { "type": "json_schema", "schema": extraction_schema() } },
            "system": EXTRACT_SYSTEM,
            "messages": [{
                "role": "user",
                "content": format!("Extract all accounts and their transactions from this statement.\n\nLabel: {label}\n\n{text}")
            }]
        });
        let text_block = self.send(&body)?;
        let result: ExtractionResult = serde_json::from_str(&text_block)?;
        if result.accounts.is_empty() {
            anyhow::bail!("No accounts found in statement");
        }
        Ok(result)
    }

    fn categorize(&self, descriptions: &[String], known_categories: &[String]) -> Result<Vec<String>> {
        if descriptions.is_empty() {
            return Ok(vec![]);
        }
        let known = if known_categories.is_empty() {
            DEFAULT_CATEGORIES.to_string()
        } else {
            known_categories.join(", ")
        };
        let numbered = descriptions
            .iter()
            .enumerate()
            .map(|(i, d)| format!("{}. {d}", i + 1))
            .collect::<Vec<_>>()
            .join("\n");
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "categories": { "type": "array", "items": { "type": "string" } } },
            "required": ["categories"],
            "additionalProperties": false
        });
        let body = serde_json::json!({
            "model": Self::CATEGORIZE_MODEL,
            "max_tokens": 8192,
            "output_config": { "format": { "type": "json_schema", "schema": schema }, "effort": "low" },
            "system": format!(
                "You assign one spending category to each bank or credit card transaction description. \
                 Prefer these existing categories so reports stay consistent: {known}. \
                 Only introduce a new category when none of them fits. \
                 Return exactly one category per numbered input, in the same order."
            ),
            "messages": [{ "role": "user", "content": format!("Categorize these {} transactions:\n\n{numbered}", descriptions.len()) }]
        });
        let text_block = self.send(&body)?;
        let parsed: serde_json::Value = serde_json::from_str(&text_block)?;
        let categories: Vec<String> = serde_json::from_value(parsed["categories"].clone())?;
        if categories.len() != descriptions.len() {
            anyhow::bail!(
                "categorize returned {} categories for {} descriptions",
                categories.len(),
                descriptions.len()
            );
        }
        Ok(categories)
    }
}
