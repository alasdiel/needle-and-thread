// The phone app (crates/phone) at phone size, against a stand-in for GitHub: the tree and GraphQL
// APIs answer from fixtures/sample-vault, so nothing leaves the machine. Run with the phone app
// served by `trunk serve` in crates/phone (port 1421), or NEEDLE_PHONE_URL.
import { chromium, webkit } from 'playwright';
import { createHash } from 'node:crypto';
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';

const engineName = process.argv[2] === 'webkit' ? 'webkit' : 'chromium';
const engine = engineName === 'webkit' ? webkit : chromium;
const here = (f) => new URL(f, import.meta.url).pathname;
const vaultDir = here('../fixtures/sample-vault');

// The sample vault as GitHub would list it: every file with a blob sha.
const files = new Map();
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path);
    else {
      const text = readFileSync(path, 'utf8');
      const sha = createHash('sha1').update(`blob ${Buffer.byteLength(text)}\0${text}`).digest('hex');
      files.set(sha, { path: relative(vaultDir, path), text });
    }
  }
};
walk(vaultDir);

let failures = 0;
const check = (what, ok, detail = '') => {
  if (!ok) failures++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}${detail ? ` — ${detail}` : ''}`);
};

// A profile on disk, as a phone has: WebKit's throwaway profile forgets cached files on reload.
const profile = mkdtempSync(join(tmpdir(), 'needle-phone-'));
const context = await engine.launchPersistentContext(profile, { viewport: { width: 390, height: 844 }, hasTouch: true, colorScheme: 'dark' });
const page = context.pages()[0] ?? (await context.newPage());
const problems = [];
page.on('pageerror', (e) => problems.push(`page error: ${e.message}`));
page.on('console', (m) => {
  // The refused token is meant to fail; the browser logs that request itself.
  if (m.type() === 'error' && !m.text().includes('401')) problems.push(`console error: ${m.text().slice(0, 300)}`);
});

// --- The stand-in for GitHub --------------------------------------------------------------------
const calls = { tree: 0, graphql: 0, blobs: 0 };
let token = 'good';
const cors = { 'access-control-allow-origin': '*', 'access-control-allow-headers': '*' };
await context.route('https://api.github.com/**', async (route) => {
  const request = route.request();
  if (request.method() === 'OPTIONS') return route.fulfill({ status: 204, headers: { ...cors, 'access-control-allow-methods': 'GET, POST' } });
  if (request.headers()['authorization'] !== `Bearer ${token}`) return route.fulfill({ status: 401, headers: cors, body: '{}' });
  const url = new URL(request.url());
  if (url.pathname === '/repos/me/vault/git/trees/main') {
    calls.tree++;
    const tree = [...files].map(([sha, f]) => ({ path: f.path, type: 'blob', sha }));
    return route.fulfill({ headers: cors, contentType: 'application/json', body: JSON.stringify({ tree, truncated: false }) });
  }
  if (url.pathname === '/graphql') {
    calls.graphql++;
    const { query } = request.postDataJSON();
    const repository = {};
    for (const [, field, sha] of query.matchAll(/(f\d+): object\(oid: "([0-9a-f]+)"\)/g)) {
      calls.blobs++;
      repository[field] = { text: files.get(sha)?.text ?? null, isBinary: false, isTruncated: false };
    }
    return route.fulfill({ headers: cors, contentType: 'application/json', body: JSON.stringify({ data: { repository } }) });
  }
  return route.fulfill({ status: 404, headers: cors, body: '{}' });
});

const shot = (name) => page.screenshot({ path: here(`./shots/phone-${name}.png`) });
const settle = () => page.waitForTimeout(300);
const tap = (selector) => page.locator(selector).first().tap();

await page.goto(process.env.NEEDLE_PHONE_URL ?? 'http://127.0.0.1:1421/');
await page.waitForSelector('form', { timeout: 60000 });

// --- Setup ---------------------------------------------------------------------------------------
check('a new phone starts at setup', (await page.locator('h1').textContent()) === 'Needle and Thread');
await page.fill('#repo', 'me/vault');
await page.fill('#token', 'wrong');
await shot('1-setup');
await tap('button[type="submit"]');
await page.waitForSelector('.problem', { timeout: 10000 });
check('a token GitHub refuses says so', (await page.locator('.problem').textContent()).includes("didn't accept the token"));
await tap('.secondary');
await page.waitForSelector('form');
check('…and Change goes back with the repository kept', (await page.inputValue('#repo')) === 'me/vault');
await page.fill('#token', 'good');
await tap('button[type="submit"]');
await page.waitForSelector('.tabs', { timeout: 15000 });
check('with a good token, the outline opens', (await page.locator('.row.scene').count()) > 0);
check('it read the tree once and the files in one batch', calls.tree === 1 && calls.graphql === 1, JSON.stringify(calls));
check('only text files it needs were fetched', calls.blobs === [...files.values()].filter((f) => /\.(md|toml)$/.test(f.path)).length, JSON.stringify(calls));
check('the token is kept on the phone', await page.evaluate(() => localStorage.getItem('needle-phone-token') === 'good'));

// --- The outline ---------------------------------------------------------------------------------
check('the project is named at the top', (await page.locator('.project .title').textContent()).length > 0);
check('chapters are numbered', (await page.locator('.chapter .n').first().textContent()) === '1');
check('the night market is in it', (await page.locator('.row.scene', { hasText: 'The night market' }).count()) === 1);
check('it says where it read from', (await page.locator('.fresh').textContent()).includes('From GitHub'));
check('the Outline tab is current', (await page.locator('.tab.on').textContent()).includes('Outline'));
await shot('2-outline');

// --- Reading -------------------------------------------------------------------------------------
await page.locator('.row.scene', { hasText: 'The night market' }).tap();
await settle();
check('a scene opens to read', (await page.locator('h1.heading').textContent()) === 'The night market');
check('…with its text', (await page.locator('.prose').textContent()).includes('The market opened at dusk'));
check('…its piece number and chapter', /^piece \d+/i.test(await page.locator('.label').first().textContent()));
check('…and no tab bar, but steps to the scenes either side', (await page.locator('.tabs').count()) === 0 && (await page.locator('.steps').count()) === 1);
const size = async () => page.locator('.prose').evaluate((el) => getComputedStyle(el).fontSize);
const before = await size();
await tap('button[aria-label="Text size"]');
check('Aa makes the text bigger', parseFloat(await size()) > parseFloat(before), `${before} → ${await size()}`);
await shot('3-reading');

// --- A link --------------------------------------------------------------------------------------
await page.locator('.prose .wl', { hasText: 'Mara' }).first().tap();
await settle();
check('tapping a name opens its note as a sheet', (await page.locator('.sheet h2').textContent()) === 'Mara Venn');
check('…saying what it is', (await page.locator('.sheet .kind').textContent()).toLowerCase() === 'character');
check('…and the scenes it’s in', (await page.locator('.sheet .row.scene', { hasText: 'The night market' }).count()) === 1);
await shot('4-sheet');
await tap('.sheet .secondary');
await settle();
check('Open the whole note shows it as a page', (await page.locator('h1.heading').textContent()) === 'Mara Venn');
await tap('.back');
await settle();
check('Back returns to the scene', (await page.locator('h1.heading').textContent()) === 'The night market');
await page.locator('.scrim').count().then(async (n) => n && (await tap('.scrim')));

// Stepping along.
const nextTitle = await page.locator('.step.next .step-title').textContent().catch(() => null);
if (nextTitle) {
  await tap('.step.next');
  await settle();
  check('the next scene opens from the foot', (await page.locator('h1.heading').textContent()) === nextTitle);
}
await tap('.back');
await settle();
check('Back from reading returns to the outline', (await page.locator('.tabs').count()) === 1);

// --- Notes and search ------------------------------------------------------------------------
await page.locator('.tab', { hasText: 'Notes' }).tap();
await settle();
check('Notes lists them by kind', (await page.locator('.part').first().textContent()).toLowerCase().includes('character'));
check('…Old Teodor among them', (await page.locator('.row', { hasText: 'Old Teodor' }).count()) === 1);
await shot('5-notes');

await page.locator('.tab', { hasText: 'Search' }).tap();
await settle();
await page.fill('.searchbox input', 'compass');
await settle();
check('search finds a scene by its words', (await page.locator('.hit', { hasText: 'The night market' }).count()) === 1);
check('…with the word marked in context', (await page.locator('.hit mark').first().textContent()).toLowerCase() === 'compass');
await shot('6-search');

// --- Opening again ---------------------------------------------------------------------------
const graphqlBefore = calls.graphql;
await page.reload();
await page.waitForSelector('.tabs', { timeout: 15000 });
check('opening again needs no setup', (await page.locator('form').count()) === 0);
check('…and takes the files from the cache, not GitHub', calls.graphql === graphqlBefore, JSON.stringify(calls));

// --- Light theme -----------------------------------------------------------------------------
await page.emulateMedia({ colorScheme: 'light' });
await page.locator('.row.scene', { hasText: 'The night market' }).tap();
await settle();
await shot('7-reading-light');
await page.emulateMedia({ colorScheme: 'dark' });
await tap('.back');
await settle();

// --- Disconnecting ---------------------------------------------------------------------------
await tap('button[aria-label="Settings"]');
await settle();
check('settings name the repository', (await page.locator('.repo').textContent()) === 'me/vault');
await page.locator('.sheet .danger').tap();
await page.waitForSelector('form');
check('Disconnect goes back to setup', (await page.locator('form').count()) === 1);
check('…and forgets the token', await page.evaluate(() => localStorage.getItem('needle-phone-token') === null));

check('no page or console errors', problems.length === 0, problems.join(' | '));
console.log(`\n${failures === 0 ? 'all passed' : `${failures} failed`} (phone, ${engineName})`);
await context.close();
rmSync(profile, { recursive: true, force: true });
process.exitCode = failures === 0 ? 0 : 1;
