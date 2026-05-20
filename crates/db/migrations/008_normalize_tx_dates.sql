-- Normalize transaction dates stored as MM/DD (no year) to YYYY-MM-DD using
-- statement_period as the anchor. Some extractor outputs (notably credit-card
-- statements that print only MM/DD on each row) left dates without a year.
--
-- Rule: a statement with period YYYY-MM typically spans the period month and
-- the prior month. So:
--   year = period_year         if tx_month <= period_month
--   year = period_year - 1     otherwise (tx_month > period_month → prior year)
--
-- Idempotent: only matches rows that still look like MM/DD.

UPDATE transactions
SET date = (
    CASE
        WHEN CAST(substr(transactions.date, 1, 2) AS INTEGER)
             <= CAST(substr(s.statement_period, 6, 2) AS INTEGER)
        THEN substr(s.statement_period, 1, 4)
        ELSE printf('%04d', CAST(substr(s.statement_period, 1, 4) AS INTEGER) - 1)
    END
    || '-' || substr(transactions.date, 1, 2)
    || '-' || substr(transactions.date, 4, 2)
)
FROM statements s
WHERE transactions.statement_id = s.id
  AND transactions.date GLOB '[0-9][0-9]/[0-9][0-9]'
  AND s.statement_period GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]';
