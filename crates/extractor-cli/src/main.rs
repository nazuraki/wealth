use anyhow::Result;
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: extractor-cli <path-to-pdf> [label]");
        eprintln!("Example: extractor-cli chase_dec_2024.pdf");
        std::process::exit(1);
    }

    let pdf_path = Path::new(&args[1]);
    let label = args.get(2).map(String::as_str).unwrap_or(&args[1]);

    if !pdf_path.exists() {
        eprintln!("File not found: {}", pdf_path.display());
        std::process::exit(1);
    }

    let client = extractor::AnthropicClient::from_env()?;

    eprintln!("Extracting text from {}...", pdf_path.display());
    let raw_text = extractor::extract_text(pdf_path)?;

    eprintln!("Parsing {} characters with Claude...", raw_text.len());
    let result = extractor::parse_line_items(&raw_text, label, &client)?;

    // JSON to stdout so it can be piped; summary to stderr
    println!("{}", serde_json::to_string_pretty(&result)?);

    for ae in &result.accounts {
        eprintln!(
            "\n✓ {} ···{} {}\n  {} transactions | Debits: ${:.2} | Credits: ${:.2}",
            ae.account.institution,
            ae.account.account_number_last4,
            ae.account.statement_period,
            ae.summary.transaction_count,
            ae.summary.total_debits,
            ae.summary.total_credits
        );
    }

    Ok(())
}
