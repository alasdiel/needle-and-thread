// The timeline: story order across the page, lanes as rows, the tray, and setting when something
// happens from the panel. Also that the same markup becomes a list at phone width. See
// harness.mjs for how a suite runs; the mock's timeline changes the way a header would.
import { openApp } from './harness.mjs';

const { page, check, shot, calls, settle, finish } = await openApp('timeline');
await page.click('[title="Timeline"]');
await page.waitForSelector('.timeline .time-piece', { timeout: 10000 });
await settle();

const placed = () => page.locator('.timeline-inner .time-piece .piece-title').allTextContents();
const tray = () => page.locator('.timeline-tray .piece-title').allTextContents();
const piece = (title) => page.locator('.time-piece').filter({ has: page.locator('.piece-title').getByText(title, { exact: true }) });
const panel = page.locator('.when-panel');
const top = async (title) => (await piece(title).boundingBox()).y;
const left = async (title) => (await piece(title).boundingBox()).x;

// --- Story order and lanes ------------------------------------------------------------------
check('the board button and the timeline are not both open', (await page.locator('.board').count()) === 0);
check('the scene’s envelope tab hides with the page', (await page.locator('.envelope-tab').count()) === 0);
check('story order runs across the page', JSON.stringify(await placed()) === JSON.stringify(['What Teodor saw', 'The harbor', 'The night market', 'The ledger leaves port', 'The long night']), (await placed()).join(' | '));
const lefts = await Promise.all(['What Teodor saw', 'The harbor', 'The night market'].map(left));
check('…each a step after the last', lefts[0] < lefts[1] && lefts[1] < lefts[2], lefts.join(', '));
const lanes = await page.locator('.lane-name').allTextContents();
check('lanes by POV, plot points last', JSON.stringify(lanes) === JSON.stringify(['Old Teodor', 'Mara Venn', 'Plot points']), lanes.join(' | '));
check('a POV keeps to its lane', (await top('The harbor')) === (await top('The night market')) && (await top('The harbor')) !== (await top('The long night')));
// `--lane` is the lane's colour in app.css, so the positional index has to be called something
// else: when it wasn't, every thread was painted with the colour "1" and vanished.
const threads = await page.locator('.lane-thread').evaluateAll((els) => els.map((e) => ({ w: Math.round(e.getBoundingClientRect().width), bg: getComputedStyle(e).backgroundColor })));
check('a lane with pieces apart is joined by its thread', threads.some((t) => t.w > 100), JSON.stringify(threads));
check('…painted in the lane’s colour', threads.every((t) => /^rgba?\(/.test(t.bg)), JSON.stringify(threads.map((t) => t.bg)));
check('the flashback’s piece number stands out', (await piece('What Teodor saw').locator('.piece-number.flashback').count()) === 1);
check('…and only it', (await page.locator('.piece-number.flashback').count()) === 1);
check('a plot point placed by order is dashed', await piece('The ledger leaves port').evaluate((el) => el.classList.contains('loose') && el.classList.contains('event')));
check('…and says what it follows', (await piece('The ledger leaves port').textContent()).includes('after The night market'));
const tape = await page.locator('.tape-tick').allTextContents();
check('times are on the tape', tape.includes('14 March 1998, 19:00') && tape.length === 4, tape.join(' | '));
const gaps = await page.locator('.tape-gap').allTextContents();
check('so is the time between', gaps.includes('6 hours') && gaps.includes('18 years'), gaps.join(' | '));
check('what has no time waits in the tray', JSON.stringify(await tray()) === JSON.stringify(['Someday by the sea', 'The quay']), (await tray()).join(' | '));
check('…with the reason, if there is one', (await page.locator('.timeline-tray .piece-problem').textContent()) === 'nothing is called The harbour');
await shot('1-pov');

// Scrolled sideways, the lane names stay at the left; scrolled down, the tape stays on top.
await page.locator('.timeline-scroll').evaluate((el) => { el.scrollLeft = 300; });
await settle();
const namesBox = await page.locator('.timeline-lanes').boundingBox();
const scrollBox = await page.locator('.timeline-scroll').boundingBox();
check('scrolled sideways, the lane names stay in view', Math.abs(namesBox.x - scrollBox.x) < 2, `${namesBox.x} vs ${scrollBox.x}`);
check('…and the story has moved under them', (await left('What Teodor saw')) < namesBox.x + namesBox.width);
await page.locator('.timeline-scroll').evaluate((el) => { el.scrollTop = 120; el.scrollLeft = 0; });
await settle();
const tapeBox = await page.locator('.tape-measure').boundingBox();
const tickBox = await page.locator('.tape-tick').first().boundingBox();
check('scrolled down, the tape stays on top', Math.abs(tapeBox.y - scrollBox.y) < 2, `${tapeBox.y} vs ${scrollBox.y}`);
check('…and its times stay on it', Math.abs(tickBox.y - tapeBox.y) < 2);
await page.locator('.timeline-scroll').evaluate((el) => { el.scrollTop = 0; });

await page.locator('.timeline-bar').getByRole('button', { name: 'Thread' }).click();
await settle();
const threadLanes = await page.locator('.lane-name').allTextContents();
check('lanes by thread', JSON.stringify(threadLanes) === JSON.stringify(['No thread', 'The missing ledger']) || JSON.stringify(threadLanes) === JSON.stringify(['The missing ledger', 'No thread']), threadLanes.join(' | '));
check('…with the catch-all last', threadLanes.at(-1) === 'No thread');
await page.locator('.timeline-bar').getByRole('button', { name: 'POV' }).click();
await settle();

// --- The panel ------------------------------------------------------------------------------
await piece('The night market').click();
await settle();
check('a click opens the panel', (await panel.count()) === 1);
check('…saying which scene and whose', (await panel.locator('.panel-kind').textContent()) === 'Scene 2 · Mara Venn');
check('…on the way its time is written', (await panel.getByRole('button', { name: 'From', exact: true }).getAttribute('aria-pressed')) === 'true');
const fields = await panel.locator('input[type="text"]').evaluateAll((els) => els.map((e) => e.value));
check('…with its fields filled in', JSON.stringify(fields) === JSON.stringify(['The harbor', '+6h']), fields.join(' | '));
check('the chosen piece is outlined', await piece('The night market').evaluate((el) => el.classList.contains('chosen')));
await shot('2-panel');

// Give the scene from the tray a date.
await piece('Someday by the sea').click();
await settle();
check('a piece from the tray opens too, with a date to fill in', (await panel.getByRole('button', { name: 'Date', exact: true }).getAttribute('aria-pressed')) === 'true');
await panel.locator('input[type="text"]').fill('nonsense');
await panel.getByRole('button', { name: 'Set' }).click();
await settle();
check('a date it can’t read is refused, and says why', (await panel.locator('.when-problem').textContent()).includes('isn\'t a date'));
check('…and stays in the tray', (await tray()).includes('Someday by the sea'));
await panel.locator('input[type="text"]').fill('1998-03-17 09:00');
await panel.locator('input[type="text"]').press('Enter');
await settle();
const set = await calls('set_when');
check('Enter sets it', set.length === 2 && set[1].args.when.type === 'text' && set[1].args.when.text === '1998-03-17 09:00' && set[1].args.path === 'someday', JSON.stringify(set.map((c) => c.args)));
check('…sending its owner by name', set[1].args.itemOwner === 'tidewater' && set[1].args.kind === 'scene');
check('it leaves the tray for the timeline', (await placed()).includes('Someday by the sea') && !(await tray()).includes('Someday by the sea'));
check('the problem has gone', (await panel.locator('.when-problem').count()) === 0);

// Between two others.
await panel.getByRole('button', { name: 'Between' }).click();
const orderInputs = panel.locator('input[type="text"]');
check('Between asks for after and before', (await orderInputs.count()) === 2);
check('…offering names as you type', (await page.locator('#timeline-names option').count()) === 6);
await orderInputs.nth(0).fill('The harbor');
await panel.getByRole('button', { name: 'Set' }).click();
await settle();
const last = (await calls('set_when')).at(-1).args.when;
check('an empty before isn’t sent', last.type === 'order' && last.after === 'The harbor' && last.before == null, JSON.stringify(last));
check('…and the piece is dashed now', await piece('Someday by the sea').evaluate((el) => el.classList.contains('loose')));

// Back to the tray.
await panel.getByRole('button', { name: 'Not yet' }).click();
await panel.getByRole('button', { name: 'Set' }).click();
await settle();
check('Not yet puts it back in the tray', (await tray()).includes('Someday by the sea'));
await page.keyboard.press('Escape');
await settle();
check('Escape closes the panel', (await panel.count()) === 0);

// --- In a narrow window ---------------------------------------------------------------------
await piece('The ledger leaves port').click();
await settle();
const chosenBox = await piece('The ledger leaves port').boundingBox();
const panelBox = await panel.boundingBox();
check('a piece the panel would cover is scrolled into view beside it', chosenBox.x + chosenBox.width <= panelBox.x + 1, `${chosenBox.x + chosenBox.width} vs ${panelBox.x}`);
await page.keyboard.press('Escape');
await settle();

// --- At phone width, the lanes become a list ------------------------------------------------
await page.setViewportSize({ width: 390, height: 844 });
await settle();
check('the tape goes', (await page.locator('.tape-measure').isVisible()) === false);
check('the lane names go', (await page.locator('.timeline-lanes').isVisible()) === false);
check('story order is still the same list', JSON.stringify(await placed()) === JSON.stringify(['What Teodor saw', 'The harbor', 'The night market', 'The ledger leaves port', 'The long night']), (await placed()).join(' | '));
const phoneTops = await Promise.all(['What Teodor saw', 'The harbor', 'The night market'].map(top));
check('…now one under the other', phoneTops[0] < phoneTops[1] && phoneTops[1] < phoneTops[2], phoneTops.join(', '));
const phoneLefts = await Promise.all(['The harbor', 'The long night'].map(left));
check('…with every row on one left edge, lanes or not', phoneLefts[0] === phoneLefts[1], phoneLefts.join(', '));
const rowWidth = (await piece('The harbor').boundingBox()).width;
const roomWidth = (await page.locator('.timeline-scroll').boundingBox()).width;
check('…each row filling the room it has', rowWidth > roomWidth - 32, `${rowWidth} of ${roomWidth}`);
check('nothing scrolls sideways', await page.locator('.timeline-scroll').evaluate((el) => el.scrollWidth <= el.clientWidth + 1));
check('the time between is in the row now that there is no tape', (await page.locator('.row-gap').first().isVisible()) && (await page.locator('.row-gap').allTextContents()).includes('6 hours'));
check('a piece still carries its own time', (await piece('The harbor').textContent()).includes('14 March 1998, 19:00'));
await shot('5-phone');
await page.setViewportSize({ width: 946, height: 1030 });
await settle();
check('back at window width, the lanes come back', await page.locator('.timeline-lanes').isVisible());

// --- Opening -----------------------------------------------------------------------------
// Chosen first, so the panel has already taken its column: opening it shifts the board, and a
// piece that moves out from under the pointer swallows the second click.
await piece('The long night').click();
await settle();
await piece('The long night').dblclick();
await settle();
check('a double-click opens the scene', (await page.locator('.timeline').count()) === 0 && (await calls('open_scene')).length >= 1);

process.exitCode = (await finish()) === 0 ? 0 : 1;
