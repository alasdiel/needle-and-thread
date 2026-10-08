// Building on the network board: tying strings between cards, the side panel for a card or a
// string, a plot point from a double-click, zones, pinning notes up and the Show filters. See
// harness.mjs for how a suite runs; the mock's board changes as the real files would.
import { openBoard } from './harness.mjs';

const { page, check, shot, saves, calls, settle, finish } = await openBoard('building');
const tool = (name) => page.locator('.board-tools').getByRole('button', { name, exact: true }).click();
const cardBox = (title) => page.locator('[data-card]', { hasText: title }).boundingBox();
const middle = (b) => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 });
const cardAt = async (title) => middle(await cardBox(title));
const drag = async (a, b) => {
  await page.mouse.move(a.x, a.y);
  await page.mouse.down();
  await page.mouse.move(b.x, b.y, { steps: 10 });
  await page.mouse.up();
  await settle();
};
const typing = (id) => page.waitForFunction((id) => document.activeElement?.id === id, id, { timeout: 3000 });
const tape = (text) => page.locator('.tape', { hasText: text });
const panel = page.locator('.board-panel');
const boardBox = await page.locator('.board').boundingBox();

// --- What was already there ---------------------------------------------------------------
check('a changed relationship shows its last label', (await tape('resents').count()) === 1, await page.locator('.tape').allTextContents().then((t) => t.join(' | ')));
check('…and not its first', (await tape('owes').count()) === 0);

// --- String: tie Mara to the market -------------------------------------------------------
await tool('String');
check('String is chosen', (await page.locator('.board.tool-tie').count()) === 1);
await drag(await cardAt('Mara Venn'), await cardAt('Night Market'));
await typing('board-pending');
check('a string let go on a card asks for its label', (await page.locator('#board-pending').count()) === 1);
await page.keyboard.type('hides from');
await page.keyboard.press('Enter');
await settle();
let tied = await calls('add_relationship');
check('it ties Mara to the market, one way', tied.length === 1 && tied[0].args.from === 'nt_mara' && tied[0].args.to === 'nt_market' && tied[0].args.directed === true, JSON.stringify(tied.map((c) => c.args)));
check('the label is on its tape, reading one way', (await tape('hides from →').count()) === 1);
check('the new string opens in the panel', (await panel.locator('.panel-kind', { hasText: 'Relationship' }).count()) === 1);
await shot('1-tied');

// Suggestions: labels already on the board are offered as you type.
await drag(await cardAt('Old Teodor'), await cardAt('The missing ledger'));
await typing('board-pending');
await page.keyboard.type('ow');
const offered = page.locator('.label-suggestions button', { hasText: 'owes' });
check('a label used before is offered', (await offered.count()) === 1, await page.locator('.label-suggestions').allTextContents().then((t) => t.join(' | ')));
await offered.dispatchEvent('pointerdown');
await settle();
tied = await calls('add_relationship');
check('picking it ties the string with that label', tied.length === 2 && tied[1].args.label === 'owes', JSON.stringify(tied.at(-1)?.args));

// Escape lets the string drop; so does letting go on the cork.
await drag(await cardAt('Old Teodor'), await cardAt('Night Market'));
await typing('board-pending');
await page.keyboard.press('Escape');
await settle();
await drag(await cardAt('Old Teodor'), { x: boardBox.x + 40, y: boardBox.y + boardBox.height - 40 });
check('Escape, or letting go on the cork, ties nothing', (await calls('add_relationship')).length === 2);

// --- The panel for a string ---------------------------------------------------------------
await tool('Move');
await tape('hides from').click();
await settle();
check('clicking a tape opens its string', (await panel.locator('.panel-title', { hasText: 'Mara Venn' }).count()) === 1);
await panel.getByRole('button', { name: 'Both ways', exact: true }).click();
await settle();
let edits = await calls('edit_relationship');
check('Both ways makes it mutual', edits.at(-1)?.args.directed === false, JSON.stringify(edits.at(-1)?.args));
check('…and the tape loses its arrow', (await tape('hides from').textContent())?.trim() === 'hides from');
await panel.getByRole('button', { name: 'One way', exact: true }).click();
await settle();
await panel.getByRole('button', { name: 'Turn round', exact: true }).click();
await settle();
edits = await calls('edit_relationship');
check('Turn round reverses it', edits.at(-1)?.args.reverse === true);
check('…so it reads from the market to Mara', (await panel.locator('.panel-title').textContent())?.startsWith('Night Market'), await panel.locator('.panel-title').textContent());
await page.fill('#string-label', 'watches');
await page.keyboard.press('Enter');
await settle();
edits = await calls('edit_relationship');
check('relabelling it in the panel saves the label', edits.at(-1)?.args.label === 'watches');
check('…and the tape says so', (await tape('watches').count()) === 1);

// History, on the string that changed.
await tape('resents').click();
await settle();
const history = await panel.locator('.panel-history').textContent().catch(() => '');
check('a string that changed shows how', history.includes('From the start') && history.includes('owes') && history.includes('At The night market') && history.includes('resents'), history);
check('…and its first label is the one to edit', (await page.inputValue('#string-label')) === 'owes');

// A string can go to the cut bin.
await tape('watches').click();
await settle();
await panel.getByRole('button', { name: 'Move to the cut bin' }).click();
await settle();
check('Move to the cut bin cuts its note', (await calls('cut_note')).length === 1);
check('…and the string is gone', (await tape('watches').count()) === 0 && (await panel.count()) === 0);
await shot('2-strings');

// --- The panel for a card -----------------------------------------------------------------
await page.mouse.click((await cardAt('Old Teodor')).x, (await cardAt('Old Teodor')).y);
await settle();
check('clicking a card opens it in the panel', (await panel.locator('.panel-title', { hasText: 'Old Teodor' }).count()) === 1);
const strings = await panel.locator('.panel-list').textContent().catch(() => '');
check('the panel lists its strings', strings.includes('resents') && strings.includes('owes') && strings.includes('The missing ledger'), strings);
check('a card that is up on its own has no Take down', (await panel.getByRole('button', { name: 'Take down' }).count()) === 0);
await panel.locator('.panel-row', { hasText: 'The missing ledger' }).click();
await settle();
check('a string in the list opens that string', (await panel.locator('.panel-kind', { hasText: 'Relationship' }).count()) === 1);
await page.keyboard.press('Escape');
await settle();
check('Escape closes the panel', (await panel.count()) === 0);

// --- Double-click the cork for a plot point -------------------------------------------------
const cork = { x: boardBox.x + boardBox.width - 160, y: boardBox.y + boardBox.height - 160 };
await page.mouse.dblclick(cork.x, cork.y);
await typing('board-pending');
check('double-clicking the cork starts a plot point', (await page.locator('.pending-card').count()) === 1);
await page.keyboard.type('The ledger burns');
await page.keyboard.press('Enter');
await settle();
const added = await calls('add_card');
check('it makes a plot point note', added.length === 1 && added[0].args.kind === 'event' && added[0].args.title === 'The ledger burns', JSON.stringify(added[0]?.args));
check('…and its card is up', (await page.locator('[data-card]', { hasText: 'The ledger burns' }).count()) === 1);
const burns = await cardAt('The ledger burns');
check('…where the double-click was', Math.hypot(burns.x - cork.x, burns.y - cork.y) < 4, `${burns.x.toFixed(0)},${burns.y.toFixed(0)} vs ${cork.x},${cork.y}`);
await page.mouse.dblclick(cork.x - 200, cork.y);
await typing('board-pending');
await page.keyboard.press('Escape');
await settle();
check('Escape, or no title, makes nothing', (await calls('add_card')).length === 1 && (await page.locator('.pending-card').count()) === 0);

// Double-clicking a card opens its note, which leaves the board.
await page.mouse.dblclick(burns.x, burns.y);
await settle();
check('double-clicking a card opens its note', (await calls('open_note')).some((c) => c.args.path === 'events/the-ledger-burns'));
check('…and the board closes for it', (await page.locator('.board').count()) === 0);
await page.click('[title="Network board"]');
await page.waitForSelector('.board [data-card]');
await settle();
await shot('3-plot-point');

// --- Zones --------------------------------------------------------------------------------
await tool('Zone');
const z0 = { x: boardBox.x + 80, y: boardBox.y + 400 };
await drag(z0, { x: z0.x + 260, y: z0.y + 180 });
await typing('zone-naming');
check('a zone is laid down and asks for its name', (await page.locator('.zone').count()) === 1);
await page.keyboard.type('The harbour');
await page.keyboard.press('Enter');
await settle();
let zone = (await saves()).at(-1)?.zones?.[0];
check('the zone is saved with its name', zone?.name === 'The harbour' && zone.size[0] > 200, JSON.stringify(zone));
const was = zone;

await tool('Move');
const tapeBox = await page.locator('.zone-tape').boundingBox();
await drag(middle(tapeBox), { x: middle(tapeBox).x + 60, y: middle(tapeBox).y + 30 });
zone = (await saves()).at(-1)?.zones?.[0];
check('dragging its tape moves it', Math.abs(zone.at[0] - was.at[0] - 60) < 6 && Math.abs(zone.at[1] - was.at[1] - 30) < 6, JSON.stringify(zone.at));
check('pressing the tape chooses it', (await page.locator('.zone.chosen').count()) === 1);
const corner = await page.locator('.zone-size').boundingBox();
await drag(middle(corner), { x: middle(corner).x + 50, y: middle(corner).y + 40 });
const grown = (await saves()).at(-1)?.zones?.[0];
check('its corner resizes it', Math.abs(grown.size[0] - zone.size[0] - 50) < 6 && Math.abs(grown.size[1] - zone.size[1] - 40) < 6, JSON.stringify(grown.size));
await page.mouse.dblclick(middle(await page.locator('.zone-tape').boundingBox()).x, middle(await page.locator('.zone-tape').boundingBox()).y);
await typing('zone-naming');
await page.fill('#zone-naming', 'The quay');
await page.keyboard.press('Enter');
await settle();
check('double-clicking its tape renames it', (await saves()).at(-1)?.zones?.[0]?.name === 'The quay');
// Pressing the paper itself pans, so a zone can't trap the board.
const before = await cardAt('Mara Venn');
const paper = await page.locator('.zone').boundingBox();
await drag({ x: paper.x + paper.width / 2, y: paper.y + paper.height - 20 }, { x: paper.x + paper.width / 2 + 40, y: paper.y + paper.height - 20 });
const after = await cardAt('Mara Venn');
check('dragging the paper pans the board', Math.abs(after.x - before.x - 40) < 4, `${(after.x - before.x).toFixed(0)}`);
await shot('4-zone');
await page.locator('.zone-tape').click();
await page.keyboard.press('Delete');
await settle();
check('Delete takes a chosen zone down', ((await saves()).at(-1)?.zones ?? []).length === 0 && (await page.locator('.zone').count()) === 0);

// --- Pin up -------------------------------------------------------------------------------
await page.locator('.board-tools').getByRole('button', { name: 'Pin up', exact: true }).click();
await page.waitForSelector('.pin-picker .panel-row');
const offeredNotes = await page.locator('.pin-picker .panel-row').allTextContents();
check('Pin up offers what is not on the board', offeredNotes.some((t) => t.includes('The Drowned Harbour')) && offeredNotes.some((t) => t.includes('Things to check')), offeredNotes.join(' | '));
check('…and not what is, nor relationships', !offeredNotes.some((t) => t.includes('Mara Venn') || t.includes('owes')), offeredNotes.join(' | '));
await page.fill('#pin-query', 'drown');
check('typing narrows the list', (await page.locator('.pin-picker .panel-row').count()) === 1);
await page.locator('.pin-picker .panel-row').first().click();
await settle();
check('picking one pins it up', (await calls('pin_card')).at(-1)?.args.id === 'nt_harbour' && (await page.locator('[data-card]', { hasText: 'The Drowned Harbour' }).count()) === 1);
check('…saying where it came from', (await page.locator('[data-card]', { hasText: 'The Drowned Harbour' }).locator('.card-from').textContent()) === 'glass-coast');
const harbour = await cardAt('The Drowned Harbour');
await page.mouse.click(harbour.x, harbour.y);
await settle();
await panel.getByRole('button', { name: 'Take down' }).click();
await settle();
check('a pinned card can be taken down', (await calls('unpin_card')).length === 1 && (await page.locator('[data-card]', { hasText: 'The Drowned Harbour' }).count()) === 0);

// --- Show ---------------------------------------------------------------------------------
await page.locator('.board-tools').getByRole('button', { name: 'Show', exact: true }).click();
await page.locator('.board-popover label', { hasText: 'Characters' }).locator('input').uncheck();
await settle();
check('leaving characters off hides their cards', (await page.locator('.card.polaroid').count()) === 0 && (await page.locator('.card').count()) > 0);
check('…and their strings', (await tape('resents').count()) === 0);
await page.locator('.board-popover label', { hasText: 'Characters' }).locator('input').check();
await page.locator('.board-popover label', { hasText: 'Automatic links' }).locator('input').uncheck();
await settle();
check('automatic links can be hidden', (await page.locator('.twine').count()) === 0);
await page.locator('.board-popover label', { hasText: 'Automatic links' }).locator('input').check();
await page.selectOption('.board-popover select', { label: 'The missing ledger' });
await settle();
const left = (await page.locator('.card .card-title').allTextContents()).sort();
check('one thread shows it and what is tied to it', JSON.stringify(left) === JSON.stringify(['Old Teodor', 'The missing ledger']), left.join(', '));
check('the Show button says a filter is on', (await page.locator('.board-tools button.active', { hasText: 'Show' }).count()) === 1);
await shot('5-filtered');
await page.selectOption('.board-popover select', { label: 'Everything' });
await settle();
check('Everything brings them back', (await page.locator('.card').count()) === 5, `${await page.locator('.card').count()} cards`);

process.exitCode = (await finish()) === 0 ? 0 : 1;
