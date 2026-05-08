import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    name: "extractor",
    environment: "node",
    globals: true,
  },
});
