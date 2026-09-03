//! Feed transaction → app transaction shape: dates, periods, signed amounts.

use crate::client::FeedTransaction;

/// Civil date from unix seconds (UTC). Algorithm from Howard Hinnant's
/// "chrono-compatible low-level date algorithms"; avoids a chrono dependency.
pub fn unix_to_ymd(ts: i64) -> (i32, u32, u32) {
    let days = ts.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m, d)
}

pub fn iso_date(ts: i64) -> String {
    let (y, m, d) = unix_to_ymd(ts);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM` of an ISO date.
pub fn period_of(date: &str) -> &str {
    &date[..7.min(date.len())]
}

/// Unix seconds at 00:00 UTC on the first day of the month after `period` (YYYY-MM).
pub fn month_after_start(period: &str) -> Option<i64> {
    let y: i64 = period.get(..4)?.parse().ok()?;
    let m: i64 = period.get(5..7)?.parse().ok()?;
    let (y, m) = if m >= 12 { (y + 1, 1) } else { (y, m + 1) };
    Some(days_from_civil(y, m, 1) * 86_400)
}

/// Unix seconds at 00:00 UTC for an ISO date (YYYY-MM-DD).
pub fn date_to_unix(date: &str) -> Option<i64> {
    let y: i64 = date.get(..4)?.parse().ok()?;
    let m: i64 = date.get(5..7)?.parse().ok()?;
    let d: i64 = date.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86_400)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedTx {
    pub external_id: String,
    pub date: String,
    pub period: String,
    pub description: String,
    pub amount: f64,
    /// "debit" or "credit".
    pub kind: &'static str,
}

/// Why a feed transaction was not imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    Pending,
    BadAmount,
    EmptyDescription,
}

pub fn normalize(tx: &FeedTransaction) -> Result<NormalizedTx, Skip> {
    if tx.pending || tx.posted <= 0 {
        return Err(Skip::Pending);
    }
    let signed: f64 = tx.amount.trim().parse().map_err(|_| Skip::BadAmount)?;
    if !signed.is_finite() {
        return Err(Skip::BadAmount);
    }
    let description = [Some(tx.description.as_str()), tx.payee.as_deref(), tx.memo.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .ok_or(Skip::EmptyDescription)?
        .to_string();
    let date = iso_date(tx.posted);
    let period = period_of(&date).to_string();
    Ok(NormalizedTx {
        external_id: tx.id.clone(),
        date,
        period,
        description,
        amount: signed.abs(),
        kind: if signed < 0.0 { "debit" } else { "credit" },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tx(amount: &str, posted: i64) -> FeedTransaction {
        FeedTransaction {
            id: "x".into(),
            posted,
            amount: amount.into(),
            description: "GITHUB".into(),
            payee: None,
            memo: None,
            pending: false,
        }
    }

    #[test]
    fn unix_to_ymd_known_dates() {
        assert_eq!(unix_to_ymd(0), (1970, 1, 1));
        assert_eq!(unix_to_ymd(951_782_400), (2000, 2, 29));
        assert_eq!(unix_to_ymd(1_767_225_599), (2025, 12, 31));
        assert_eq!(unix_to_ymd(1_767_225_600), (2026, 1, 1));
        assert_eq!(iso_date(1_767_225_600), "2026-01-01");
    }

    #[test]
    fn month_after_start_rolls_over_year() {
        assert_eq!(month_after_start("2025-12"), Some(1_767_225_600));
        assert_eq!(month_after_start("2026-01"), Some(1_769_904_000));
        assert_eq!(month_after_start("garbage"), None);
        assert_eq!(unix_to_ymd(month_after_start("2026-05").unwrap()), (2026, 6, 1));
    }

    #[test]
    fn date_to_unix_round_trips() {
        assert_eq!(date_to_unix("2026-01-01"), Some(1_767_225_600));
        assert_eq!(iso_date(date_to_unix("2024-02-29").unwrap()), "2024-02-29");
        assert_eq!(date_to_unix("2026-13-01"), None);
        assert_eq!(date_to_unix("bad"), None);
    }

    #[test]
    fn normalize_maps_sign_to_kind() {
        let debit = normalize(&tx("-12.34", 1_767_225_600)).unwrap();
        assert_eq!(debit.kind, "debit");
        assert_eq!(debit.amount, 12.34);
        assert_eq!(debit.period, "2026-01");
        let credit = normalize(&tx("100", 1_767_225_600)).unwrap();
        assert_eq!(credit.kind, "credit");
        assert_eq!(credit.amount, 100.0);
    }

    #[test]
    fn normalize_skips_pending_and_bad_rows() {
        let mut pending = tx("-1", 1_767_225_600);
        pending.pending = true;
        assert_eq!(normalize(&pending), Err(Skip::Pending));
        assert_eq!(normalize(&tx("-1", 0)), Err(Skip::Pending));
        assert_eq!(normalize(&tx("abc", 1_767_225_600)), Err(Skip::BadAmount));
        let mut blank = tx("-1", 1_767_225_600);
        blank.description = "  ".into();
        assert_eq!(normalize(&blank), Err(Skip::EmptyDescription));
        blank.payee = Some(" Payee ".into());
        assert_eq!(normalize(&blank).unwrap().description, "Payee");
    }
}
