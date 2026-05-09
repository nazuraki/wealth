-- Merge accounts that share the same last 4 digits into the earliest-inserted one,
-- then remove the orphaned duplicates.

UPDATE statements
SET account_id = (
    SELECT MIN(a.id)
    FROM accounts a
    WHERE a.account_number_last4 = (
        SELECT account_number_last4 FROM accounts WHERE id = statements.account_id
    )
)
WHERE account_id IN (
    SELECT id FROM accounts
    WHERE id NOT IN (SELECT MIN(id) FROM accounts GROUP BY account_number_last4)
);

DELETE FROM accounts
WHERE id NOT IN (SELECT MIN(id) FROM accounts GROUP BY account_number_last4);
