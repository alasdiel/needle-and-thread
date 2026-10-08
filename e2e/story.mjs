// The network board through story time: the slider along its foot, and the board changing as it
// moves. See harness.mjs for how a suite runs; tauri-mock.js has the story it follows.
import { openBoard } from './harness.mjs';

const { page, check, shot, settle, finish } = await openBoard('story');
const slider = page.locator('.board-story input[type="range"]');
const title = () => page.locator('.board-story-title').textContent();
const tape = (text) => page.locator('.tape', { hasText: text });
const faded = (name) => page.locator('.card.not-yet', { hasText: name }).count();
const strings = () => page.locator('path.string').count();
const to = async (step) => {
  await slider.fill(String(step));
  await settle();
};

check('the slider is along the foot of the board', (await slider.count()) === 1);
check('it starts at the end of the book', (await title()) === 'The end' && (await slider.inputValue()) === '5');
check('…where the string shows its last label', (await tape('resents').count()) === 1);
check('…and nothing is faded', (await page.locator('.card.not-yet').count()) === 0);
check('…and no string is flagged', (await page.locator('.tape.unplaced').count()) === 0);

await to(0);
check('at the first step it says where it is', (await title()) === '1. What Teodor saw');
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

process.exitCode = (await finish()) === 0 ? 0 : 1;
