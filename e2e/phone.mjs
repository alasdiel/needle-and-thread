// The phone app (crates/phone) at phone size, against a stand-in for GitHub: the tree and GraphQL
// APIs answer from fixtures/sample-vault, and commits made with createCommitOnBranch change it,
// so nothing leaves the machine. Run with the phone app served by `trunk serve` in crates/phone
// (port 1421), or NEEDLE_PHONE_URL.
import { chromium, webkit } from 'playwright';
import { createHash } from 'node:crypto';
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';

const engineName = process.argv[2] === 'webkit' ? 'webkit' : 'chromium';
const engine = engineName === 'webkit' ? webkit : chromium;
const here = (f) => new URL(f, import.meta.url).pathname;
const vaultDir = here('../fixtures/sample-vault');

// The sample vault as GitHub holds it: the branch's files by path, and every blob by sha.
const blobSha = (text) => createHash('sha1').update(`blob ${Buffer.byteLength(text)}\0${text}`).digest('hex');
const repo = new Map();
const blobs = new Map();
const put = (path, text) => {
  const sha = blobSha(text);
  repo.set(path, sha);
  blobs.set(sha, text);
};
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path);
    else put(relative(vaultDir, path), readFileSync(path, 'utf8'));
  }
};
walk(vaultDir);
// A link made with Share on the desktop, which Settings offers to open as the author.
put('.needle/shares.toml', `[[share]]\nid = "AQEBAQEBAQEBAQEBAQEBAQ"\nkey = "${Buffer.alloc(32, 2).toString('base64url')}"\nauthor = "${Buffer.alloc(32, 3).toString('base64url')}"\nname = "Beta readers"\nproject = "tidewater"\ncovers = "book"\ncreated = 1791627360\n`);
const textAt = (path) => blobs.get(repo.get(path));
let head = 'c0';
const commits = [];
// What the desktop does meanwhile: a commit of its own, moving the branch on.
const commitElsewhere = (path, text) => {
  put(path, text);
  head = `c${commits.length + 1}-desktop`;
  commits.push({ message: 'Edited on the desktop', paths: [path], elsewhere: true });
};

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
// While GitHub is cut off on purpose, the browser logs the failed requests itself.
let quiet = false;
page.on('pageerror', (e) => problems.push(`page error: ${e.message}`));
page.on('console', (m) => {
  // The refused token is meant to fail; the browser logs that request itself.
  if (m.type() === 'error' && !m.text().includes('401') && !quiet) problems.push(`console error: ${m.text().slice(0, 300)}`);
});

// --- The stand-in for GitHub --------------------------------------------------------------------
const calls = { tree: 0, graphql: 0, blobs: 0 };
let token = 'good';
// A token for reading only: GitHub refuses its writes.
const readOnly = 'reader';
let offline = false;
const cors = { 'access-control-allow-origin': '*', 'access-control-allow-headers': '*' };
await context.route('https://api.github.com/**', async (route) => {
  const request = route.request();
  if (offline) return route.abort('internetdisconnected');
  if (request.method() === 'OPTIONS') return route.fulfill({ status: 204, headers: { ...cors, 'access-control-allow-methods': 'GET, POST' } });
  const auth = request.headers()['authorization'];
  if (auth !== `Bearer ${token}` && auth !== `Bearer ${readOnly}`) return route.fulfill({ status: 401, headers: cors, body: '{}' });
  const json = (body) => route.fulfill({ headers: cors, contentType: 'application/json', body: JSON.stringify(body) });
  const url = new URL(request.url());
  if (url.pathname === '/repos/me/vault/git/trees/main') {
    calls.tree++;
    const tree = [...repo].map(([path, sha]) => ({ path, type: 'blob', sha }));
    return json({ tree, truncated: false });
  }
  if (url.pathname === '/graphql') {
    calls.graphql++;
    const { query, variables } = request.postDataJSON();
    if (query.startsWith('mutation')) {
      if (auth === `Bearer ${readOnly}`) return json({ errors: [{ type: 'FORBIDDEN', message: 'Resource not accessible by personal access token' }] });
      const input = variables.input;
      if (input.expectedHeadOid !== head) return json({ errors: [{ message: `Expected branch to point to "${input.expectedHeadOid}" but it did not. Pull and try again.` }] });
      for (const { path, contents } of input.fileChanges.additions) put(path, Buffer.from(contents, 'base64').toString('utf8'));
      head = `c${commits.length + 1}`;
      commits.push({ message: input.message.headline, paths: input.fileChanges.additions.map((a) => a.path) });
      const commit = { oid: head };
      for (const [, field, path] of query.matchAll(/(f\d+): file\(path: ("(?:[^"\\]|\\.)*")\)/g)) commit[field] = { oid: repo.get(JSON.parse(path)) };
      return json({ data: { createCommitOnBranch: { commit } } });
    }
    const repository = {};
    for (const [, field, sha] of query.matchAll(/(f\d+): object\(oid: "([0-9a-f]+)"\)/g)) {
      calls.blobs++;
      repository[field] = { text: blobs.get(sha) ?? null, isBinary: false, isTruncated: false };
    }
    if (query.includes('ref(qualifiedName')) repository.ref = { target: { oid: head } };
    for (const [, field, expression] of query.matchAll(/(f\d+): object\(expression: ("(?:[^"\\]|\\.)*")\)/g)) {
      const sha = repo.get(JSON.parse(expression).replace(/^main:/, ''));
      repository[field] = sha ? { oid: sha, text: blobs.get(sha), isTruncated: false } : null;
    }
    return json({ data: { repository } });
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
check('only text files it needs were fetched', calls.blobs === [...repo.keys()].filter((path) => /\.(md|toml)$/.test(path)).length, JSON.stringify(calls));
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

// --- Writing (the canvas's Phone writing page) -------------------------------------------------
const SCENE = 'projects/tidewater/manuscript/night-market.md';
const NOTE = 'projects/tidewater/notes/characters/old-teodor.md';
const editor = '.writing .needle-prose';
const DRAFTS = 'needle-phone-drafts:me/vault';
const drafts = () => page.evaluate((key) => localStorage.getItem(key) ?? '', DRAFTS);
const until = async (cond, ms = 8000) => {
  for (let t = 0; t < ms; t += 100) {
    if (await cond()) return true;
    await page.waitForTimeout(100);
  }
  return false;
};
const openScene = async () => {
  await page.locator('.row.scene', { hasText: 'The night market' }).tap();
  await settle();
};
const write = async () => {
  await tap('.write');
  await page.waitForSelector(editor);
  await settle();
};
// Puts the cursor at the end of the text, or selects `words` in it.
const caretAtEnd = () =>
  page.evaluate((sel) => {
    const el = document.querySelector(sel);
    el.focus();
    const range = document.createRange();
    range.selectNodeContents(el.lastElementChild);
    range.collapse(false);
    getSelection().removeAllRanges();
    getSelection().addRange(range);
  }, editor);
const select = (words) =>
  page.evaluate(
    ([sel, words]) => {
      const el = document.querySelector(sel);
      el.focus();
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const at = node.data.indexOf(words);
        if (at < 0) continue;
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + words.length);
        getSelection().removeAllRanges();
        getSelection().addRange(range);
        return true;
      }
      return false;
    },
    [editor, words],
  );
// The editor takes in typing after a pause of a second.
const pause = () => page.waitForTimeout(1400);
const putAway = (away) =>
  page.evaluate((away) => {
    Object.defineProperty(document, 'visibilityState', { value: away ? 'hidden' : 'visible', configurable: true });
    document.dispatchEvent(new Event('visibilitychange'));
  }, away);

await openScene();
check('a scene being read offers Write', (await page.locator('.write').count()) === 1);
await write();
check('Write turns the page into the editor', (await page.locator(editor).getAttribute('contenteditable')) === 'true');
check('…with the same text', (await page.locator(editor).textContent()).includes('The market opened at dusk'));
check('…the bar of formats, and Done', (await page.locator('.fmt .wb').count()) === 8 && (await page.locator('.done').count()) === 1);
check("…and the keyboard's own spellcheck", (await page.locator(editor).getAttribute('spellcheck')) === 'true');
await caretAtEnd();
await page.keyboard.type(' She did not look back.');
await pause();
check('a pause keeps it on the phone', (await page.locator('.saved').textContent()) === 'Saved on this phone');
check('…as a draft that outlives the page', (await drafts()).includes('She did not look back.'));
check('…and nothing goes to GitHub yet', commits.length === 0, JSON.stringify(commits));
await shot('8-writing');
await tap('.done');
check('Done sends it to GitHub', await until(() => commits.length === 1), JSON.stringify(commits));
check('…as one commit, worded like the desktop’s snapshots', commits[0]?.message === 'Edited night-market on the phone (+5 words)', commits[0]?.message);
check('…with the new words in the file', textAt(SCENE).includes('She did not look back.'));
check('…and back to reading them', (await page.locator('.writing').count()) === 0 && (await page.locator('.prose').textContent()).includes('She did not look back.'));
check('…with the draft gone', await until(async () => (await drafts()) === ''));

// Formats from the bar.
await write();
check('the line says it’s on GitHub', (await page.locator('.saved').textContent()) === 'On GitHub · just now');
await select('compass');
await tap('.wb[aria-label="Italic"]');
check('Italic makes the selected words italic', (await page.locator(`${editor} em`, { hasText: 'compass' }).count()) === 1);
check('…shows as on', (await page.locator('.wb[aria-label="Italic"]').getAttribute('class')).includes('on'));
check('…and leaves the cursor in the text', await page.evaluate((sel) => document.activeElement === document.querySelector(sel), editor));

// Linking from the bar (Format 1b).
await caretAtEnd();
await tap('.wb[aria-label="Link to a note"]');
await page.keyboard.type('Teo');
await page.waitForSelector('.chips .chip', { timeout: 5000 }).catch(() => {});
check('[[ turns the bar into notes to link', (await page.locator('.chips .chip').first().textContent().catch(() => '')).includes('Teodor'));
await shot('9-linking');
const linksBefore = await page.locator(`${editor} .wikilink`).count();
await page.locator('.chips .chip').first().tap();
check('tapping one links it', (await page.locator(`${editor} .wikilink`).count()) === linksBefore + 1);
check('…and the formats come back', (await page.locator('.fmt .wb').count()) === 8);

// The desktop changed another paragraph meanwhile: the two merge.
await pause();
commitElsewhere(SCENE, textAt(SCENE).replace('The market opened at dusk,', 'The market opened at dusk that day,'));
await tap('.done');
check('a change made elsewhere meanwhile is merged in', await until(() => commits.length === 3 && textAt(SCENE).includes('that day')), JSON.stringify(commits));
check('…keeping the phone’s', textAt(SCENE).includes('*compass*') && textAt(SCENE).includes('[[Old Teodor|Teodor]]'), textAt(SCENE).slice(-200));
check('…and the page reads both', (await page.locator('.prose').textContent()).includes('that day'));

// Both changed the same paragraph: the phone's stays, the desktop's is kept to settle.
await write();
commitElsewhere(SCENE, textAt(SCENE).replace('“You came,” he said.', '“You came at last,” he said.'));
await select('You came,');
await page.keyboard.type('You came back,');
await pause();
await tap('.done');
check('a clash is sent', await until(() => commits.length === 5), JSON.stringify(commits));
check('…with the phone’s version in the scene', textAt(SCENE).includes('“You came back,” he said.'));
const kept = [...repo.keys()].find((path) => path.startsWith('.needle/conflicts/') && path.endsWith(SCENE) && !path.includes('/.base/'));
check('…the desktop’s kept to settle there', kept && textAt(kept).includes('“You came at last,” he said.'), kept);
check('…saying so in the commit', commits[4]?.message.endsWith('· 1 file to merge by hand'), commits[4]?.message);
check('…and on the phone', (await page.locator('.notice').textContent().catch(() => '')).includes('settle it on the desktop'));
await shot('10-clash');
await tap('.notice .unalias');

// No connection: kept on the phone, and sent when it's back.
await write();
await caretAtEnd();
await page.keyboard.type(' The tide turned.');
await pause();
offline = quiet = true;
await putAway(true);
check('with no connection it says so', await until(async () => (await page.locator('.strip').textContent().catch(() => '')).includes('no connection')));
check('…and the line says not sent', (await page.locator('.saved').textContent()) === 'Not sent');
await shot('11-offline');
offline = false;
await putAway(false);
await page.evaluate(() => window.dispatchEvent(new Event('online')));
check('back online, it goes', await until(() => textAt(SCENE).includes('The tide turned.')), JSON.stringify(commits.slice(-1)));
quiet = false;
check('…and the strip goes', await until(async () => (await page.locator('.strip').count()) === 0));

// Closed before it was sent: the draft goes when the app opens again.
await caretAtEnd();
await page.keyboard.type(' Gulls.');
await pause();
await page.reload();
await page.waitForSelector('.tabs', { timeout: 15000 });
check('a draft left on the phone goes when it opens again', await until(() => textAt(SCENE).includes('Gulls.')));

// A token that can only read.
await tap('button[aria-label="Settings"]');
await settle();
await page.locator('.sheet .secondary', { hasText: 'Use a different token' }).tap();
await page.waitForSelector('form');
check('a different token keeps the repository', (await page.inputValue('#repo')) === 'me/vault');
await page.fill('#token', readOnly);
await tap('button[type="submit"]');
await page.waitForSelector('.tabs', { timeout: 15000 });
await openScene();
await write();
await caretAtEnd();
await page.keyboard.type(' Rain.');
await pause();
await tap('.wb[aria-label="Hide keyboard"]');
await putAway(true);
check('a token that can only read says so', await until(async () => (await page.locator('.strip').textContent().catch(() => '')).includes('can only read')));
await putAway(false);
await tap('.back');
await settle();
check('Back from writing goes where reading would', (await page.locator('.tabs').count()) === 1);
await tap('button[aria-label="Settings"]');
await settle();
check('Settings warn that it isn’t sent', (await page.locator('.sheet').textContent()).includes('isn’t on GitHub yet'));
await page.locator('.sheet .secondary', { hasText: 'Use a different token' }).tap();
await page.waitForSelector('form');
await page.fill('#token', 'good');
await tap('button[type="submit"]');
await page.waitForSelector('.tabs', { timeout: 15000 });
check('with one that can write, it goes', await until(() => textAt(SCENE).includes('Rain.')));

// A note: its text, its other names and its title.
await page.locator('.tab', { hasText: 'Notes' }).tap();
await settle();
await page.locator('.row', { hasText: 'Old Teodor' }).tap();
await settle();
await write();
check('a note opens to write with its title', (await page.inputValue('.titlefield')) === 'Old Teodor');
await caretAtEnd();
await page.keyboard.type(' He never asks twice.');
await tap('.alias.add');
await page.keyboard.type('the instrument-maker');
await page.keyboard.press('Enter');
await settle();
check('another name can be added', (await page.locator('.alias', { hasText: 'the instrument-maker' }).count()) === 1);
await page.fill('.titlefield', 'Teodor Brask');
await page.locator('.titlefield').dispatchEvent('change');
await pause();
await shot('12-note');
const commitsBefore = commits.length;
await tap('.done');
check('a note’s changes go in one commit', await until(() => commits.length === commitsBefore + 1), JSON.stringify(commits.slice(commitsBefore)));
check('…its text and names', textAt(NOTE).includes('He never asks twice.') && textAt(NOTE).includes('the instrument-maker') && textAt(NOTE).includes('title = "Teodor Brask"'), textAt(NOTE));
check('…and the links to it, which keep their words', textAt(SCENE).includes('[[Teodor Brask|Old Teodor]]') && commits.at(-1).paths.includes(SCENE), JSON.stringify(commits.at(-1)));
await tap('.back');
await settle();

// --- Disconnecting ---------------------------------------------------------------------------
await tap('button[aria-label="Settings"]');
await settle();
check('settings name the repository', (await page.locator('.repo').textContent()) === 'me/vault');
check('…and list the links shared from the desktop', (await page.locator('.sheet .shared-link').textContent()).includes('Beta readers · Tidewater'));
const authorLink = await page.locator('.sheet .shared-link').getAttribute('href');
check('…each opening the reader page as the author', authorLink === `read/#AQEBAQEBAQEBAQEBAQEBAQ.${Buffer.alloc(32, 2).toString('base64url')}.${Buffer.alloc(32, 3).toString('base64url')}`, authorLink);
await page.locator('.sheet .danger').tap();
await page.waitForSelector('form');
check('Disconnect goes back to setup', (await page.locator('form').count()) === 1);
check('…and forgets the token', await page.evaluate(() => localStorage.getItem('needle-phone-token') === null));

check('no page or console errors', problems.length === 0, problems.join(' | '));
console.log(`\n${failures === 0 ? 'all passed' : `${failures} failed`} (phone, ${engineName})`);
await context.close();
rmSync(profile, { recursive: true, force: true });
process.exitCode = failures === 0 ? 0 : 1;
