-- Normalize MM/DD/YY - MM/DD/YY (e.g. "04/28/25 - 05/27/25") → YYYY-MM using the end date's month/year.
UPDATE statements
SET statement_period =
    '20' || substr(statement_period, length(statement_period) - 1, 2)
    || '-'
    || substr(statement_period, instr(statement_period, ' - ') + 3, 2)
WHERE statement_period LIKE '__/__/__ - __/__/__';

-- Normalize MM/DD/YYYY - MM/DD/YYYY (e.g. "01/24/2026 - 02/23/2026") → YYYY-MM.
UPDATE statements
SET statement_period =
    substr(statement_period, length(statement_period) - 3, 4)
    || '-'
    || substr(statement_period, instr(statement_period, ' - ') + 3, 2)
WHERE statement_period LIKE '__/__/____ - __/__/____';
