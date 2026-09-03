//! Description-based classification shared by the PDF importer and feed sync.

const TRANSFER_PATTERNS: &[&str] = &[
    "payment - thank you",
    "autopay",
    "online pmt",
    "ach payment",
    "online payment",
    "online transfer",
    "payment to chase card",
    "transfer to ",
    "transfer from ",
];

/// True when a description looks like a payment or transfer between the
/// user's own accounts rather than real income or spending.
pub fn is_transfer(description: &str) -> bool {
    let lower = description.to_lowercase();
    TRANSFER_PATTERNS.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_transfer_matches_known_patterns() {
        assert!(is_transfer("PAYMENT - THANK YOU"));
        assert!(is_transfer("Payment - Thank You"));
        assert!(is_transfer("AUTOPAY"));
        assert!(is_transfer("Chase Autopay"));
        assert!(is_transfer("ONLINE PMT"));
        assert!(is_transfer("ACH PAYMENT"));
        assert!(is_transfer("ONLINE PAYMENT"));
        assert!(is_transfer("ONLINE TRANSFER"));
        assert!(is_transfer("Payment to Chase card ending in 1234"));
        assert!(is_transfer("Transfer to Online Savings"));
        assert!(is_transfer("Transfer from Interest Checking"));
    }

    #[test]
    fn is_transfer_does_not_match_regular_transactions() {
        assert!(!is_transfer("WHOLE FOODS MARKET"));
        assert!(!is_transfer("DIRECT DEPOSIT"));
        assert!(!is_transfer("AMAZON.COM"));
        assert!(!is_transfer("TRANSFER FEE"));
        assert!(!is_transfer("WIRE TRANSFER FEE"));
    }
}
