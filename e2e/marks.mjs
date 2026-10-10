// The writer's own marks on the network board: handwritten notes, marker rings and arrows,
// moving them, rubbing them out. See harness.mjs for how a suite runs.
import { openBoard } from './harness.mjs';

const { page, check, shot, saves, settle, finish } = await openBoard('marks');
const lastMarks = async () => (await saves()).at(-1)?.marks ?? [];
// A new note takes focus on the next frame; type before then and the keys go nowhere.
const focused = () => page.waitForFunction(() => document.activeElement?.classList.contains('mark-editing'), null, { timeout: 3000 });
await shot('1-open');

// What network.toml already held is drawn.
check('saved note is drawn', (await page.locator('.mark-note', { hasText: 'he knows' }).count()) === 1);
check('saved ring is drawn', (await page.locator('.board-marks .marker-ink').count()) >= 1);
const tools = await page.locator('.board-tools button').allTextContents();
check('toolbar has the tools in order', ['Move', 'String', 'Write', 'Marker', 'Zone'].every((t, i) => tools[i]?.trim() === t), tools.join(', '));

// Screen spot of a card's middle, and of a point on the board in board units.
const cardBox = async (title) => page.locator('[data-card]', { hasText: title }).boundingBox();
const middle = (b) => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 });
const boardBox = await page.locator('.board').boundingBox();
// Above the cassette along the foot.
const emptySpot = { x: boardBox.x + 120, y: boardBox.y + boardBox.height - 230 };

// --- Write, loose on the cork -------------------------------------------------------------
await page.click('.board-tools button:has-text("Write")');
check('Write is chosen', (await page.locator('.board.tool-write').count()) === 1);
await page.mouse.click(emptySpot.x, emptySpot.y);
await page.waitForSelector('.mark-editing', { timeout: 3000 }).catch(() => {});
check('a note opens for writing', (await page.locator('.mark-editing').count()) === 1);
await focused();
await page.keyboard.type('who has the ledger?');
await page.keyboard.press('Enter');
await settle();
let marks = await lastMarks();
let loose = marks.find((m) => m.kind === 'note' && m.text === 'who has the ledger?');
check('loose note saved with its words', !!loose, JSON.stringify(loose));
check('loose note is on no card', loose && !loose.on);

// --- Write on a card ----------------------------------------------------------------------
const market = middle(await cardBox('Night Market'));
await page.mouse.click(market.x, market.y + 10);
await focused();
await page.keyboard.type('too quiet');
await page.keyboard.press('Enter');
await settle();
marks = await lastMarks();
const onCard = marks.find((m) => m.kind === 'note' && m.text === 'too quiet');
check('note written on a card goes on that card', onCard?.on === 'nt_market', JSON.stringify(onCard));

// --- An empty note is thrown away; Escape throws away a new one --------------------------
const before = (await lastMarks()).length;
await page.mouse.click(emptySpot.x + 200, emptySpot.y - 40);
await focused();
await page.keyboard.press('Enter');
await settle();
await page.mouse.click(emptySpot.x + 260, emptySpot.y - 80);
await focused();
await page.keyboard.type('never mind');
await page.keyboard.press('Escape');
await settle();
check('empty and cancelled notes leave nothing', (await lastMarks()).length === before, `${(await lastMarks()).length} vs ${before}`);
await shot('2-written');

// --- Marker: a ring on the cork, and an arrow card to card --------------------------------
await page.click('.board-tools button:has-text("Marker")');
check('Marker is chosen', (await page.locator('.board.tool-marker').count()) === 1);
const ringFrom = { x: boardBox.x + 60, y: boardBox.y + 120 };
await page.mouse.move(ringFrom.x, ringFrom.y);
await page.mouse.down();
await page.mouse.move(ringFrom.x + 90, ringFrom.y + 50, { steps: 6 });
check('a ring is previewed while drawing', (await page.locator('.marker.preview').count()) === 1);
await page.mouse.move(ringFrom.x + 180, ringFrom.y + 110, { steps: 6 });
await page.mouse.up();
await settle();
const rings = (await lastMarks()).filter((m) => m.kind === 'ring');
check('ring saved', rings.length === 2, `${rings.length} rings`);
check('preview gone after drawing', (await page.locator('.marker.preview').count()) === 0);

const mara = middle(await cardBox('Mara Venn'));
const ledger = middle(await cardBox('The missing ledger'));
await page.mouse.move(mara.x, mara.y);
await page.mouse.down();
await page.mouse.move(ledger.x, ledger.y, { steps: 10 });
await page.mouse.up();
await settle();
const arrow = (await lastMarks()).find((m) => m.kind === 'arrow');
check('arrow saved', !!arrow, JSON.stringify(arrow));
if (arrow) {
  // Mara is at (0, 0) and the ledger at (420, 240): the arrow stops at their edges.
  const [fx, fy] = arrow.from;
  const [tx, ty] = arrow.to;
  check('arrow starts at Mara’s edge, not her middle', Math.hypot(fx, fy) > 40 && Math.hypot(fx, fy) < 120, `from ${fx.toFixed(0)},${fy.toFixed(0)}`);
  check('arrow ends at the ledger’s edge', Math.hypot(tx - 420, ty - 240) > 30 && Math.hypot(tx - 420, ty - 240) < 120, `to ${tx.toFixed(0)},${ty.toFixed(0)}`);
}
// A second arrow the same way bows instead of lying on the first.
await page.mouse.move(mara.x, mara.y);
await page.mouse.down();
await page.mouse.move(ledger.x, ledger.y, { steps: 10 });
await page.mouse.up();
await settle();
const arrows = (await lastMarks()).filter((m) => m.kind === 'arrow');
check('second arrow between the same cards bows', arrows.length === 2 && arrows[1].bend !== 0, JSON.stringify(arrows.map((a) => a.bend)));
await shot('3-marked');

// --- Move: drag the loose note onto a card, then drag that card --------------------------
await page.keyboard.press('Escape');
check('Escape goes back to Move', (await page.locator('.board.tool-write, .board.tool-marker').count()) === 0);
const looseEl = page.locator('.mark-note', { hasText: 'who has the ledger?' });
const lb = await looseEl.boundingBox();
const teodor = middle(await cardBox('Old Teodor'));
await page.mouse.move(lb.x + lb.width / 2, lb.y + lb.height / 2);
await page.mouse.down();
await page.mouse.move(teodor.x, teodor.y + 15, { steps: 12 });
await page.mouse.up();
await settle();
loose = (await lastMarks()).find((m) => m.text === 'who has the ledger?');
check('note dropped on a card goes onto it', loose?.on === 'nt_teodor', JSON.stringify(loose));

const noteBefore = await looseEl.boundingBox();
await page.mouse.move(teodor.x, teodor.y - 30);
await page.mouse.down();
await page.mouse.move(teodor.x + 100, teodor.y + 50, { steps: 10 });
await page.mouse.up();
await settle();
const noteAfter = await looseEl.boundingBox();
check('a note on a card moves with the card', Math.abs(noteAfter.x - noteBefore.x - 100) < 12 && Math.abs(noteAfter.y - noteBefore.y - 80) < 12,
  `moved ${(noteAfter.x - noteBefore.x).toFixed(0)},${(noteAfter.y - noteBefore.y).toFixed(0)}`);

// --- Rub out: pick a ring by its line, press Delete ----------------------------------------
const ringsBefore = (await lastMarks()).filter((m) => m.kind === 'ring').length;
// A point that really is on the seed ring's line, a third of the way round it.
const onLine = await page.evaluate(() => {
  const path = document.querySelector('.marker-hit[data-mark="mk_seed2"]');
  const p = path.getPointAtLength(path.getTotalLength() / 3);
  const m = path.getScreenCTM();
  return { x: m.a * p.x + m.c * p.y + m.e, y: m.b * p.x + m.d * p.y + m.f };
});
await page.mouse.click(onLine.x, onLine.y);
await settle();
check('clicking a ring’s line picks it', (await page.locator('.marker.chosen').count()) === 1);
check('Rub out appears for a chosen mark', (await page.locator('.board-tools button:has-text("Rub out")').count()) === 1);
await page.keyboard.press('Delete');
await settle();
check('Delete rubs it out', (await lastMarks()).filter((m) => m.kind === 'ring').length === ringsBefore - 1);
await shot('4-after');

// --- Reload: what was saved comes back -------------------------------------------------
const kept = (await lastMarks()).length;
await page.click('[title="Network board"]');
await page.click('[title="Network board"]');
await page.waitForSelector('.board [data-card]');
await settle();
const drawnNotes = await page.locator('.mark-note').count();
const savedNotes = (await lastMarks()).filter((m) => m.kind === 'note').length;
check('reopened board draws every saved note', drawnNotes === savedNotes, `${drawnNotes} drawn, ${savedNotes} saved, ${kept} marks`);

process.exitCode = (await finish()) === 0 ? 0 : 1;
