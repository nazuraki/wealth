use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub institution: String,
    pub account_number_last4: String,
    pub account_type: Option<String>,
    pub statement_period: String,
    pub opening_balance: Option<f64>,
    pub closing_balance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    Debit,
    Credit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub date: String,
    pub description: String,
    pub category: String,
    pub amount: f64,
    #[serde(rename = "type")]
    pub transaction_type: TransactionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub total_debits: f64,
    pub total_credits: f64,
    pub transaction_count: u32,
}

/// One account's worth of data extracted from a statement PDF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountExtraction {
    pub account: Account,
    pub transactions: Vec<Transaction>,
    pub summary: Summary,
}

/// Full result for a PDF, which may contain multiple accounts (e.g. a combined
/// checking + savings statement).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub accounts: Vec<AccountExtraction>,
}

// ── Client trait ──────────────────────────────────────────────────────────────

pub trait ClaudeClient: Send + Sync {
    fn extract_from_statement(&self, text: &str, label: &str) -> Result<ExtractionResult>;
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn extract_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)?;
    Ok(text)
}

pub fn parse_line_items(text: &str, label: &str, client: &dyn ClaudeClient) -> Result<ExtractionResult> {
    client.extract_from_statement(text, label)
}

// ── Anthropic HTTP client ─────────────────────────────────────────────────────

pub struct AnthropicClient {
    api_key: String,
    base_url: String,
    http: reqwest::blocking::Client,
}

impl AnthropicClient {
    pub const DEFAULT_ENDPOINT: &'static str = "https://api.anthropic.com/v1/messages";

    pub fn with_config(api_key: String, base_url: String) -> Self {
        Self {
            api_key,
            base_url,
            http: reqwest::blocking::Client::new(),
        }
    }

    pub fn new(api_key: String) -> Self {
        Self::with_config(api_key, Self::DEFAULT_ENDPOINT.to_string())
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;
        Ok(Self::new(api_key))
    }
}

// Reusable sub-schemas.
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

impl ClaudeClient for AnthropicClient {
    fn extract_from_statement(&self, text: &str, label: &str) -> Result<ExtractionResult> {
        let schema = serde_json::json!({
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
        });

        let body = serde_json::json!({
            "model": "claude-opus-4-7",
            "max_tokens": 8192,
            "output_config": {
                "format": { "type": "json_schema", "schema": schema }
            },
            "system": "You are a financial data extraction assistant. Extract structured transaction data from bank and credit card statements. \
A single PDF may contain multiple accounts (e.g. a combined checking + savings statement) — return one entry per account in the accounts array. \
For amounts: debits (purchases, payments, fees) are positive numbers with type 'debit'. \
Credits (deposits, refunds, payments received) are positive numbers with type 'credit'. \
Infer a category for each transaction (e.g. Groceries, Dining, Travel, Utilities, Income, Transfer, Fee). \
For account_type: use 'credit_card' for credit card statements, 'checking' for checking accounts, 'savings' for savings accounts, or null if unclear. \
statement_period must be in YYYY-MM format using the statement end date (e.g. a statement ending May 27 2025 → '2025-05'). \
If a field cannot be determined from the text, use null for nullable fields or an empty string for strings.",
            "messages": [{
                "role": "user",
                "content": format!(
                    "Extract all accounts and their transactions from this statement.\n\nLabel: {label}\n\n{text}"
                )
            }]
        });

        let resp = self
            .http
            .post(&self.base_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            anyhow::bail!("Anthropic API error {status}: {body}");
        }

        let json: serde_json::Value = resp.json()?;
        let text_block = json["content"]
            .as_array()
            .and_then(|arr| arr.iter().find(|b| b["type"] == "text"))
            .and_then(|b| b["text"].as_str())
            .ok_or_else(|| anyhow::anyhow!("No text block in Anthropic response"))?;

        let result: ExtractionResult = serde_json::from_str(text_block)?;
        if result.accounts.is_empty() {
            anyhow::bail!("No accounts found in statement");
        }
        Ok(result)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn make_account(last4: &str, account_type: &str, period: &str) -> Account {
        Account {
            institution: "First National Bank".into(),
            account_number_last4: last4.into(),
            account_type: Some(account_type.into()),
            statement_period: period.into(),
            opening_balance: Some(1000.0),
            closing_balance: Some(850.5),
        }
    }

    fn make_transactions() -> Vec<Transaction> {
        vec![
            Transaction {
                date: "2024-12-01".into(),
                description: "WHOLE FOODS MARKET".into(),
                category: "Groceries".into(),
                amount: 87.32,
                transaction_type: TransactionType::Debit,
            },
            Transaction {
                date: "2024-12-03".into(),
                description: "DIRECT DEPOSIT".into(),
                category: "Income".into(),
                amount: 2500.0,
                transaction_type: TransactionType::Credit,
            },
        ]
    }

    fn make_summary() -> Summary {
        Summary { total_debits: 87.32, total_credits: 2500.0, transaction_count: 2 }
    }

    fn single_account_fixture() -> ExtractionResult {
        ExtractionResult {
            accounts: vec![AccountExtraction {
                account: make_account("4242", "checking", "2024-12"),
                transactions: make_transactions(),
                summary: make_summary(),
            }],
        }
    }

    fn two_account_fixture() -> ExtractionResult {
        ExtractionResult {
            accounts: vec![
                AccountExtraction {
                    account: make_account("4242", "checking", "2024-12"),
                    transactions: make_transactions(),
                    summary: make_summary(),
                },
                AccountExtraction {
                    account: make_account("9999", "savings", "2024-12"),
                    transactions: vec![Transaction {
                        date: "2024-12-15".into(),
                        description: "INTEREST PAYMENT".into(),
                        category: "Income".into(),
                        amount: 12.50,
                        transaction_type: TransactionType::Credit,
                    }],
                    summary: Summary { total_debits: 0.0, total_credits: 12.50, transaction_count: 1 },
                },
            ],
        }
    }

    struct MockClient {
        fixture: ExtractionResult,
        captured: Mutex<Option<(String, String)>>,
    }

    impl MockClient {
        fn new(fixture: ExtractionResult) -> Self {
            Self { fixture, captured: Mutex::new(None) }
        }
    }

    impl ClaudeClient for MockClient {
        fn extract_from_statement(&self, text: &str, label: &str) -> Result<ExtractionResult> {
            *self.captured.lock().unwrap() = Some((text.to_string(), label.to_string()));
            Ok(self.fixture.clone())
        }
    }

    #[test]
    fn parse_line_items_returns_extraction_result() {
        let client = MockClient::new(single_account_fixture());
        let result = parse_line_items("raw statement text", "test-label", &client).unwrap();
        assert_eq!(result.accounts.len(), 1);
        assert_eq!(result.accounts[0].account.institution, "First National Bank");
        assert_eq!(result.accounts[0].transactions.len(), 2);
    }

    #[test]
    fn parse_line_items_passes_text_and_label() {
        let client = MockClient::new(single_account_fixture());
        parse_line_items("my statement", "my-label", &client).unwrap();
        let (text, label) = client.captured.lock().unwrap().clone().unwrap();
        assert!(text.contains("my statement"));
        assert!(label.contains("my-label"));
    }

    #[test]
    fn extraction_result_serializes_to_json_schema() {
        let result = single_account_fixture();
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["accounts"][0]["account"]["institution"], "First National Bank");
        assert_eq!(json["accounts"][0]["transactions"][0]["type"], "debit");
        assert_eq!(json["accounts"][0]["transactions"][1]["type"], "credit");
        assert_eq!(json["accounts"][0]["summary"]["transaction_count"], 2);
    }

    #[test]
    fn two_account_fixture_has_two_entries() {
        let result = two_account_fixture();
        assert_eq!(result.accounts.len(), 2);
        assert_eq!(result.accounts[0].account.account_number_last4, "4242");
        assert_eq!(result.accounts[1].account.account_number_last4, "9999");
    }
}
