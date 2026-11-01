import { defineConfig, devices } from "@playwright/test";

/**
 * E2E and interaction-latency tests run against a live stack (`docker compose up`, or
 * `make dev`). Point PLAYWRIGHT_BASE_URL elsewhere to test another deployment.
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  // "github" turns failures into check-run annotations, readable without log access.
  reporter: process.env.CI ? [["github"], ["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: process.env.PLAYWRIGHT_BASE_URL ?? "http://localhost:3000",
    trace: "retain-on-failure",
    viewport: { width: 1440, height: 900 },
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1440, height: 900 },
        // Headless Chromium defaults to software WebGL (SwiftShader); use the GPU when there
        // is one, as users' browsers do. GPU-less CI runners still fall back to software.
        launchOptions: { args: ["--enable-gpu", "--ignore-gpu-blocklist"] },
      },
    },
  ],
});
