import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  outputDir: ".codex/test-results",
  testMatch: "**/*.spec.ts",
  use: {
    baseURL: "http://127.0.0.1:1421",
    viewport: { width: 1500, height: 1050 },
    headless: true,
    launchOptions: {
      executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
    },
  },
  webServer: {
    command: "npm run dev -- --port 1421",
    url: "http://127.0.0.1:1421",
    reuseExistingServer: false,
  },
  reporter: "list",
});
