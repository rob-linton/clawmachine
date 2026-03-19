#!/usr/bin/env node
// Puppeteer-based OAuth login script for ClaudeCodeClaw.
// Reads: OAUTH_URL, OAUTH_EMAIL, PUPPETEER_EXECUTABLE_PATH
// Outputs JSON lines to stdout for progress.
// Note: Anthropic uses passwordless (magic link) login.

const puppeteer = require('puppeteer-core');

const URL = process.env.OAUTH_URL;
const EMAIL = process.env.OAUTH_EMAIL;
const BROWSER = process.env.PUPPETEER_EXECUTABLE_PATH || '/usr/bin/chromium';

function emit(obj) { console.log(JSON.stringify(obj)); }

(async () => {
  if (!URL || !EMAIL) {
    emit({ step: 'error', message: 'Missing OAUTH_URL or OAUTH_EMAIL' });
    process.exit(1);
  }

  let browser;
  try {
    browser = await puppeteer.launch({
      executablePath: BROWSER,
      headless: 'new',
      args: [
        '--no-sandbox',
        '--disable-setuid-sandbox',
        '--disable-dev-shm-usage',
        '--disable-blink-features=AutomationControlled',
        '--window-size=1920,1080',
      ],
    });
    const page = await browser.newPage();

    // Stealth: realistic user agent + hide webdriver
    await page.setUserAgent(
      'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36'
    );
    await page.evaluateOnNewDocument(() => {
      Object.defineProperty(navigator, 'webdriver', { get: () => undefined });
      window.chrome = { runtime: {} };
    });
    await page.setViewport({ width: 1920, height: 1080 });

    emit({ step: 'navigating', message: 'Opening Anthropic login page...' });
    await page.goto(URL, { waitUntil: 'networkidle0', timeout: 60000 });

    // Fill email field
    const emailInput = await page.waitForSelector('input#email, input[type=email]', { timeout: 10000 });
    await emailInput.click({ clickCount: 3 }); // select all existing text
    await emailInput.type(EMAIL, { delay: 30 });

    emit({ step: 'login', message: 'Submitting email for magic link...' });

    // Click "Continue with email"
    const buttons = await page.$$('button[type=submit]');
    let clicked = false;
    for (const btn of buttons) {
      const text = await page.evaluate(el => el.textContent.trim(), btn);
      if (text.toLowerCase().includes('continue with email') || text.toLowerCase().includes('continue')) {
        await btn.click();
        clicked = true;
        break;
      }
    }
    if (!clicked && buttons.length > 0) {
      await buttons[0].click();
    }

    // Wait for the page to respond
    await new Promise(r => setTimeout(r, 3000));

    // Check what happened after clicking
    const afterClick = await page.evaluate(() => ({
      url: window.location.href,
      body: document.body.innerText.substring(0, 500),
    }));

    emit({
      step: 'email_sent',
      message: `Check your email (${EMAIL}) for a login link from Anthropic. Click it to complete the OAuth flow.`,
      detail: afterClick.body.substring(0, 200),
    });

    // Now we wait. The user clicks the magic link in their email,
    // which redirects to the claude auth login callback.
    // We don't need to do anything else in the browser.
    // The claude auth login process (running in parallel) will receive the callback.

    // Wait up to 5 minutes for claude auth login to complete
    // (the parent process monitors the credentials file)
    emit({ step: 'waiting', message: 'Waiting for you to click the login link in your email (up to 5 minutes)...' });

    // Keep browser alive for a bit in case there are redirects
    await new Promise(r => setTimeout(r, 10000));

    await browser.close();

    // Signal that the browser part is done — parent process checks for credentials
    emit({ step: 'browser_done', message: 'Browser flow complete. Checking for OAuth tokens...' });
    process.exit(0);
  } catch (err) {
    emit({ step: 'error', message: err.message || String(err) });
    if (browser) await browser.close().catch(() => {});
    process.exit(1);
  }
})();
