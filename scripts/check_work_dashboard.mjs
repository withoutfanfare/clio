// Actual WorkView and CLI storage; only the native Tauri transport is replaced.
// Requires built clio, installed UI dependencies and PLAYWRIGHT_MODULE.
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';
import { once } from 'node:events';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const temp = await mkdtemp(resolve(tmpdir(), 'clio-work-view-'));
const entry = `.work-proof-${process.pid}`;
const html = resolve(root, 'ui', `${entry}.html`);
const source = resolve(root, 'ui', `${entry}.ts`);
const { chromium } = await import(pathToFileURL(process.env.PLAYWRIGHT_MODULE));
const port = process.env.WORK_PROOF_PORT || '5179';
const url = `http://127.0.0.1:${port}/${entry}.html`;
const cli = (args, report) => JSON.parse(execFileSync(resolve(root, 'target/debug/clio'),
  ['--local', '--db-path', resolve(temp, 'test.sqlite'), '--json', 'work', ...args],
  { input: report ? JSON.stringify(report) : undefined, encoding: 'utf8', timeout: 10000 }));
const fixture = (run, project, state, actor) => ({ project, task: run, task_title: `Example ${run}`,
  source: 'browser-proof', session_id: `session-${run}`, run_id: run, sequence: 0,
  observed_at: Math.floor(Date.now()/1000), worktree: `fixture-tree-${run}`, revision: 'fixture',
  state, summary: `Example ${run} update`, next_step: actor === 'user' ? 'Review the result' : 'Continue scoped work',
  next_actor: actor, evidence: ['Disposable test evidence'], evidence_status: 'current', supersedes: null });
let server, browser;
let calls = 0, unavailable = false;
try {
  await writeFile(html, `<html class="dark"><head><meta name="viewport" content="width=device-width,initial-scale=1"></head><body><div id="app"></div><script type="module" src="/${entry}.ts"></script></body></html>`, { flag: 'wx' });
  await writeFile(source, `import { createApp } from 'vue'; import WorkView from './src/views/WorkView.vue'; import './src/style.css'; createApp(WorkView).mount('#app');`, { flag: 'wx' });
  server = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', port, '--strictPort'], { cwd: resolve(root, 'ui'), stdio: 'ignore' });
  for (let i = 0; i < 100; i++) {
    if (server.exitCode !== null) throw new Error('Vite exited before readiness');
    try { if ((await fetch(url)).ok) break; } catch {}
    await new Promise(r => setTimeout(r, 100));
  }
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 736, height: 850 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.exposeFunction('proofInvoke', (command) => {
    assert.equal(command, 'cmd_work_overview');
    calls++;
    if (unavailable) throw new Error('Isolated connection outage');
    return cli(['overview']);
  });
  await page.addInitScript(() => { window.__TAURI_INTERNALS__ = { invoke: command => window.proofInvoke(command) }; });
  await page.goto(url);
  await page.getByRole('heading', { name: 'No work reports yet' }).waitFor();
  const reports = [fixture('a', 'Sample project A', 'waiting', 'user'),
    fixture('b', 'Sample project A', 'implemented', 'user'), fixture('c', 'Sample project B', 'running', 'agent')];
  reports[2].observed_at -= 600;
  reports.forEach(report => cli(['report', '-'], report));
  // Wait for real automatic polling: no refresh button or artificial timer.
  await page.locator('.project-row').nth(1).waitFor({ timeout: 15000 });
  assert.ok(calls >= 2);
  assert.equal(await page.locator('.project-row').count(), 2);
  assert.equal(await page.locator('.project-row[open]').count(), 0);
  assert.match(await page.locator('.work-status').innerText(), /3 tasks.*2 projects.*2 need Danny/s);
  await page.locator('.project-summary').first().focus();
  await page.keyboard.press('Enter');
  assert.equal(await page.locator('.project-row[open]').count(), 1);
  await page.locator('.provenance summary').first().click();
  assert.ok(await page.getByText('fixture-tree-a', { exact: true }).isVisible());
  unavailable = true;
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await page.getByRole('alert').waitFor();
  assert.equal(await page.locator('.project-row').count(), 2);
  unavailable = false;
  cli(['report', '-'], { ...reports[2], sequence: 1, observed_at: Math.floor(Date.now()/1000), summary: 'Recovered current report' });
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await page.getByRole('alert').waitFor({ state: 'hidden' });
  assert.doesNotMatch(await page.locator('.work-status').innerText(), /uncertain/);
  for (const width of [736, 360]) {
    await page.setViewportSize({ width, height: 850 });
    for (const dark of [true, false]) {
      await page.evaluate(value => document.documentElement.classList.toggle('dark', value), dark);
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `Overflow at ${width}, dark=${dark}`);
    }
  }
  await page.setViewportSize({ width: 736, height: 650 });
  await page.evaluate(() => { document.documentElement.classList.add('dark'); document.querySelectorAll('details').forEach(el => el.open = false); });
  if (process.env.WORK_PROOF_SCREENSHOT) await page.screenshot({ path: process.env.WORK_PROOF_SCREENSHOT, fullPage: true });
  assert.deepEqual(errors, []);
  console.log('PASS: actual WorkView, CLI data, automatic refresh, drill-down, outage/recovery, 736/360px, light/dark');
} finally {
  if (browser) await browser.close();
  if (server && server.exitCode === null) { server.kill('SIGTERM'); await once(server, 'exit'); }
  await Promise.all([rm(html, { force: true }), rm(source, { force: true }), rm(temp, { force: true, recursive: true })]);
  console.log('CLEANUP: browser and Vite stopped; temporary UI files and database removed');
}
