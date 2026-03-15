import { test, expect } from '@playwright/test';

test('inspect Flutter DOM structure', async ({ page }) => {
  await page.goto('/');

  // Wait for Flutter to fully load
  await page.waitForTimeout(5000);

  // Dump all text content visible in the accessibility tree
  const ariaLabels = await page.evaluate(() => {
    const elements: string[] = [];
    // Check flt-semantics elements
    document.querySelectorAll('[aria-label]').forEach(el => {
      elements.push(`aria-label: "${el.getAttribute('aria-label')}" tag=${el.tagName}`);
    });
    // Check role elements
    document.querySelectorAll('[role]').forEach(el => {
      const role = el.getAttribute('role');
      const label = el.getAttribute('aria-label') || el.textContent?.trim().substring(0, 50) || '';
      elements.push(`role=${role} label="${label}"`);
    });
    return elements;
  });

  console.log('=== Flutter DOM elements ===');
  for (const el of ariaLabels) {
    console.log(el);
  }

  // Also check for any visible text nodes
  const textContent = await page.evaluate(() => {
    const walker = document.createTreeWalker(
      document.body,
      NodeFilter.SHOW_TEXT,
      null
    );
    const texts: string[] = [];
    let node;
    while (node = walker.nextNode()) {
      const t = node.textContent?.trim();
      if (t && t.length > 0 && t.length < 100) {
        texts.push(t);
      }
    }
    return texts;
  });

  console.log('\n=== Text nodes ===');
  for (const t of textContent) {
    console.log(`"${t}"`);
  }

  // Try getByRole
  const buttons = await page.getByRole('button').count();
  console.log(`\n=== Buttons by role: ${buttons} ===`);

  const links = await page.getByRole('link').count();
  console.log(`=== Links by role: ${links} ===`);

  const tabs = await page.getByRole('tab').count();
  console.log(`=== Tabs by role: ${tabs} ===`);

  // Take screenshot
  await page.screenshot({ path: 'test-results/dashboard.png', fullPage: true });

  expect(true).toBe(true);
});
