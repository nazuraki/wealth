mod anthropic;
mod classify;

pub use anthropic::AnthropicClient;
pub use classify::is_transfer;

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
    Transfer,
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

    /// One category per description, in order. `known_categories` are the
    /// categories already in use so the model reuses them where they fit.
    fn categorize(&self, descriptions: &[String], known_categories: &[String]) -> Result<Vec<String>>;
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

        fn categorize(&self, descriptions: &[String], _known: &[String]) -> Result<Vec<String>> {
            Ok(descriptions.iter().map(|_| "Other".to_string()).collect())
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
