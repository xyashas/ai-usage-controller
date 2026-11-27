import { defineConfig } from "@playwright/test";
import fs from "node:fs";

const host = process.env.PLAYWRIGHT_HOST || "127.0.0.1";
const port = process.env.PLAYWRIGHT_PORT || "4173";
const baseURL = `http://${host}:${port}`;

const systemChromium = ["/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/google-chrome"].find((p) =>
  fs.existsSync(p)
);

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  retries: 0,
  reporter: "list",
  use: {
    baseURL,
    trace: "off",
    viewport: { width: 900, height: 800 },
    launchOptions: {
      ...(process.env.CHROMIUM_PATH || systemChromium
        ? { executablePath: process.env.CHROMIUM_PATH || systemChromium }
        : {}),
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    },
  },
  webServer: {
    command: `npm run preview -- --host ${host} --port ${port} --strictPort`,
    url: baseURL,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
