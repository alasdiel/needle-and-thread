// What every suite starts from: a browser on `trunk serve` (or NEEDLE_URL) with tauri-mock.js
// standing in for the backend, and the network board open. `check` reports each step and
// counts what failed, so one broken piece doesn't hide the rest; `finish` reports any page or
// console errors and exits non-zero if anything failed.
import { chromium, webkit } from 'playwright';
import { readFileSync } from 'node:fs';

export async function openBoard(suite) {
  const engineName = process.argv[2] === 'webkit' ? 'webkit' : 'chromium';
  const engine = engineName === 'webkit' ? webkit : chromium;
  const here = (f) => new URL(f, import.meta.url).pathname;
  const browser = await engine.launch();
  const page = await browser.newPage({ viewport: { width: 946, height: 1030 } });
  await page.addInitScript({ content: readFileSync(here('./tauri-mock.js'), 'utf8') });

  const problems = [];
  page.on('pageerror', (e) => problems.push(`page error: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error') problems.push(`console error: ${m.text().slice(0, 300)}`);
  });

  let failures = 0;
  const check = (what, ok, detail = '') => {
    if (!ok) failures++;
    console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}${detail ? ` — ${detail}` : ''}`);
  };

  await page.goto(process.env.NEEDLE_URL ?? 'http://127.0.0.1:1420/');
  await page.waitForSelector('[title="Network board"]', { timeout: 60000 });
  await page.click('[title="Network board"]');
  await page.waitForSelector('.board [data-card]', { timeout: 10000 });
  await page.waitForTimeout(250);

  return {
    page,
    check,
    shot: (name) => page.screenshot({ path: here(`./shots/${suite}-${name}.png`) }),
    saves: () => page.evaluate(() => window.__saves),
    calls: (cmd) => page.evaluate((cmd) => window.__calls.filter((c) => c.cmd === cmd), cmd),
    settle: () => page.waitForTimeout(250),
    finish: async () => {
      check('no page or console errors', problems.length === 0, problems.join(' | '));
      console.log(`\n${failures === 0 ? 'all passed' : `${failures} failed`} (${suite}, ${engineName})`);
      await browser.close();
      return failures;
    },
  };
}
