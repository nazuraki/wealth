import { describe, it, expect } from "vitest";
import { parseLineItems } from "../index.js";
import {
  createMockAnthropicClient,
  EXTRACTION_FIXTURE,
} from "@wealth/test-utils";

describe("parseLineItems", () => {
  it("returns structured extraction data from Claude response", async () => {
    const client = createMockAnthropicClient();
    const result = await parseLineItems("raw statement text", "test-label", client);

    expect(result.account.institution).toBe(EXTRACTION_FIXTURE.account.institution);
    expect(result.account.account_number_last4).toBe(EXTRACTION_FIXTURE.account.account_number_last4);
    expect(result.transactions).toHaveLength(EXTRACTION_FIXTURE.transactions.length);
    expect(result.summary.transaction_count).toBe(EXTRACTION_FIXTURE.summary.transaction_count);
  });

  it("passes rawText and label through to Claude", async () => {
    const client = createMockAnthropicClient();
    await parseLineItems("my statement", "my-label", client);

    const callArg = client.messages.create.mock.calls[0][0];
    expect(callArg.messages[0].content).toContain("my statement");
    expect(callArg.messages[0].content).toContain("my-label");
  });

  it("handles custom fixture responses", async () => {
    const custom = {
      ...EXTRACTION_FIXTURE,
      account: { ...EXTRACTION_FIXTURE.account, institution: "Chase" },
    };
    const client = createMockAnthropicClient(custom);
    const result = await parseLineItems("text", "label", client);
    expect(result.account.institution).toBe("Chase");
  });
});
