import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Run in Node environment (no DOM needed for a server-side relay).
    environment: "node",
    // Test files: anything under src/**/__tests__/**/*.test.ts
    include: ["src/**/__tests__/**/*.test.ts"],
    // Timeouts: increase slightly to allow retries with short back-off.
    testTimeout: 10_000,
    // Produce a clean, readable summary in CI.
    reporter: ["verbose"],
  },
});
