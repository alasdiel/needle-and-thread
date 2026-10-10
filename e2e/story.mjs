// The network board through story time: the cassette along its foot, and the board changing as
// it's wound. See harness.mjs for how a suite runs; tauri-mock.js has the story it follows.
import { openBoard } from './harness.mjs';

const { page, check, shot, settle, finish } = await openBoard('story');
const cassette = page.locator('svg.cassette');
const title = () => page.locator('.board-story-title').textContent();
const value = () => cassette.getAttribute('aria-valuenow');
const tape = (text) => page.locator('.tape', { hasText: text });
const faded = (name) => page.locator('.card.not-yet', { hasText: name }).count();
const strings = () => page.locator('path.string').count();
const to = async (step) => {
  await cassette.focus();
  await page.keyboard.press('Home');
  for (let i = 0; i < step; i++) await page.keyboard.press('ArrowRight');
  await settle();
};
// The middle of a reel on screen.
const hub = async (side) => {
  const b = await page.locator(`.cassette-reel${side === 'left' ? '.left' : ':not(.left)'} .cassette-hit`).boundingBox();
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
};
// Winds a reel round by `turns` (positive is clockwise on screen), in small moves.
const wind = async (side, turns, during) => {
  const c = await hub(side);
  const r = 22;
  await page.mouse.move(c.x + r, c.y);
  await page.mouse.down();
  const n = Math.round(Math.abs(turns) * 36);
  for (let i = 1; i <= n; i++) {
    const a = Math.sign(turns) * (i / 36) * 2 * Math.PI;
    await page.mouse.move(c.x + r * Math.cos(a), c.y + r * Math.sin(a));
    if (i === Math.floor(n / 2) && during) await during();
  }
  await page.mouse.up();
  await settle();
};

check('the cassette is along the foot of the board', (await cassette.count()) === 1);
check('it starts at the end of the book', (await title()) === 'The end' && (await value()) === '5');
check('…with the project on its label', (await page.locator('.cassette-name').textContent()) === 'Tidewater');
check('…the right reel full and the left empty', Number(await page.locator('.cassette-reel:not(.left) .cassette-wound').getAttribute('r')) > Number(await page.locator('.cassette-reel.left .cassette-wound').getAttribute('r')));
check('…and no pencil at rest', (await page.locator('.cassette-pencil').count()) === 0);
check('…where the string shows its last label', (await tape('resents').count()) === 1);
check('…and nothing is faded', (await page.locator('.card.not-yet').count()) === 0);
check('…and no string is flagged', (await page.locator('.tape.unplaced').count()) === 0);

await to(0);
check('at the first step it says where it is', (await title()) === 'What Teodor saw' && (await page.locator('.board-story-step').textContent()) === 'Step 1 of 5');
check('…and when', (await page.locator('.board-story-time').textContent()) === '2 June 1979');
check('Mara isn’t in the story yet, so she’s faded', (await faded('Mara Venn')) === 1);
check('…but Teodor is', (await faded('Old Teodor')) === 0);
check('…and so is her pin', (await page.locator('g.pin.not-yet').count()) === 1);
check('a string that hasn’t begun isn’t there', (await strings()) === 0 && (await page.locator('.tape').count()) === 0);
await shot('1-start');

await to(1);
check('at the harbor Mara comes in', (await faded('Mara Venn')) === 0);
check('…and the string begins, with its first label', (await strings()) === 1 && (await tape('owes').count()) === 1);
check('a string with a time not on the timeline is flagged', (await page.locator('.tape.unplaced').getAttribute('title')) === 'Not on the timeline: The Drowning');

await to(2);
check('at the night market it turns', (await tape('resents').count()) === 1 && (await tape('owes').count()) === 0);
await shot('2-turned');

await to(5);
check('the last stop is the end again', (await title()) === 'The end' && (await page.locator('.tape.unplaced').count()) === 0);

let mid = {};
await wind('left', -0.55, async () => {
  mid.pencil = await page.locator('.cassette-pencil').count();
  mid.grip = await page.locator('.cassette-reel.left .cassette-grip').count();
  mid.caps = await page.locator('.board-story-caps').first().textContent();
  await shot('3-winding');
});
check('winding the left reel back shows the pencil in it, and a grip ring', mid.pencil === 1 && mid.grip === 1);
check('…says it’s winding back', mid.caps === 'Winding back');
check('…and goes back a step per sixth of a turn', (await value()) === '2', await value());
check('letting go puts the pencil away', (await page.locator('.cassette-pencil').count()) === 0);

await wind('right', 0.4);
check('winding the right reel clockwise goes on', (await value()) === '4', await value());

const left = await hub('left');
await page.mouse.click(left.x + 16, left.y);
await settle();
check('clicking a reel goes a step towards it', (await value()) === '3', await value());
await shot('4-wound');

await page.locator('.board-story-return').click();
await settle();
check('Wind to the end goes back to the end', (await title()) === 'The end' && (await page.locator('.board-story-return').count()) === 0);

process.exitCode = (await finish()) === 0 ? 0 : 1;
