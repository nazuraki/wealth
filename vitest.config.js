import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    globals: true,
    projects: [
      "packages/db/vitest.config.js",
      "packages/reporter/vitest.config.js",
    ],
  },
});
