use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub institution: String,
    pub account_number_last4: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub account: Account,
    pub transactions: Vec<Transaction>,
    pub summary: Summary,
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
    http: reqwest::blocking::Client,
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: reqwest::blocking::Client::new(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;
        Ok(Self::new(api_key))
    }
}

impl ClaudeClient for AnthropicClient {
    fn extract_from_statement(&self, text: &str, label: &str) -> Result<ExtractionResult> {
        let body = serde_json::json!({
            "model": "claude-opus-4-7",
            "max_tokens": 4096,
            "output_config": {
                "format": {
                    "type": "json_schema",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "account": {
                                "type": "object",
                                "properties": {
                                    "institution": { "type": "string" },
                                    "account_number_last4": { "type": "string" },
                                    "statement_period": { "type": "string" },
                                    "opening_balance": { "type": ["number", "null"] },
                                    "closing_balance": { "type": ["number", "null"] }
                                },
                                "required": [
                                    "institution", "account_number_last4", "statement_period",
                                    "opening_balance", "closing_balance"
                                ],
                                "additionalProperties": false
                            },
                            "transactions": {
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
                            },
                            "summary": {
                                "type": "object",
                                "properties": {
                                    "total_debits": { "type": "number" },
                                    "total_credits": { "type": "number" },
                                    "transaction_count": { "type": "integer" }
                                },
                                "required": ["total_debits", "total_credits", "transaction_count"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["account", "transactions", "summary"],
                        "additionalProperties": false
                    }
                }
            },
            "system": "You are a financial data extraction assistant. Extract structured transaction data from bank and credit card statements. \
For amounts: debits (purchases, payments, fees) are positive numbers with type 'debit'. \
Credits (deposits, refunds, payments received) are positive numbers with type 'credit'. \
Infer a category for each transaction (e.g. Groceries, Dining, Travel, Utilities, Income, Transfer, Fee). \
If a field cannot be determined from the text, use null for nullable fields or an empty string for strings.",
            "messages": [{
                "role": "user",
                "content": format!(
                    "Extract all transactions and account information from this statement.\n\nLabel: {label}\n\n{text}"
                )
            }]
        });

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
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
        Ok(result)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn fixture() -> ExtractionResult {
        ExtractionResult {
            account: Account {
                institution: "First National Bank".into(),
                account_number_last4: "4242".into(),
                statement_period: "2024-12".into(),
                opening_balance: Some(1000.0),
                closing_balance: Some(850.5),
            },
            transactions: vec![
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
                Transaction {
                    date: "2024-12-10".into(),
                    description: "NETFLIX.COM".into(),
                    category: "Entertainment".into(),
                    amount: 15.99,
                    transaction_type: TransactionType::Debit,
                },
            ],
            summary: Summary {
                total_debits: 103.31,
                total_credits: 2500.0,
                transaction_count: 3,
            },
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
        let client = MockClient::new(fixture());
        let result = parse_line_items("raw statement text", "test-label", &client).unwrap();
        assert_eq!(result.account.institution, "First National Bank");
        assert_eq!(result.account.account_number_last4, "4242");
        assert_eq!(result.transactions.len(), 3);
        assert_eq!(result.summary.transaction_count, 3);
    }

    #[test]
    fn parse_line_items_passes_text_and_label() {
        let client = MockClient::new(fixture());
        parse_line_items("my statement", "my-label", &client).unwrap();
        let (text, label) = client.captured.lock().unwrap().clone().unwrap();
        assert!(text.contains("my statement"));
        assert!(label.contains("my-label"));
    }

    #[test]
    fn extraction_result_serializes_to_json_schema() {
        let result = fixture();
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["account"]["institution"], "First National Bank");
        assert_eq!(json["transactions"][0]["type"], "debit");
        assert_eq!(json["transactions"][1]["type"], "credit");
        assert_eq!(json["summary"]["transaction_count"], 3);
    }
}
