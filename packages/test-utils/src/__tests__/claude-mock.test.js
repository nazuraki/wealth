import { describe, it, expect, vi } from "vitest";
import {
  createMockAnthropicClient,
  EXTRACTION_FIXTURE,
} from "../claude-mock.js";

describe("createMockAnthropicClient", () => {
  it("returns fixture by default", async () => {
    const client = createMockAnthropicClient();
    const response = await client.messages.create({});
    const parsed = JSON.parse(response.content[0].text);
    expect(parsed.account.institution).toBe(
      EXTRACTION_FIXTURE.account.institution
    );
    expect(parsed.transactions).toHaveLength(3);
  });

  it("accepts a custom fixture", async () => {
    const custom = { ...EXTRACTION_FIXTURE, account: { ...EXTRACTION_FIXTURE.account, institution: "Custom Bank" } };
    const client = createMockAnthropicClient(custom);
    const response = await client.messages.create({});
    const parsed = JSON.parse(response.content[0].text);
    expect(parsed.account.institution).toBe("Custom Bank");
  });

  it("exposes a spy for call assertions", async () => {
    const client = createMockAnthropicClient();
    await client.messages.create({ model: "test" });
    expect(client.messages.create).toHaveBeenCalledWith({ model: "test" });
  });
});
