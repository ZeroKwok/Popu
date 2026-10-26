import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests/browser", workers: 2, timeout: 30000,
  expect: { timeout: 12000 },
  use: { browserName: "chromium", channel: "msedge", baseURL: "http://127.0.0.1:4179", headless: true },
  projects: [
    { name: "light", use: { colorScheme: "light", deviceScaleFactor: 1 } },
    { name: "dark-hidpi", use: { colorScheme: "dark", deviceScaleFactor: 2 } },
  ],
  webServer: { command: "node scripts/ui-test-server.mjs", url: "http://127.0.0.1:4179", reuseExistingServer: false },
});
