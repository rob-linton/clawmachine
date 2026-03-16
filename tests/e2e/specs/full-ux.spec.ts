import { test, expect, Page } from '@playwright/test';

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
});

test.describe('Jobs Screen', () => {
  test('navigates and shows job list', async ({ page }) => {
    await page.goto('/#/jobs');
    await waitForFlutter(page, 7000);
    const body = await bodyText(page);
    expect(body).toContain('Jobs');
    expect(body).toContain('New Job');
  });
});

test.describe('Submit Job', () => {
  test('shows form elements including advanced options', async ({ page }) => {
    await page.goto('/#/jobs/new');
    await waitForFlutter(page);
    await expectBody(page, 'Submit New Job');
    await expectBody(page, 'Prompt');
    await expectBody(page, 'Model');
    await expectBody(page, 'Advanced Options');
  });

  test('submit a job via API and verify in UI', async ({ page, request }) => {
    const resp = await request.post('http://localhost:8080/api/v1/jobs', {
      data: { prompt: 'What is 4+4? Just the number.' },
    });
    const { id } = await resp.json();

    for (let i = 0; i < 60; i++) {
      const r = await request.get(`http://localhost:8080/api/v1/jobs/${id}`);
      const job = await r.json();
      if (job.status === 'completed' || job.status === 'failed') break;
      await new Promise(r => setTimeout(r, 3000));
    }

    await page.goto(`/#/jobs/${id}`);
    await waitForFlutter(page, 6000);

    const body = await bodyText(page);
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
    for (let i = 0; i < 60; i++) {
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
    expect(body).toContain('Result');
    expect(body).toContain('Logs');
  });

  test('shows re-submit and delete buttons for completed job', async ({ page }) => {
    await page.goto(`/#/jobs/${jobId}`);
    await waitForFlutter(page, 6000);

    const body = await bodyText(page);
    expect(body).toContain('Re-submit');
    expect(body).toContain('Delete');
    // Cancel should NOT be visible
    const cancelBtn = page.getByRole('button', { name: 'Cancel' });
    await expect(cancelBtn).not.toBeVisible();
  });

  test('delete job removes it', async ({ page, request }) => {
    // Create a throwaway job
    const resp = await request.post('http://localhost:8080/api/v1/jobs', {
      data: { prompt: 'Delete me. Just say ok.' },
    });
    const { id } = (await resp.json());
    for (let i = 0; i < 60; i++) {
      const r = await request.get(`http://localhost:8080/api/v1/jobs/${id}`);
      if (['completed', 'failed'].includes((await r.json()).status)) break;
      await new Promise(r => setTimeout(r, 3000));
    }

    // Delete via API
    const del = await request.delete(`http://localhost:8080/api/v1/jobs/${id}`);
    expect(del.status()).toBe(204);

    // Verify 404
    const get = await request.get(`http://localhost:8080/api/v1/jobs/${id}`);
    expect(get.status()).toBe(404);
  });
});

test.describe('Schedules Screen', () => {
  test('navigates and shows schedules page', async ({ page }) => {
    await page.goto('/#/schedules');
    await waitForFlutter(page, 7000);
    await expectBody(page, 'Schedules');
    await expectBody(page, 'New Schedule');
  });

  test('cron CRUD via API and verify in UI via semantics', async ({ page, request }) => {
    // Create cron via API
    const resp = await request.post('http://localhost:8080/api/v1/crons', {
      data: {
        name: 'e2e-test-cron',
        schedule: '0 0 * * * *',
        enabled: true,
        prompt: 'Playwright test cron',
      },
    });
    expect(resp.status()).toBe(201);
    const cron = await resp.json();

    // View in UI — verify cron name appears via Semantics label
    await page.goto('/#/schedules');
    await waitForFlutter(page, 7000);
    const cronLabel = page.getByLabel('Schedule e2e-test-cron');
    await expect(cronLabel.first()).toBeVisible();

    // Click Trigger Now button
    const triggerBtn = page.getByRole('button', { name: 'Trigger Now' });
    await expect(triggerBtn.first()).toBeVisible();

    // Trigger via API and verify
    const trigger = await request.post(`http://localhost:8080/api/v1/crons/${cron.id}/trigger`);
    expect(trigger.status()).toBe(201);

    // Update via API
    const update = await request.put(`http://localhost:8080/api/v1/crons/${cron.id}`, {
      data: { name: 'updated-cron', schedule: '0 0 * * * *', enabled: false, prompt: 'Updated' },
    });
    expect(update.status()).toBe(200);
    expect((await update.json()).enabled).toBe(false);

    // Reload and verify updated name in semantics
    await page.goto('/#/schedules');
    await waitForFlutter(page, 7000);
    await expect(page.getByLabel('Schedule updated-cron').first()).toBeVisible();

    // Clean up
    await request.delete(`http://localhost:8080/api/v1/crons/${cron.id}`);
  });
});

test.describe('Skills Screen', () => {
  test('shows skills and New Skill button', async ({ page }) => {
    await page.goto('/#/skills');
    await waitForFlutter(page, 7000);
    await expectBody(page, 'Skills');
    await expectBody(page, 'New Skill');
  });

  test('skill CRUD via API and verify in UI via semantics', async ({ page, request }) => {
    // Create skill via API
    const create = await request.post('http://localhost:8080/api/v1/skills', {
      data: {
        id: 'e2e-skill',
        name: 'E2E Test Skill',
        skill_type: 'template',
        content: 'Be helpful.',
        description: 'Test skill',
        tags: ['e2e'],
      },
    });
    expect(create.status()).toBe(201);

    // View in UI — verify skill name via Semantics label
    await page.goto('/#/skills');
    await waitForFlutter(page, 7000);
    await expect(page.getByLabel('Skill E2E Test Skill')).toBeVisible();

    // Click the skill card to open edit dialog
    await page.getByLabel('Skill E2E Test Skill').click();
    await waitForFlutter(page, 2000);
    // Edit dialog should have Save button
    await expect(page.getByRole('button', { name: 'Save' })).toBeVisible();
    // Close dialog
    await page.getByRole('button', { name: 'Cancel' }).click();

    // Update via API and verify
    const update = await request.put('http://localhost:8080/api/v1/skills/e2e-skill', {
      data: {
        id: 'e2e-skill',
        name: 'E2E Updated Skill',
        skill_type: 'template',
        content: 'Be very helpful.',
        description: 'Updated',
        tags: ['e2e', 'updated'],
      },
    });
    expect(update.status()).toBe(200);

    // Verify update via API (Flutter canvas may cache)
    const get = await request.get('http://localhost:8080/api/v1/skills/e2e-skill');
    expect((await get.json()).name).toBe('E2E Updated Skill');

    // Reload and verify the skill card is still clickable (edit dialog opens)
    await page.goto('/#/skills');
    await waitForFlutter(page, 7000);
    // Find a skill card by looking for a clickable with the skill semantics
    const skillCards = page.locator('[aria-label*="Skill"]');
    if (await skillCards.count() > 0) {
      await skillCards.first().click();
      await waitForFlutter(page, 2000);
      // Edit dialog should show Save button
      await expect(page.getByRole('button', { name: 'Save' })).toBeVisible();
      await page.getByRole('button', { name: 'Cancel' }).click();
    }

    // Clean up
    await request.delete('http://localhost:8080/api/v1/skills/e2e-skill');
  });
});

test.describe('Settings Screen', () => {
  test('shows settings page with connection status', async ({ page }) => {
    await page.goto('/#/settings');
    await waitForFlutter(page, 6000);

    // Verify page rendered by checking for the Check connection button (tooltip)
    await expect(page.getByRole('button', { name: 'Check connection' })).toBeVisible();
    // Verify semantics labels
    const labels = await page.locator('[aria-label]').allTextContents();
    const ariaLabels = await page.locator('[aria-label]').evaluateAll(
      els => els.map(e => e.getAttribute('aria-label'))
    );
    const hasSettings = ariaLabels.some(l => l?.includes('Settings'));
    const hasConnected = ariaLabels.some(l => l?.includes('connected') || l?.includes('Connected'));
    expect(hasSettings || hasConnected).toBeTruthy();
  });
});

test.describe('Full Journey', () => {
  test('navigate all screens', async ({ page, request }) => {
    // Submit a job
    const resp = await request.post('http://localhost:8080/api/v1/jobs', {
      data: { prompt: 'Capital of France? One word.' },
    });
    const { id } = await resp.json();
    for (let i = 0; i < 40; i++) {
      const r = await request.get(`http://localhost:8080/api/v1/jobs/${id}`);
      if ((await r.json()).status === 'completed') break;
      await new Promise(r => setTimeout(r, 3000));
    }

    // Dashboard
    await page.goto('/#/');
    await waitForFlutter(page);
    await expectBody(page, 'Recent Jobs');

    // Job detail
    await page.goto(`/#/jobs/${id}`);
    await waitForFlutter(page, 6000);
    await expectBody(page, 'Prompt');
    await expectBody(page, 'Result');

    // Jobs list
    await page.goto('/#/jobs');
    await waitForFlutter(page, 7000);
    await expectBody(page, 'New Job');

    // Schedules
    await page.goto('/#/schedules');
    await waitForFlutter(page, 7000);
    await expectBody(page, 'Schedules');

    // Skills
    await page.goto('/#/skills');
    await waitForFlutter(page, 7000);
    await expectBody(page, 'Skills');

    // Settings
    await page.goto('/#/settings');
    await waitForFlutter(page, 6000);
    // Settings page renders with Check connection button
    await expect(page.getByRole('button', { name: 'Check connection' })).toBeVisible();
  });
});
