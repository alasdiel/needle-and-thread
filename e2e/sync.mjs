// Snapshots made elsewhere coming in (DESIGN §5, Sync): when the backup says there are some, the
// window takes them in and shows the open scene as it now is. tauri-mock.js's take_in brings in
// whatever a step puts in window.__incoming. See harness.mjs for how a suite runs.
import { openApp } from './harness.mjs';

const { page, check, shot, settle, calls, finish } = await openApp('sync');
const editor = page.locator('.ProseMirror');
await page.waitForFunction(() => document.querySelector('.ProseMirror')?.textContent.includes('The market opened at dusk.'), null, { timeout: 10000 });
const notice = () => page.locator('.notice').textContent().catch(() => '');
// The backup finds snapshots made elsewhere; `incoming` is what taking them in brings.
const arrive = async (incoming) => {
  await page.evaluate((incoming) => {
    window.__incoming = incoming;
    window.__backup.remote = 'git@github.com:me/novel.git';
    window.__backup.status = { state: 'incoming' };
    window.__emit('backup-status');
  }, incoming);
  await settle();
  await settle();
};

await arrive({});
check('the window takes in what the backup found', (await calls('take_in')).length === 1);
check('…and with nothing changed, the scene stays as it was', (await editor.textContent()).includes('The market opened at dusk.'));
check('…and nothing is said about it', (await page.locator('.notice').count()) === 0);

const outlines = (await calls('project_outline')).length;
await arrive({ text: 'Written on the phone.' });
check('it takes in again when more arrives', (await calls('take_in')).length === 2);
check('the open scene shows what came in', (await editor.textContent()).includes('Written on the phone.'));
check('…with a note saying why', (await notice()).includes('changed on another device'), await notice());
check('the outline is read again', (await calls('project_outline')).length > outlines);
await shot('1-changed');

await arrive({ text: 'Written on the phone, then here.', clashes: [{ path: 'projects/tidewater/manuscript/night-market.md', copy: '.needle/conflicts/2026-10-10-0941/projects/tidewater/manuscript/night-market.md' }] });
check('a clash says yours was kept', (await notice()).includes('yours was kept'), await notice());
await shot('2-clash');

process.exitCode = (await finish()) === 0 ? 0 : 1;
