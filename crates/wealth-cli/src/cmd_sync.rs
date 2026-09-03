use crate::{config, Args};
use anyhow::{anyhow, Result};
use extractor::{AnthropicClient, ClaudeClient};
use simplefin::{HttpFeed, SyncReport};

pub fn run(args: &Args) -> Result<i32> {
    let settings = config::settings(args)?;
    let access_url = settings
        .simplefin_access_url
        .clone()
        .ok_or_else(|| anyhow!("no SimpleFIN access URL: save one in the app's Settings, pass --access-url, or set SIMPLEFIN_ACCESS_URL"))?;
    let feed = HttpFeed::new(&access_url)?;

    let client = if args.flag("no-model") {
        None
    } else {
        settings.api_key.clone().map(|key| {
            let base = settings
                .endpoint_url
                .clone()
                .unwrap_or_else(|| AnthropicClient::DEFAULT_ENDPOINT.to_string());
            AnthropicClient::with_config(key, base)
        })
    };
    if client.is_none() && !args.flag("no-model") {
        eprintln!("note: no Anthropic API key; new transactions without a matching rule stay Uncategorized");
    }

    let mut conn = config::open(args)?;
    let report = simplefin::run_sync(&mut conn, &feed, client.as_ref().map(|c| c as &dyn ClaudeClient))?;

    if args.flag("json") {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    Ok(if report.errors.is_empty() && report.unmapped.is_empty() { 0 } else { 2 })
}

fn print_report(r: &SyncReport) {
    for a in &r.accounts {
        eprintln!(
            "✓ {} ···{}: {} new, {} already present, {} before cutover, {} pending{}",
            a.institution,
            a.account_number_last4,
            a.inserted,
            a.already_present,
            a.skipped_before_cutover,
            a.skipped_pending,
            if a.periods.is_empty() { String::new() } else { format!(" [{}]", a.periods.join(", ")) }
        );
    }
    eprintln!(
        "categories: {} by rule, {} by model, {} uncategorized",
        r.categorized_by_rule, r.categorized_by_model, r.uncategorized
    );
    for u in &r.unmapped {
        eprintln!(
            "! unmapped feed account {} — {} \"{}\"\n  map it with: wealth-cli map {} --account <ID>  (or --institution \"{}\" --last4 NNNN)",
            u.simplefin_id, u.institution, u.name, u.simplefin_id, u.institution
        );
    }
    for e in &r.errors {
        eprintln!("! {e}");
    }
}
