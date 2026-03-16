import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './specs',
  timeout: 300_000,
  expect: { timeout: 30_000 },
  use: {
    baseURL: 'http://localhost:8080',
    headless: true,
    screenshot: 'only-on-failure',
  },
  retries: 0,
});
