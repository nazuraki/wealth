import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    globals: true,
    projects: [
      "packages/test-utils/vitest.config.js",
      "packages/extractor/vitest.config.js",
      "packages/db/vitest.config.js",
      "packages/reporter/vitest.config.js",
    ],
  },
});
