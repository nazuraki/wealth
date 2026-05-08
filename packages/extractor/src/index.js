import "dotenv/config";
import Anthropic from "@anthropic-ai/sdk";
import fs from "fs";
import { createRequire } from "module";

// pdf-parse is CommonJS; bridge it via createRequire
const require = createRequire(import.meta.url);
const pdfParse = require("pdf-parse");

const client = new Anthropic();

async function extractText(pdfPath) {
  const buffer = fs.readFileSync(pdfPath);
  const data = await pdfParse(buffer);
  return data.text;
}

async function parseLineItems(rawText, label) {
  const response = await client.messages.create({
    model: "claude-opus-4-7",
    max_tokens: 4096,
    output_config: {
      format: {
        type: "json_schema",
        schema: {
          type: "object",
          properties: {
            account: {
              type: "object",
              properties: {
                institution: { type: "string" },
                account_number_last4: { type: "string" },
                statement_period: { type: "string" },
                opening_balance: { type: ["number", "null"] },
                closing_balance: { type: ["number", "null"] },
              },
              required: [
                "institution",
                "account_number_last4",
                "statement_period",
                "opening_balance",
                "closing_balance",
              ],
              additionalProperties: false,
            },
            transactions: {
              type: "array",
              items: {
                type: "object",
                properties: {
                  date: { type: "string" },
                  description: { type: "string" },
                  category: { type: "string" },
                  amount: { type: "number" },
                  type: { type: "string", enum: ["debit", "credit"] },
                },
                required: ["date", "description", "category", "amount", "type"],
                additionalProperties: false,
              },
            },
            summary: {
              type: "object",
              properties: {
                total_debits: { type: "number" },
                total_credits: { type: "number" },
                transaction_count: { type: "number" },
              },
              required: ["total_debits", "total_credits", "transaction_count"],
              additionalProperties: false,
            },
          },
          required: ["account", "transactions", "summary"],
          additionalProperties: false,
        },
      },
    },
    system:
      "You are a financial data extraction assistant. Extract structured transaction data from bank and credit card statements. " +
      "For amounts: debits (purchases, payments, fees) are positive numbers with type 'debit'. " +
      "Credits (deposits, refunds, payments received) are positive numbers with type 'credit'. " +
      "Infer a category for each transaction (e.g. Groceries, Dining, Travel, Utilities, Income, Transfer, Fee). " +
      "If a field cannot be determined from the text, use null for nullable fields or an empty string for strings.",
    messages: [
      {
        role: "user",
        content: `Extract all transactions and account information from this statement.\n\nLabel: ${label}\n\n${rawText}`,
      },
    ],
  });

  const textBlock = response.content.find((b) => b.type === "text");
  return JSON.parse(textBlock.text);
}

async function main() {
  const args = process.argv.slice(2);
  if (args.length === 0) {
    console.error("Usage: node extract-statement.js <path-to-pdf> [label]");
    console.error("Example: node extract-statement.js chase_dec_2024.pdf");
    process.exit(1);
  }

  const pdfPath = args[0];
  const label = args[1] ?? pdfPath;

  if (!fs.existsSync(pdfPath)) {
    console.error(`File not found: ${pdfPath}`);
    process.exit(1);
  }

  console.error(`Extracting text from ${pdfPath}...`);
  const rawText = await extractText(pdfPath);

  console.error(`Parsing ${rawText.length} characters with Claude...`);
  const result = await parseLineItems(rawText, label);

  // Print structured JSON to stdout so it can be piped
  console.log(JSON.stringify(result, null, 2));

  // Print summary to stderr so it doesn't interfere with piped output
  const { summary, account } = result;
  console.error(`\n✓ ${account.institution} ${account.statement_period}`);
  console.error(
    `  ${summary.transaction_count} transactions | ` +
      `Debits: $${summary.total_debits.toFixed(2)} | ` +
      `Credits: $${summary.total_credits.toFixed(2)}`,
  );
}

main().catch((err) => {
  console.error(err.message);
  process.exit(1);
});
