#!/usr/bin/env node
// Puppeteer-based OAuth login script for ClaudeCodeClaw.
// Reads: OAUTH_URL, OAUTH_EMAIL, OAUTH_PASSWORD, PUPPETEER_EXECUTABLE_PATH
// Outputs JSON lines to stdout for progress. Reads MFA code from stdin if needed.

const puppeteer = require('puppeteer-core');

const URL = process.env.OAUTH_URL;
const EMAIL = process.env.OAUTH_EMAIL;
const PASSWORD = process.env.OAUTH_PASSWORD;
const BROWSER = process.env.PUPPETEER_EXECUTABLE_PATH || '/usr/bin/chromium';

function emit(obj) { console.log(JSON.stringify(obj)); }

async function readLine(timeoutMs) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('MFA timeout')), timeoutMs);
    process.stdin.setEncoding('utf8');
    process.stdin.resume();
    process.stdin.once('data', (data) => {
      clearTimeout(timer);
      process.stdin.pause();
      resolve(data.toString().trim());
    });
  });
}

(async () => {
  if (!URL || !EMAIL || !PASSWORD) {
    emit({ step: 'error', message: 'Missing OAUTH_URL, OAUTH_EMAIL, or OAUTH_PASSWORD' });
    process.exit(1);
  }

  let browser;
  try {
    browser = await puppeteer.launch({
      executablePath: BROWSER,
      headless: 'new',
      args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage'],
    });
    const page = await browser.newPage();

    emit({ step: 'navigating', message: 'Opening login page...' });
    await page.goto(URL, { waitUntil: 'networkidle2', timeout: 30000 });

    // Find and fill email
    const emailSel = await page.waitForSelector(
      'input[type=email], input[name=email], #email',
      { timeout: 10000 }
    );
    await emailSel.type(EMAIL, { delay: 50 });

    // Find and fill password
    const passSel = await page.waitForSelector(
      'input[type=password], input[name=password], #password',
      { timeout: 10000 }
    );
    await passSel.type(PASSWORD, { delay: 50 });

    // Find and click submit
    const submitBtn = await page.waitForSelector(
      'button[type=submit], input[type=submit]',
      { timeout: 5000 }
    );
    emit({ step: 'login', message: 'Submitting credentials...' });
    await submitBtn.click();

    await new Promise(r => setTimeout(r, 3000));

    // Check for MFA
    const mfaInput = await page.$([
      'input[autocomplete=one-time-code]',
      'input[name=code]',
      'input[name=totp]',
      'input[name=mfa]',
    ].join(', '));

    const mfaLabel = await page.evaluate(() => {
      const labels = Array.from(document.querySelectorAll('label, p, h2, h3'));
      return labels.some(el => /verification|authenticator|code/i.test(el.textContent));
    });

    if (mfaInput || mfaLabel) {
      emit({ step: 'mfa_required', mfa_type: 'totp' });
      const code = await readLine(300000); // 5 min timeout
      const target = mfaInput || await page.$('input[type=text], input[type=number]');
      if (target) {
        await target.type(code, { delay: 50 });
        const mfaSubmit = await page.$('button[type=submit], input[type=submit]');
        if (mfaSubmit) await mfaSubmit.click();
      }
    }

    emit({ step: 'authorizing', message: 'Completing authorization...' });
    await page.waitForNavigation({ timeout: 30000 }).catch(() => {});
    await new Promise(r => setTimeout(r, 3000));

    emit({ step: 'success', message: 'OAuth login completed' });
    await browser.close();
    process.exit(0);
  } catch (err) {
    emit({ step: 'error', message: err.message || String(err) });
    if (browser) await browser.close().catch(() => {});
    process.exit(1);
  }
})();
