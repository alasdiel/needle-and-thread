// Settling a clash (DESIGN §5, Sync): a paragraph changed here and on another device too shows
// the other version under this one, with Use this one, Keep both and Keep mine. tauri-mock.js
// keeps the clashes in window.__merges; take_in sets them from window.__incoming.merges.
import { openApp } from './harness.mjs';

const { page, check, shot, settle, calls, finish } = await openApp('merge');
const editor = page.locator('.ProseMirror');
await page.waitForFunction(() => document.querySelector('.ProseMirror')?.textContent.includes('The market opened at dusk.'), null, { timeout: 10000 });

const PATH = 'projects/tidewater/manuscript/night-market.md';
const BASE = 'The market opened at dusk.\n\nMara counted the coins.\n\nTeodor watched.\n';
const record = (mine, theirs, extra = {}) => ({
  stamp: '2026-10-10-1016', path: PATH, at: 1791627360, project: 'tidewater', scene: 'night-market',
  owner: null, world: false, note: null, base: BASE, theirs, mine, ...extra,
});
// Brings in `mine` as the open scene's text, with the clashes in `merges`.
const arrive = async (mine, merges) => {
  await page.evaluate(({ mine, merges }) => {
    window.__incoming = { text: mine, merges, clashes: merges.map((m) => ({ path: m.path, copy: `.needle/conflicts/${m.stamp}/${m.path}` })) };
    window.__backup.remote = 'git@github.com:me/novel.git';
    window.__backup.status = { state: 'incoming' };
    window.__emit('backup-status');
  }, { mine, merges });
  await settle();
  await settle();
  await settle();
};
const text = () => editor.textContent();
const left = () => page.evaluate(() => window.__merges.length);

// --- Use this one ------------------------------------------------------------------------------
const MINE = 'The market opened at dusk.\n\nMara counted the coins at the desk.\n\nTeodor watched.\n';
const THEIRS = 'The market opened at dusk.\n\nMara counted the coins on the phone.\n\nTeodor watched.\n';
await arrive(MINE, [record(MINE, THEIRS)]);
check('the notice says both versions wait', (await page.locator('.notice').textContent().catch(() => '')).includes('need merging'));
check('a banner says one paragraph here changed elsewhere too', (await page.locator('.merge-banner').textContent().catch(() => '')).includes('One paragraph here'));
check('the other version shows under this one', (await page.locator('.merge-other').count()) === 1);
check('…saying where it came from', (await page.locator('.merge-when').textContent()).includes('The other device'));
const marked = async (tag) => (await page.locator(`.merge-other ${tag}`).allTextContents()).join(' ');
check('…with what it added marked', (await marked('ins')) === 'on phone.', await marked('ins'));
check('…and what it took out', (await marked('del')) === 'at desk.', await marked('del'));
check('…and the rest as it reads', (await page.locator('.merge-text').textContent()) === 'Mara counted the coins atondesk.phone.' || (await page.locator('.merge-text').textContent()).includes('the'));
check('this side’s paragraph is marked', (await page.locator('.merge-mine').textContent()).includes('at the desk'));
check('the box sits right after it', await page.evaluate(() => document.querySelector('.merge-mine')?.nextElementSibling?.classList.contains('merge-other')));
check('“1 to merge” shows by Saved', (await page.locator('.merge-count').textContent().catch(() => '')).includes('1 to merge'));
check('the scene has a dot in the outline', (await page.locator('.scene-row .merge-dot').count()) === 1);
await shot('1-clash');

await page.click('.merge-actions button[data-choice="theirs"]');
await settle();
check('Use this one puts theirs in the text', (await text()).includes('on the phone') && !(await text()).includes('at the desk'), await text());
check('…and the box, banner, count and dot go', (await page.locator('.merge-other, .merge-banner, .merge-count, .scene-row .merge-dot').count()) === 0);
check('…and nothing is left to settle', (await left()) === 0);
const settled = (await calls('settle_merge')).at(-1);
check('…so the kept versions are removed', settled && settled.args.base == null, JSON.stringify(settled?.args));
await page.waitForTimeout(1300);
check('the text is saved', (await calls('save_scene')).some((c) => c.args.markdown.includes('on the phone')));
await shot('2-used');

// --- Keep both ---------------------------------------------------------------------------------
await arrive(MINE, [record(MINE, THEIRS)]);
await page.click('.merge-actions button[data-choice="both"]');
await settle();
const both = await text();
check('Keep both keeps mine, then theirs after it', both.indexOf('at the desk') >= 0 && both.indexOf('at the desk') < both.indexOf('on the phone'), both);
check('…and settles it', (await left()) === 0 && (await page.locator('.merge-other').count()) === 0);

// --- Keep mine, and two at once ----------------------------------------------------------------
// Apart, so they're two clashes; neighbouring ones would be one.
const MINE2 = 'The market opened at noon.\n\nMara counted the coins.\n\nTeodor watched from the desk.\n';
const THEIRS2 = 'The market opened at dawn.\n\nMara counted the coins.\n\nTeodor watched from the phone.\n';
await arrive(MINE2, [record(MINE2, THEIRS2)]);
check('two clashes show two boxes', (await page.locator('.merge-other').count()) === 2);
check('…and the banner counts them', (await page.locator('.merge-banner').textContent()).includes('2 places'));
await page.click('.merge-banner button');
await settle();
check('Next moves the cursor into the first', await page.evaluate(() => getSelection()?.anchorNode?.parentElement?.closest('.merge-mine, p')?.textContent.includes('at noon')));
await page.locator('.merge-other').first().locator('button[data-choice="mine"]').click();
await settle();
check('Keep mine leaves the text alone', (await text()).includes('at noon') && (await text()).includes('from the desk'));
check('…and only the other clash is left', (await page.locator('.merge-other').count()) === 1 && (await left()) === 1);
const kept = (await calls('settle_merge')).at(-1);
check('…with the kept versions brought into line', kept?.args.base?.includes('at noon') && kept?.args.theirs?.includes('at noon'), JSON.stringify(kept?.args));
await editor.click();
await page.keyboard.press('Home');
await page.keyboard.type('So ');
await page.waitForTimeout(1300);
check('typing in a settled paragraph doesn’t bring the clash back', (await page.locator('.merge-other').count()) === 1);
await page.locator('.merge-other button[data-choice="theirs"]').click();
await settle();
check('settling the last one clears everything', (await left()) === 0 && (await page.locator('.merge-banner').count()) === 0);

// --- A clash in a scene that isn't open --------------------------------------------------------
const OTHER = { ...record('A.\n', 'C.\n'), path: 'projects/tidewater/manuscript/long-chapter.md', scene: 'long-chapter', base: 'B.\n' };
await page.evaluate((other) => { window.__merges = [other]; }, OTHER);
await arrive('The market opened at dusk.\n', [OTHER]);
check('a clash elsewhere shows in the count', (await page.locator('.merge-count').textContent().catch(() => '')).includes('1 to merge'));
check('…and puts a dot on that scene', (await page.locator('.scene-row', { hasText: 'The long night' }).locator('.merge-dot').count()) === 1);
check('…but nothing in the open scene', (await page.locator('.merge-other, .merge-banner').count()) === 0);
await shot('3-elsewhere');

process.exitCode = (await finish()) === 0 ? 0 : 1;
