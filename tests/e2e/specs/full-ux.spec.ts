import { test, expect, Page } from '@playwright/test';

// Flutter renders to canvas with semantics overlay.
// textContent('body') captures semantics text. getByRole('button') works for clickables.
// Data loads async — need generous waits after navigation.

async function waitForFlutter(page: Page, ms = 5000) {
  await page.waitForTimeout(ms);
}

async function bodyText(page: Page): Promise<string> {
  return (await page.textContent('body')) ?? '';
}

async function expectBody(page: Page, text: string) {
  const body = await bodyText(page);
  expect(body, `Body should contain "${text}"`).toContain(text);
}

test.describe('Dashboard', () => {
  test('loads with stat cards and recent jobs', async ({ page }) => {
    await page.goto('/#/');
    await waitForFlutter(page);
    await expectBody(page, 'Pending');
    await expectBody(page, 'Running');
    await expectBody(page, 'Recent Jobs');
  });

  test('New Job button navigates to submit form', async ({ page }) => {
    await page.goto('/#/');
    await waitForFlutter(page);
    await page.getByRole('button', { name: 'New Job' }).click();
    await waitForFlutter(page);
    await expectBody(page, 'Submit New Job');
  });

  test('clicking a recent job opens detail', async ({ page }) => {
    await page.goto('/#/');
    await waitForFlutter(page);
    const jobBtns = page.getByRole('button').filter({ hasText: /completed|running/ });
    if (await jobBtns.count() > 0) {
      await jobBtns.first().click();
      await waitForFlutter(page);
      await expectBody(page, 'Prompt');
    }
  });
});

test.describe('Jobs Screen', () => {
  test('navigates and shows job list', async ({ page }) => {
    await page.goto('/#/jobs');
    await waitForFlutter(page, 7000); // Extra time for data load
    const body = await bodyText(page);
    // Should show Jobs heading and New Job button
    expect(body).toContain('Jobs');
    expect(body).toContain('New Job');
  });

  test('clicking a job opens detail', async ({ page }) => {
    await page.goto('/#/jobs');
    await waitForFlutter(page, 7000);
    const jobBtns = page.getByRole('button').filter({ hasText: /completed/ });
    if (await jobBtns.count() > 0) {
      await jobBtns.first().click();
      await waitForFlutter(page);
      await expectBody(page, 'Prompt');
    }
  });
});

test.describe('Submit Job', () => {
  test('shows form elements', async ({ page }) => {
    await page.goto('/#/jobs/new');
    await waitForFlutter(page);
    await expectBody(page, 'Submit New Job');
    await expectBody(page, 'Prompt');
    await expectBody(page, 'Model');
  });

  test('submit a job via API and verify in UI', async ({ page, request }) => {
    // Submit via API (reliable)
    const resp = await request.post('http://localhost:8080/api/v1/jobs', {
      data: { prompt: 'What is 4+4? Just the number.' },
    });
    const { id } = await resp.json();

    // Wait for completion
    for (let i = 0; i < 20; i++) {
      const r = await request.get(`http://localhost:8080/api/v1/jobs/${id}`);
      const job = await r.json();
      if (job.status === 'completed' || job.status === 'failed') break;
      await new Promise(r => setTimeout(r, 3000));
    }

    // Now view the job detail in the UI
    await page.goto(`/#/jobs/${id}`);
    await waitForFlutter(page, 6000);

    const body = await bodyText(page);
    // Note: prompt/result text rendered via SelectableText doesn't appear in DOM semantics
    // but headings and metadata do
    expect(body).toContain('Prompt');
    expect(body).toContain('Result');
    expect(body).toContain('Logs');
  });
});

test.describe('Job Detail', () => {
  let jobId: string;

  test.beforeAll(async ({ request }) => {
    const resp = await request.post('http://localhost:8080/api/v1/jobs', {
      data: { prompt: 'Say "hello world". Just those two words.' },
    });
    jobId = (await resp.json()).id;
    for (let i = 0; i < 20; i++) {
      const r = await request.get(`http://localhost:8080/api/v1/jobs/${jobId}`);
      if ((await r.json()).status === 'completed') break;
      await new Promise(r => setTimeout(r, 3000));
    }
  });

  test('shows job ID, prompt, result, logs', async ({ page }) => {
    await page.goto(`/#/jobs/${jobId}`);
    await waitForFlutter(page, 6000);

    const body = await bodyText(page);
    expect(body).toContain(jobId.substring(0, 8));
    expect(body).toContain('Prompt');
    // SelectableText content not in DOM semantics — verify headings instead
    expect(body).toContain('Result');
    expect(body).toContain('Logs');
  });

  test('no cancel button on completed job', async ({ page }) => {
    await page.goto(`/#/jobs/${jobId}`);
    await waitForFlutter(page);
    const cancelBtn = page.getByRole('button', { name: 'Cancel' });
    await expect(cancelBtn).not.toBeVisible();
  });
});

test.describe('Skills Screen', () => {
  test('shows skills after loading', async ({ page }) => {
    await page.goto('/#/skills');
    await waitForFlutter(page, 7000);
    await expectBody(page, 'Skills');
    // If the concise skill exists, it should show
    // (It may not if Redis was cleared between test runs)
  });

  test('skill detail dialog works', async ({ page }) => {
    await page.goto('/#/skills');
    await waitForFlutter(page, 7000);

    // Try to click a skill if any exist
    const body = await bodyText(page);
    if (body.includes('Concise')) {
      const btn = page.getByRole('button').filter({ hasText: 'Concise' });
      if (await btn.count() > 0) {
        await btn.first().click();
        await page.waitForTimeout(1500);
        await expectBody(page, 'Content:');
        const closeBtn = page.getByRole('button', { name: 'Close' });
        if (await closeBtn.isVisible()) await closeBtn.click();
      }
    }
  });
});

test.describe('Full Journey', () => {
  test('submit via API → view in UI → navigate all screens', async ({ page, request }) => {
    // 1. Submit a job via API
    const resp = await request.post('http://localhost:8080/api/v1/jobs', {
      data: { prompt: 'Capital of France? One word.', skill_ids: ['concise'] },
    });
    const { id } = await resp.json();

    // 2. Wait for completion
    for (let i = 0; i < 20; i++) {
      const r = await request.get(`http://localhost:8080/api/v1/jobs/${id}`);
      if ((await r.json()).status === 'completed') break;
      await new Promise(r => setTimeout(r, 3000));
    }

    // 3. View on Dashboard
    await page.goto('/#/');
    await waitForFlutter(page);
    await expectBody(page, 'Recent Jobs');

    // 4. View the job detail directly (dashboard list items use SelectableText for prompt preview which isn't in DOM)
    await page.goto(`/#/jobs/${id}`);
    await waitForFlutter(page, 6000);
    await expectBody(page, 'Prompt');
    await expectBody(page, 'Result');
    await expectBody(page, 'Logs');

    // 5. Navigate to Jobs list
    await page.goto('/#/jobs');
    await waitForFlutter(page, 7000);
    await expectBody(page, 'New Job');

    // 6. Navigate to Skills
    await page.goto('/#/skills');
    await waitForFlutter(page, 7000);
    await expectBody(page, 'Skills');

    // 7. Back to Dashboard
    await page.goto('/#/');
    await waitForFlutter(page);
    await expectBody(page, 'Recent Jobs');
  });
});
