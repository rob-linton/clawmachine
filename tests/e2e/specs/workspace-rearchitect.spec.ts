import { test, expect, Page, APIRequestContext } from '@playwright/test';

const API = 'http://localhost:8080/api/v1';

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

// ============================================================
// Config API Endpoints
// ============================================================

test.describe('Config API', () => {
  test('GET /config returns defaults', async ({ request }) => {
    const resp = await request.get(`${API}/config`);
    expect(resp.status()).toBe(200);
    const config = await resp.json();
    expect(config).toHaveProperty('execution_backend');
    expect(config).toHaveProperty('sandbox_image');
    expect(config).toHaveProperty('docker_memory_limit');
    expect(config).toHaveProperty('docker_cpu_limit');
    expect(config).toHaveProperty('docker_credential_mounts');
    expect(config).toHaveProperty('repos_dir');
    expect(config).toHaveProperty('checkouts_dir');
  });

  test('PUT /config/{key} sets and GET retrieves it', async ({ request }) => {
    // Set a value
    const put = await request.put(`${API}/config/test_key`, {
      data: { value: 'test_value_123' },
    });
    expect(put.status()).toBe(200);
    const putBody = await put.json();
    expect(putBody.key).toBe('test_key');
    expect(putBody.value).toBe('test_value_123');

    // Read it back
    const get = await request.get(`${API}/config/test_key`);
    expect(get.status()).toBe(200);
    const getBody = await get.json();
    expect(getBody.value).toBe('test_value_123');

    // Clean up - set to empty (or just leave; tests are idempotent)
  });

  test('PUT /config bulk update works', async ({ request }) => {
    const resp = await request.put(`${API}/config`, {
      data: { bulk_test_a: 'alpha', bulk_test_b: 'beta' },
    });
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body.updated).toBe(2);

    // Verify in full config
    const all = await request.get(`${API}/config`);
    const config = await all.json();
    expect(config.bulk_test_a).toBe('alpha');
    expect(config.bulk_test_b).toBe('beta');
  });
});

// ============================================================
// Docker API Endpoints
// ============================================================

test.describe('Docker API', () => {
  test('GET /docker/status returns availability info', async ({ request }) => {
    const resp = await request.get(`${API}/docker/status`);
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body).toHaveProperty('available');
    // If Docker is available, check extra fields
    if (body.available) {
      expect(body).toHaveProperty('server_version');
      expect(body).toHaveProperty('os');
    }
  });

  test('GET /docker/images returns image list', async ({ request }) => {
    const resp = await request.get(`${API}/docker/images`);
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body).toHaveProperty('images');
    expect(Array.isArray(body.images)).toBeTruthy();
  });

  test('POST /docker/images/pull rejects invalid image names', async ({ request }) => {
    const resp = await request.post(`${API}/docker/images/pull`, {
      data: { image: 'evil;rm -rf /' },
    });
    expect(resp.status()).toBe(400);
    const body = await resp.json();
    expect(body.error).toContain('Invalid image name');
  });

  test('POST /docker/images/pull handles non-existent image gracefully', async ({ request }) => {
    const resp = await request.post(`${API}/docker/images/pull`, {
      data: { image: 'nonexistent-image-99999:latest' },
    });
    expect(resp.status()).toBe(422);
    const body = await resp.json();
    expect(body.success).toBe(false);
  });
});

// ============================================================
// Extended Status Endpoint
// ============================================================

test.describe('Extended Status', () => {
  test('GET /status includes docker and config info', async ({ request }) => {
    const resp = await request.get(`${API}/status`);
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body.status).toBe('healthy');
    expect(body).toHaveProperty('docker_available');
    expect(body).toHaveProperty('sandbox_image_ready');
    expect(body).toHaveProperty('execution_backend');
    expect(body).toHaveProperty('worker_count');
    expect(body).toHaveProperty('queue');
    expect(typeof body.docker_available).toBe('boolean');
    expect(typeof body.worker_count).toBe('number');
  });
});

// ============================================================
// New-Style Workspace CRUD
// ============================================================

test.describe('Workspace CRUD - New Style', () => {
  let persistentId: string;
  let ephemeralId: string;
  let snapshotId: string;

  test('POST /workspaces creates persistent workspace with bare repo', async ({ request }) => {
    const resp = await request.post(`${API}/workspaces`, {
      data: {
        name: 'e2e-persistent',
        description: 'E2E persistent test',
        persistence: 'persistent',
        claude_md: '# E2E Test\nPersistent workspace.',
      },
    });
    expect(resp.status()).toBe(201);
    const ws = await resp.json();
    persistentId = ws.id;
    expect(ws.name).toBe('e2e-persistent');
    expect(ws.persistence).toBe('persistent');
    expect(ws.path).toBeNull();
    expect(ws.claude_md).toContain('E2E Test');
  });

  test('POST /workspaces creates ephemeral workspace', async ({ request }) => {
    const resp = await request.post(`${API}/workspaces`, {
      data: {
        name: 'e2e-ephemeral',
        persistence: 'ephemeral',
      },
    });
    expect(resp.status()).toBe(201);
    const ws = await resp.json();
    ephemeralId = ws.id;
    expect(ws.persistence).toBe('ephemeral');
    expect(ws.path).toBeNull();
  });

  test('POST /workspaces creates snapshot workspace with claw/base tag', async ({ request }) => {
    const resp = await request.post(`${API}/workspaces`, {
      data: {
        name: 'e2e-snapshot',
        persistence: 'snapshot',
        claude_md: '# Snapshot Base',
      },
    });
    expect(resp.status()).toBe(201);
    const ws = await resp.json();
    snapshotId = ws.id;
    expect(ws.persistence).toBe('snapshot');
  });

  test('GET /workspaces lists all three workspaces', async ({ request }) => {
    const resp = await request.get(`${API}/workspaces`);
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body.total).toBeGreaterThanOrEqual(3);
    const names = body.items.map((w: any) => w.name);
    expect(names).toContain('e2e-persistent');
    expect(names).toContain('e2e-ephemeral');
    expect(names).toContain('e2e-snapshot');
  });

  test('GET /workspaces/{id} returns workspace details', async ({ request }) => {
    const resp = await request.get(`${API}/workspaces/${persistentId}`);
    expect(resp.status()).toBe(200);
    const ws = await resp.json();
    expect(ws.id).toBe(persistentId);
    expect(ws.persistence).toBe('persistent');
  });

  test('PUT /workspaces/{id} updates workspace metadata', async ({ request }) => {
    const resp = await request.put(`${API}/workspaces/${persistentId}`, {
      data: {
        name: 'e2e-persistent-updated',
        description: 'Updated description',
        skill_ids: [],
      },
    });
    expect(resp.status()).toBe(200);
    const ws = await resp.json();
    expect(ws.name).toBe('e2e-persistent-updated');
    expect(ws.persistence).toBe('persistent'); // Immutable
  });

  // File browser tests on the persistent workspace
  test('GET /workspaces/{id}/files lists workspace files', async ({ request }) => {
    const resp = await request.get(`${API}/workspaces/${persistentId}/files`);
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body.files.length).toBeGreaterThanOrEqual(1);
    const paths = body.files.map((f: any) => f.path);
    expect(paths).toContain('.gitignore');
  });

  test('PUT /workspaces/{id}/files/{path} writes a file', async ({ request }) => {
    const resp = await request.put(
      `${API}/workspaces/${persistentId}/files/src/hello.rs`,
      { data: { content: 'fn main() { println!("hello"); }' } }
    );
    expect(resp.status()).toBe(204);

    // Verify it was written
    const read = await request.get(`${API}/workspaces/${persistentId}/files/src/hello.rs`);
    expect(read.status()).toBe(200);
    const file = await read.json();
    expect(file.content).toContain('println!');
  });

  test('DELETE /workspaces/{id}/files/{path} deletes a file', async ({ request }) => {
    const resp = await request.delete(`${API}/workspaces/${persistentId}/files/src/hello.rs`);
    expect(resp.status()).toBe(204);

    // Verify deleted
    const read = await request.get(`${API}/workspaces/${persistentId}/files/src/hello.rs`);
    expect(read.status()).toBe(404);
  });

  test('path traversal is blocked', async ({ request }) => {
    const resp = await request.get(
      `${API}/workspaces/${persistentId}/files/..%2F..%2F..%2Fetc%2Fpasswd`
    );
    expect(resp.status()).toBe(403);
  });

  // History tests
  test('GET /workspaces/{id}/history returns git log', async ({ request }) => {
    const resp = await request.get(`${API}/workspaces/${persistentId}/history`);
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body.commits.length).toBeGreaterThanOrEqual(1);
    expect(body.commits[0]).toHaveProperty('hash');
    expect(body.commits[0]).toHaveProperty('message');
    expect(body.commits[0]).toHaveProperty('date');
  });

  // Promote endpoint (snapshot workspace)
  test('POST /workspaces/{id}/promote works on snapshot workspace', async ({ request }) => {
    // Get current HEAD from history
    const history = await request.get(`${API}/workspaces/${snapshotId}/history`);
    const commits = (await history.json()).commits;
    const headHash = commits[0].hash;

    const resp = await request.post(
      `${API}/workspaces/${snapshotId}/promote?ref=${headHash}`
    );
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body.promoted).toBe(headHash);
    expect(body.tag).toBe('claw/base');
  });

  test('POST /workspaces/{id}/promote rejects non-snapshot workspace', async ({ request }) => {
    const resp = await request.post(
      `${API}/workspaces/${persistentId}/promote?ref=abc1234`
    );
    expect(resp.status()).toBe(400);
    const body = await resp.json();
    expect(body.error).toContain('snapshot');
  });

  // Sync endpoint
  test('POST /workspaces/{id}/sync rejects workspace without remote_url', async ({ request }) => {
    const resp = await request.post(`${API}/workspaces/${persistentId}/sync`);
    expect(resp.status()).toBe(400);
    const body = await resp.json();
    expect(body.error).toContain('no remote URL');
  });

  // Cleanup
  test('DELETE /workspaces/{id}?delete_files=true cleans up workspace', async ({ request }) => {
    for (const id of [persistentId, ephemeralId, snapshotId]) {
      if (!id) continue;
      const resp = await request.delete(`${API}/workspaces/${id}?delete_files=true`);
      expect(resp.status()).toBe(204);

      // Verify deleted
      const get = await request.get(`${API}/workspaces/${id}`);
      expect(get.status()).toBe(404);
    }
  });
});

// ============================================================
// Workspace with Remote URL
// ============================================================

test.describe('Workspace Remote Sync', () => {
  let wsId: string;

  test('create workspace with remote_url and sync', async ({ request }) => {
    const create = await request.post(`${API}/workspaces`, {
      data: {
        name: 'e2e-remote',
        persistence: 'persistent',
        remote_url: 'https://github.com/octocat/Hello-World.git',
      },
    });
    expect(create.status()).toBe(201);
    wsId = (await create.json()).id;

    // Sync from remote
    const sync = await request.post(`${API}/workspaces/${wsId}/sync`);
    expect(sync.status()).toBe(200);
    const syncBody = await sync.json();
    expect(syncBody.synced).toBe(true);

    // Verify remote files appeared
    const files = await request.get(`${API}/workspaces/${wsId}/files`);
    const fileList = (await files.json()).files;
    const paths = fileList.map((f: any) => f.path);
    expect(paths).toContain('README');
  });

  test('cleanup remote workspace', async ({ request }) => {
    if (wsId) {
      await request.delete(`${API}/workspaces/${wsId}?delete_files=true`);
    }
  });
});

// ============================================================
// Settings Screen UI
// ============================================================

test.describe('Settings Screen UI', () => {
  test('displays system health and execution sections', async ({ page }) => {
    await page.goto('/#/settings');
    await waitForFlutter(page, 7000);

    // Settings page renders with all sections visible in body text
    await expectBody(page, 'Settings page');
    await expectBody(page, 'System Health section');
    await expectBody(page, 'Execution Backend');

    // Execution backend selector has Local/Docker as radio buttons
    await expect(page.getByRole('radio', { name: 'Local' })).toBeVisible();
    await expect(page.getByRole('radio', { name: 'Docker' })).toBeVisible();
  });

  test('displays sandbox image with pull/build buttons', async ({ page }) => {
    await page.goto('/#/settings');
    await waitForFlutter(page, 7000);

    await expectBody(page, 'Sandbox Image section');
    await expectBody(page, 'Pull Image');
    await expectBody(page, 'Build Image');
  });

  test('displays resource limits and credential mounts', async ({ page }) => {
    await page.goto('/#/settings');
    await waitForFlutter(page, 7000);

    await expectBody(page, 'Default Resource Limits section');
    // Credential Mounts appears in merged ARIA header, check for its content instead
    await expectBody(page, 'Add Mount');
    await expectBody(page, 'Remove mount');

    // Verify form fields exist via ARIA labels
    await expect(page.getByLabel('Memory Limit')).toBeVisible();
    await expect(page.getByLabel('CPU Limit')).toBeVisible();
    await expect(page.getByLabel('PID Limit')).toBeVisible();
  });
});

// ============================================================
// Workspaces Screen UI
// ============================================================

test.describe('Workspaces Screen UI', () => {
  let wsId: string;

  test.beforeAll(async ({ request }) => {
    const resp = await request.post(`${API}/workspaces`, {
      data: {
        name: 'e2e-ui-workspace',
        description: 'For UI testing',
        persistence: 'persistent',
        claude_md: '# UI Test',
      },
    });
    wsId = (await resp.json()).id;
  });

  test('workspace list shows workspace with mode badge', async ({ page }) => {
    await page.goto('/#/workspaces');
    await waitForFlutter(page, 7000);

    // Workspace name via Semantics
    await expect(page.getByLabel('Workspace e2e-ui-workspace')).toBeVisible();
    // Persistence mode badge via Semantics
    await expect(page.getByLabel('Mode persistent')).toBeVisible();
  });

  test('create dialog shows persistence mode selector', async ({ page }) => {
    await page.goto('/#/workspaces');
    await waitForFlutter(page, 7000);

    await page.getByRole('button', { name: 'New Workspace' }).click();
    await waitForFlutter(page, 2000);

    // Persistence mode selector via Semantics
    await expect(page.getByLabel('Persistence mode selector')).toBeVisible();

    // Close dialog
    await page.getByRole('button', { name: 'Cancel' }).click();
  });

  test('workspace detail shows file browser and history', async ({ page }) => {
    await page.goto(`/#/workspaces/${wsId}`);
    await waitForFlutter(page, 7000);

    // Workspace name in body text (it's a heading)
    await expectBody(page, 'e2e-ui-workspace');
    // Persistence mode badge
    await expect(page.getByLabel('Persistence mode persistent')).toBeVisible();
    // File browser content
    await expectBody(page, 'Files');
    await expectBody(page, 'History');
  });

  test.afterAll(async ({ request }) => {
    if (wsId) {
      await request.delete(`${API}/workspaces/${wsId}?delete_files=true`);
    }
  });
});
