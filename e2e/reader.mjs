// The reader page (crates/reader) at phone size, against a real Worker run by `wrangler dev`
// (crates/worker, port 8787, or NEEDLE_WORKER). A share is made here the way the desktop makes
// one: sealed with Node's crypto, which seals exactly as needle_core::share does (AES-256-GCM,
// nonce then ciphertext and tag, the label as associated data). Run with the reader served by
// `trunk serve` in crates/reader (port 1422, or NEEDLE_READER_URL).
import { chromium, webkit } from 'playwright';
import { createCipheriv, createDecipheriv, createHash, randomBytes } from 'node:crypto';

const engineName = process.argv[2] === 'webkit' ? 'webkit' : 'chromium';
const engine = engineName === 'webkit' ? webkit : chromium;
const here = (f) => new URL(f, import.meta.url).pathname;
const worker = process.env.NEEDLE_WORKER ?? 'http://localhost:8787';
const readerUrl = process.env.NEEDLE_READER_URL ?? 'http://127.0.0.1:1422/';

let failures = 0;
const check = (what, ok, detail = '') => {
  if (!ok) failures++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}${detail ? ` — ${detail}` : ''}`);
};

// --- Sealing, as core does -----------------------------------------------------------------------
const b64 = (bytes) => Buffer.from(bytes).toString('base64url');
const seal = (key, label, plain) => {
  const nonce = randomBytes(12);
  const cipher = createCipheriv('aes-256-gcm', key, nonce);
  cipher.setAAD(Buffer.from(label));
  const body = Buffer.concat([cipher.update(plain), cipher.final(), cipher.getAuthTag()]);
  return Buffer.concat([nonce, body]);
};
const open = (key, label, sealed) => {
  const decipher = createDecipheriv('aes-256-gcm', key, sealed.subarray(0, 12));
  decipher.setAAD(Buffer.from(label));
  decipher.setAuthTag(sealed.subarray(sealed.length - 16));
  return Buffer.concat([decipher.update(sealed.subarray(12, sealed.length - 16)), decipher.final()]);
};
const commentToken = (key) => b64(createHash('sha256').update(Buffer.concat([Buffer.from('needle-and-thread comment token\0'), key])).digest());
const fingerprint = (s) => createHash('sha256').update(s).digest('hex');

const id = b64(randomBytes(16));
const key = randomBytes(32);
const author = b64(randomBytes(32));
const textLabel = `needle-and-thread text ${id}`;
const commentLabel = `needle-and-thread comment ${id}`;

const shared = {
  v: 1,
  project: 'Tidewater',
  title: 'Arrival',
  parts: [
    { chapter: 'Arrival', scene: 'night-market', title: 'The night market', markdown: 'The market opened at dusk, and nobody asked why the lanterns were lit before the bells.\n\nMara walked the length of it twice, *counting stalls*, and once more because her feet would not stop.\n' },
    { scene: 'counting-house', title: 'Counting house', markdown: 'The ledger had left port.\n' },
  ],
  updated: 1791627360,
  version: 'v1',
  moved: {},
};
const upload = () =>
  fetch(`${worker}/s/${id}`, {
    method: 'PUT',
    headers: { Authorization: `Bearer ${author}`, 'X-Commenter': fingerprint(commentToken(key)) },
    body: seal(key, textLabel, Buffer.from(JSON.stringify(shared))),
  });
check('the share is uploaded to the Worker', (await upload()).status === 204);
const postComment = async (comment, asAuthor = false) => {
  const headers = asAuthor ? { Authorization: `Bearer ${author}` } : { 'X-Comment-Token': commentToken(key) };
  const r = await fetch(`${worker}/s/${id}/comments`, { method: 'POST', headers, body: seal(key, commentLabel, Buffer.from(JSON.stringify(comment))) });
  return r.json();
};
await postComment({ v: 1, name: 'June', text: 'Why before? A festival night?', on: { scene: 'night-market', quote: 'lit before the bells', n: 0 }, version: 'v1' });
const cut = await postComment({ v: 1, name: 'June', text: 'This bit went.', on: { scene: 'night-market', quote: 'a cut sentence', n: 0 }, version: 'v1' });
// The author's app has since found that one's words gone, and says so with the next upload.
shared.moved[cut.id] = null;
await upload();
const listComments = async () => {
  const listed = await (await fetch(`${worker}/s/${id}/comments`)).json();
  return listed.filter((c) => !c.deleted).map((c) => ({ ...c, comment: JSON.parse(open(key, commentLabel, Buffer.from(c.body, 'base64url')).toString()) }));
};

// --- The page ------------------------------------------------------------------------------------
const browser = await engine.launch();
const context = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true, colorScheme: 'dark' });
const page = await context.newPage();
const problems = [];
page.on('pageerror', (e) => problems.push(`page error: ${e.message}`));
page.on('console', (m) => {
  // A link that doesn't exist is meant to get a 404; the browser logs that request itself.
  if (m.type() === 'error' && !m.text().includes('404')) problems.push(`console error: ${m.text().slice(0, 300)}`);
});
const shot = (name) => page.screenshot({ path: here(`./shots/reader-${name}.png`) });
const settle = () => page.waitForTimeout(300);
const address = (fragment) => `${readerUrl}?worker=${encodeURIComponent(worker)}#${fragment}`;

/** Selects `words` in the open scene, as a reader dragging over them would. */
const select = (words) =>
  page.evaluate((words) => {
    const prose = document.querySelector('.prose');
    const walker = document.createTreeWalker(prose, NodeFilter.SHOW_TEXT);
    const nodes = [];
    let text = '';
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      nodes.push([n, text.length]);
      text += n.data;
    }
    const at = text.indexOf(words);
    const point = (i) => {
      const [node, start] = nodes.filter(([, s]) => s <= i).pop();
      return [node, i - start];
    };
    const range = document.createRange();
    range.setStart(...point(at));
    range.setEnd(...point(at + words.length));
    const selection = getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  }, words);

await page.goto(address(`${id}.${b64(key)}`));
await page.waitForSelector('.prose span', { timeout: 60000 });
check('the shared text opens', (await page.locator('h1.heading').textContent()) === 'The night market');
check('…under its chapter', (await page.locator('main .label').first().textContent()) === 'Arrival');
check('…named at the top', (await page.locator('.project .label').textContent()).includes('Tidewater · Arrival'));
check('…with its words', (await page.locator('.prose').textContent()).includes('Mara walked the length of it twice'));
await page.waitForSelector('mark.cm', { timeout: 10000 });
check('a commented phrase is marked', (await page.locator('mark.cm').textContent()) === 'lit before the bells');
check('…and the count says two open comments, counting the one whose words were cut', (await page.locator('.icon.count').textContent()).trim() === '2');
check('no author tools for a reader', (await page.locator('.author-pill').count()) === 0);
await shot('1-reading');

await page.locator('mark.cm').tap();
await settle();
check('tapping marked words opens their thread', (await page.locator('.sheet .said').first().textContent()) === 'Why before? A festival night?');
check('…saying who and on what', (await page.locator('.sheet .said-by b').first().textContent()) === 'June' && (await page.locator('.sheet .quote').textContent()).includes('lit before the bells'));
check('…with no Resolve or Delete for a reader', (await page.locator('.sheet .link.danger').count()) === 0 && (await page.locator('.sheet .secondary', { hasText: 'Resolve' }).count()) === 0);
await shot('2-thread');
await page.keyboard.press('Escape');
await settle();
check('Escape closes the sheet', (await page.locator('.sheet').count()) === 0);

// Commenting on words.
await select('counting stalls, and once');
await page.waitForSelector('.pick', { timeout: 5000 });
check('picking words offers Comment', (await page.locator('.pick').textContent()).includes('Comment'));
await shot('3-picked');
await page.locator('.pick').tap();
await settle();
check('…which opens a sheet quoting them', (await page.locator('.sheet .quote').textContent()).includes('counting stalls, and once'));
check('…and asks the reader’s name the first time', (await page.locator('#new-name').count()) === 1);
await page.fill('#new-text', 'Love the restlessness here.');
await page.locator('.sheet .primary').tap();
check('a comment can’t go without a name', (await page.locator('.toast').textContent()).includes('Add your name'));
await page.fill('#new-name', 'Ines');
await shot('4-new');
await page.locator('.sheet .primary').tap();
await page.waitForFunction(() => document.querySelectorAll('mark.cm').length >= 2, null, { timeout: 10000 });
check('the new comment is marked on its words', (await page.locator('mark.cm').allTextContents()).join('|').includes('counting stalls'));
let stored = await listComments();
const mine = stored.find((c) => c.comment.name === 'Ines');
check('…and the Worker has it, sealed, with its place', mine?.comment.on?.scene === 'night-market' && mine.comment.on.quote === 'counting stalls, and once' && mine.comment.version === 'v1', JSON.stringify(mine?.comment));
check('…the emphasis around the words left out of the quote', !mine?.comment.on.quote.includes('*'));
check('the name is remembered', await page.evaluate(() => localStorage.getItem('needle-reader-name') === 'Ines'));

// A reply.
await page.locator('mark.cm', { hasText: 'lit before' }).tap();
await settle();
check('a returning reader isn’t asked their name again', (await page.locator('#reply-name').count()) === 0);
await page.fill('#reply', 'Agreed, June.');
await page.locator('.sheet .primary').tap();
await page.waitForFunction(() => document.querySelectorAll('.sheet .replies .said').length === 1, null, { timeout: 10000 });
check('a reply goes under the comment', (await page.locator('.sheet .replies .said').textContent()) === 'Agreed, June.');
await page.keyboard.press('Escape');

// The list, and the scene after.
await page.locator('.icon.count').tap();
await settle();
check('the comments list has every thread', (await page.locator('.sheet .row.thread').count()) === 3);
check('…including one whose words were cut, saying so', (await page.locator('.sheet .row.thread', { hasText: 'a cut sentence' }).textContent()).includes('text removed'));
await shot('5-list');
await page.keyboard.press('Escape');
await page.locator('.step.next').tap();
await settle();
check('the next scene is a tap away', (await page.locator('h1.heading').textContent()) === 'Counting house');
await page.locator('.project').tap();
await settle();
await page.locator('.sheet .row', { hasText: 'The night market' }).tap();
await settle();
check('…and the contents go back', (await page.locator('h1.heading').textContent()) === 'The night market');

// --- As the author -------------------------------------------------------------------------------
await page.goto(address(`${id}.${b64(key)}.${author}`));
await page.waitForSelector('mark.cm', { timeout: 30000 });
check('with the author key, the page says so', (await page.locator('.author-pill').count()) === 1);
await page.locator('mark.cm', { hasText: 'lit before' }).tap();
await settle();
check('the author can reply as author', (await page.locator('label[for="reply"]').textContent()) === 'Reply as author');
await page.fill('#reply', 'It’s the ledger night.');
await page.locator('.sheet .primary').tap();
await page.waitForFunction(() => document.querySelectorAll('.sheet .replies .said').length === 2, null, { timeout: 10000 });
check('…and it shows as the author’s', (await page.locator('.sheet .replies .said-by').last().textContent()).includes('Author'));
await shot('6-author');
await page.locator('.sheet .secondary', { hasText: 'Resolve' }).tap();
await page.waitForFunction(() => ![...document.querySelectorAll('mark.cm')].some((m) => m.textContent.includes('lit before')), null, { timeout: 10000 });
check('resolving takes its mark away', true);
stored = await listComments();
check('…and the Worker has it resolved', stored.find((c) => c.comment.name === 'June' && c.comment.on?.quote === 'lit before the bells')?.resolved === true);
await page.keyboard.press('Escape');
await page.locator('.icon.count').tap();
await settle();
check('the list keeps resolved ones apart', (await page.locator('.sheet .group', { hasText: 'Resolved' }).count()) === 1);
page.once('dialog', (d) => d.accept());
await page.locator('.sheet .secondary.danger').tap();
await page.waitForSelector('.setup h1', { timeout: 10000 });
check('revoking ends the share', (await page.locator('.lead').textContent()).includes('You revoked this link'));
check('…on the Worker too', (await fetch(`${worker}/s/${id}`)).status === 404);

// --- Links that don't open ----------------------------------------------------------------------
await page.goto(address(`${id}.${b64(key)}`));
await page.waitForSelector('.setup h1', { timeout: 30000 });
check('a revoked link says it doesn’t work any more', (await page.locator('.lead').textContent()).includes('doesn’t work any more'));
await page.goto(`${readerUrl}#nonsense`);
await page.reload();
await page.waitForSelector('.setup h1', { timeout: 30000 });
check('a mangled link says so', (await page.locator('.setup h1').textContent()) === 'Nothing to read here');

check('no errors on the page', problems.length === 0, problems.join('; '));
await browser.close();
console.log(failures ? `\n${failures} failed` : '\nall passed');
process.exit(failures ? 1 : 0);
