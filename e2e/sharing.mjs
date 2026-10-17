// Sharing in the window (DESIGN §10): the Shared panel's comments and links (Share 2 on the
// canvas's Sharing page), and a comment's card beside its words when they're clicked (Comments 1,
// only the clicked one). tauri-mock.js keeps the links in window.__shares and readers' comments in
// window.__comments, placed as the real share_comments places them.
import { openApp } from './harness.mjs';

const { page, check, shot, settle, calls, finish } = await openApp('sharing');
const editor = page.locator('.ProseMirror');
await page.waitForFunction(() => document.querySelector('.ProseMirror')?.textContent.includes('The market opened at dusk.'), null, { timeout: 10000 });
try {
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
} catch {
  // WebKit has no such permissions; a click is enough there.
}

// Marks can be split by other decorations (spelling), so they're read as a whole.
const marked = async () => (await page.locator('.comment-mark').allTextContents()).join('');
const markIds = () => page.evaluate(() => new Set([...document.querySelectorAll('.comment-mark')].map((m) => m.dataset.comment)).size);
const comment = (id, extra) => ({
  share: 'share1', share_name: 'Beta readers', id, name: 'Ines', text: 'Why at dusk?', author: false, resolved: false,
  created: 1791627360, reply_to: null, scene: 'night-market', anchor: { quote: 'opened at dusk', n: 0 }, quote: 'opened at dusk', fresh: true, ...extra,
});
await page.evaluate((comments) => {
  window.__shares = [{ id: 'share1', name: 'Beta readers', covers: { covers: 'chapter', chapter: 'ol_1' }, title: 'Arrival', link: 'https://example.github.io/needle-and-thread/read/#share1.key', created: 1, sent: 1791627360, error: null, comments: 3, fresh: 2 }];
  window.__comments = comments;
}, [
  comment(1),
  comment(2, { name: 'June', text: 'I liked the bit about the lanterns.', anchor: null, quote: 'the lanterns were lit' }),
  comment(3, { name: 'June', text: 'Me too.', reply_to: 1, scene: null, anchor: null, quote: '', fresh: true }),
]);

// --- The panel's comments ----------------------------------------------------------------------
await page.click('[title="Shared"]');
await page.waitForSelector('.shared-panel .shared-row', { timeout: 10000 });
check('Shared opens a panel, on its comments', (await page.locator('.shared-tabs .active').textContent()).startsWith('Comments'));
check('…counting the open threads', (await page.locator('.shared-tabs .active').textContent()) === 'Comments · 2');
check('this scene’s come first', (await page.locator('.shared-group').first().textContent()) === 'In this scene');
check('one whose words were cut is listed apart', (await page.locator('.shared-group', { hasText: 'Text removed' }).count()) === 1);
check('new ones have a dot, and so does Shared', (await page.locator('.shared-row .note-dot').count()) === 2 && (await page.locator('.shared-button .note-dot').count()) === 1);
await page.waitForSelector('.comment-mark', { timeout: 5000 });
check('the commented words are marked in the text', (await marked()) === 'opened at dusk', await marked());
check('…and only those: the cut one has nowhere to go', (await markIds()) === 1);
check('no card shows until a mark is clicked', (await page.locator('.comment-card').count()) === 0);
await shot('1-panel');

// --- A comment's card ------------------------------------------------------------------------------
await page.click('[title="Close"][aria-label="Close Shared"]');
await page.locator('.comment-mark').first().click();
await settle();
check('clicking marked words opens their card', (await page.locator('.comment-card').count()) === 1);
check('…with the comment and its reply', (await page.locator('.comment-card .comment-text').allTextContents()).join('|') === 'Why at dusk?|Me too.');
check('…saying who, and through which link', (await page.locator('.comment-card .comment-who').first().textContent()).includes('Ines') && (await page.locator('.comment-card .comment-badge').first().textContent()) === 'beta readers');
check('…beside the words, level with them', await page.evaluate(() => {
  const mark = document.querySelector('.comment-mark').getBoundingClientRect();
  // (the card's top is the words' top)
  const card = document.querySelector('.comment-card').getBoundingClientRect();
  return Math.abs(card.top - mark.top) < 4;
}));
check('the open one is drawn stronger', (await page.locator('.comment-mark.active').count()) > 0);
check('opening it marks the thread seen', (await calls('seen_comments')).some((c) => c.args.share === 'share1' && c.args.id === 3));
await shot('2-card');
await page.keyboard.press('Escape');
await settle();
check('Escape closes the card', (await page.locator('.comment-card').count()) === 0 && (await page.locator('.comment-mark.active').count()) === 0);

await page.locator('.comment-mark').first().click();
await page.fill('.comment-reply', 'It’s the ledger night.');
await page.click('.comment-actions .primary');
await page.waitForFunction(() => document.querySelectorAll('.comment-card .comment-text').length === 3, null, { timeout: 5000 });
check('a reply goes to the right comment', (await calls('reply_comment')).some((c) => c.args.to === 1 && c.args.text === 'It’s the ledger night.'));
check('…and shows as yours', (await page.locator('.comment-card .comment-who').last().textContent()).includes('You'));
await page.click('.comment-actions button:has-text("Resolve")');
await settle();
await settle();
check('Resolve resolves it and closes the card', (await calls('resolve_comment')).some((c) => c.args.id === 1 && c.args.resolved === true) && (await page.locator('.comment-card').count()) === 0);
check('…and its words aren’t marked any more', (await page.locator('.comment-mark').count()) === 0);

// The cut one, from the panel.
await page.click('[title="Shared"]');
await page.waitForSelector('.shared-row');
await page.locator('.shared-row', { hasText: 'the lanterns were lit' }).click();
await settle();
check('a comment whose words were cut opens from the panel, saying so', (await page.locator('.comment-card .comment-removed').textContent()).includes('the lanterns were lit'));
check('…and the resolved one is listed as resolved', (await page.locator('.shared-group', { hasText: 'Resolved' }).count()) === 1);
await page.keyboard.press('Escape');

// --- Links -------------------------------------------------------------------------------------
await page.click('.shared-tabs button:has-text("Links")');
await settle();
const options = await page.locator('#share-what option').allTextContents();
check('a new link can share the scene, its chapter, its part or the book', options.join('|') === 'This scene: The night market|This chapter: Arrival|This part: Part one: Landfall|The whole book', options.join('|'));
check('a link needs a name first', await page.locator('.shared-new .primary').isDisabled());
await page.selectOption('#share-what', { label: 'This chapter: Arrival' });
await page.fill('#share-name', 'Editor');
await page.click('.shared-new .primary');
await page.waitForSelector('.shared-link.made', { timeout: 5000 });
const made = (await calls('make_share')).pop()?.args;
check('Make a link shares what was picked, under its name', made?.covers?.covers === 'chapter' && made.covers.chapter === 'ol_1' && made.name === 'Editor', JSON.stringify(made));
check('…and the new link shows its address', (await page.locator('.shared-link.made .shared-address').textContent()).includes('/read/#share2'));
check('links list what they share and when they were sent', (await page.locator('.shared-link').last().textContent()).includes('Beta readers') && (await page.locator('.shared-link').last().textContent()).includes('Sent'));
await page.locator('.shared-link.made button:has-text("Copy link")').click();
check('Copy link says it copied', (await page.locator('.shared-link.made button:has-text("Copied")').count()) === 1);
await shot('3-links');
await page.locator('.shared-link.made button:has-text("Open as author")').click();
check('Open as author asks for the author’s page', (await calls('open_as_author')).some((c) => c.args.id === 'share2'));
page.once('dialog', (d) => d.accept());
await page.locator('.shared-link.made button:has-text("Revoke")').click();
await settle();
check('Revoke asks first, then revokes', (await calls('revoke_share')).some((c) => c.args.id === 'share2'));
check('…and the link goes from the list', (await page.locator('.shared-link').count()) === 1);

process.exit((await finish()) ? 1 : 0);
