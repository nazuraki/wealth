import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    name: "reporter",
    environment: "node",
    globals: true,
  },
});
