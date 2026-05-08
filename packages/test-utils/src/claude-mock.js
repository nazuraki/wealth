import { vi } from "vitest";

/** A valid extraction response matching the schema in packages/db */
export const EXTRACTION_FIXTURE = {
  account: {
    institution: "First National Bank",
    account_number_last4: "4242",
    statement_period: "2024-12",
    opening_balance: 1000.0,
    closing_balance: 850.5,
  },
  transactions: [
    {
      date: "2024-12-01",
      description: "WHOLE FOODS MARKET",
      category: "Groceries",
      amount: 87.32,
      type: "debit",
    },
    {
      date: "2024-12-03",
      description: "DIRECT DEPOSIT",
      category: "Income",
      amount: 2500.0,
      type: "credit",
    },
    {
      date: "2024-12-10",
      description: "NETFLIX.COM",
      category: "Entertainment",
      amount: 15.99,
      type: "debit",
    },
  ],
  summary: {
    total_debits: 103.31,
    total_credits: 2500.0,
    transaction_count: 3,
  },
};

/**
 * Returns a mock Anthropic client whose messages.create resolves with fixture.
 * Pass a partial fixture to override specific fields.
 *
 * @param {object} [fixture] - Override default EXTRACTION_FIXTURE
 */
export function createMockAnthropicClient(fixture = EXTRACTION_FIXTURE) {
  const create = vi.fn().mockResolvedValue({
    content: [{ type: "text", text: JSON.stringify(fixture) }],
  });
  return { messages: { create } };
}
