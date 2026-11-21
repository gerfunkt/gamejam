// Plays the game in headless Chromium through the menus of the page, as a player does.
//
// Usage: node play.ts <scenario> [address]
// The scenarios are the keys of `scenarios` below. The address defaults to the one where
// `cargo xtask serve` serves the game. The browser accepts the self-signed certificate, as a
// player does with one click. Node runs this file as it is: it removes the types.
import { mkdir } from "node:fs/promises";
import { chromium, type Browser, type BrowserContext, type Page } from "playwright";

const [scenario = "solo", address = "https://localhost:5201/"] = process.argv.slice(2);

/** The numbers that a page writes to its console every two seconds, with the hook `stat=1`. */
type Stat = Record<string, string>;

/** A page of the game, with what it wrote and what went wrong in it. */
interface Player {
	page: Page;
	stats: Stat[];
	errors: string[];
	/** The line in which the engine names the graphics adapter that it uses. */
	adapter: string;
}

const failures: string[] = [];

function check(condition: boolean, message: string): void {
	console.log(`${condition ? "ok  " : "FAIL"} ${message}`);
	if (!condition) failures.push(message);
}

/** Opens the game with the given hooks in its address, and waits for the menu. */
async function open(context: BrowserContext, label: string, hooks: string): Promise<Player> {
	const page = await context.newPage();
	const player: Player = { page, stats: [], errors: [], adapter: "" };
	page.on("console", (message) => {
		const text = message.text();
		if (text.includes("AdapterInfo")) {
			player.adapter = text;
		} else if (text.startsWith("STAT ")) {
			const stat = Object.fromEntries(text.slice(5).split(" ").map((pair) => pair.split("=") as [string, string]));
			player.stats.push(stat);
			console.log(`[${label}] ${text}`);
		} else if (message.type() === "error" || /panicked/.test(text)) {
			player.errors.push(text);
			console.log(`[${label}] error: ${text.slice(0, 400)}`);
		}
	});
	page.on("pageerror", (error) => {
		// The engine leaves its start function through this exception. It is not an error.
		if (/Using exceptions for control flow/.test(error.message)) return;
		player.errors.push(error.message);
		console.log(`[${label}] page error: ${error.message.slice(0, 400)}`);
	});
	await page.goto(`${address}?stat=1&${hooks}`);
	await page.locator("#menu").waitFor({ state: "visible", timeout: 90_000 });
	return player;
}

/** Sets the bots of the lobby with its own controls. Only the host can. */
async function chooseBots(page: Page, bots: number, skill: string): Promise<void> {
	// The page fills the lobby with its next frame, and it answers each click with the frame after.
	const shows = (text: string) => page.waitForFunction((want) => document.querySelector("#bots-count")?.textContent === want, text);
	await page.waitForFunction(() => document.querySelector("#bots-count")?.textContent !== "");
	let count = Number(await page.locator("#bots-count").textContent());
	while (count !== bots) {
		await page.getByRole("button", { name: count < bots ? "More Bots" : "Fewer Bots" }).click();
		count += count < bots ? 1 : -1;
		await shows(String(count));
	}
	await page.locator("#skill").selectOption(skill);
}

async function shot(player: Player, name: string): Promise<void> {
	await mkdir("out", { recursive: true });
	await player.page.screenshot({ path: `out/${name}.png` });
}

const last = (player: Player): Stat => player.stats.at(-1) ?? {};

const scenarios: Record<string, (browser: Browser) => Promise<void>> = {
	/** A solo match against three bots. A bot flies the plane of the player. */
	async solo(browser) {
		const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 720 } });
		const player = await open(context, "solo", "pilot=bot");
		await player.page.locator("#name").fill("Zoë");
		await shot(player, "menu");
		await player.page.getByRole("button", { name: "Fly Solo" }).click();
		await chooseBots(player.page, 3, "Veteran");
		await shot(player, "lobby");
		await player.page.getByRole("button", { name: "Start Match" }).click();
		await player.page.locator("#hud").waitFor({ state: "visible" });
		await player.page.waitForTimeout(8_000);
		await shot(player, "solo-early");
		await player.page.waitForTimeout(37_000);
		await shot(player, "solo");

		const stat = last(player);
		const backend = /backend: (\w+)/.exec(player.adapter)?.[1] ?? "none named";
		const health = (await player.page.locator("#health").boundingBox()) ?? { x: 999, y: 999, width: 0, height: 0 };
		check(health.x < 40 && health.y < 40 && health.width > 200, `the state of the plane shows at the top left (${health.x.toFixed(0)}, ${health.y.toFixed(0)})`);
		check(backend === "BrowserWebGpu", `the engine draws with WebGPU (${backend})`);
		check(player.errors.length === 0, `the page reports no error (${player.errors.length})`);
		check(stat.screen === "Playing", `the match runs (${stat.screen})`);
		check(stat.planes === "4", `the player and three bots fly (${stat.planes})`);
		check(Number(stat.fps) >= 50, `the page draws at least 50 frames a second (${stat.fps})`);
		check(Number(stat.kills) > 0, `planes are destroyed in 45 s (${stat.kills})`);
		check(Number(stat.hits_sent) > 0, `the bot that flies the player hits (${stat.hits_sent})`);
	},
};

Object.assign(scenarios, {
	/** A solo match on the client for browsers without WebGPU, which the address forces here. */
	async fallback(browser: Browser) {
		const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 720 } });
		const player = await open(context, "fallback", "pilot=bot&backend=webgl2");
		check(await player.page.locator("#fallback").isVisible(), "the menu warns that the graphics run slower");
		await shot(player, "fallback-menu");
		await player.page.getByRole("button", { name: "Fly Solo" }).click();
		await chooseBots(player.page, 3, "Veteran");
		await player.page.getByRole("button", { name: "Start Match" }).click();
		await player.page.locator("#hud").waitFor({ state: "visible" });
		await player.page.waitForTimeout(30_000);
		await shot(player, "fallback");

		const stat = last(player);
		const backend = /backend: (\w+)/.exec(player.adapter)?.[1] ?? "none named";
		check(backend === "Gl", `the engine draws with WebGL2 (${backend})`);
		check(stat.backend === "webgl2", `the page loaded the WebGL2 client (${stat.backend})`);
		check(player.errors.length === 0, `the page reports no error (${player.errors.length})`);
		check(stat.planes === "4", `the player and three bots fly (${stat.planes})`);
		check(Number(stat.fps) >= 30, `the page draws at least 30 frames a second (${stat.fps})`);
		check(Number(stat.kills) > 0, `planes are destroyed in 30 s (${stat.kills})`);
	},
});

/** Lets a player enter a name, and open the room list. */
async function findRooms(player: Player, name: string): Promise<void> {
	await player.page.locator("#name").fill(name);
	await player.page.getByRole("button", { name: "Find a Room" }).click();
	await player.page.locator("#rooms").waitFor({ state: "visible" });
}

/** The names in the lobby of a player, as the page lists them. */
async function lobbyNames(player: Player): Promise<string[]> {
	return player.page.locator("#players .name").allTextContents();
}

Object.assign(scenarios, {
	/** Three players in one room, with two bots that the host flies. Bots fly all three players. */
	async online(browser: Browser) {
		const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 960, height: 540 } });
		const title = `Test ${Date.now() % 100000}`;
		const host = await open(context, "host", "pilot=bot");
		await findRooms(host, "Zoë");
		await host.page.locator("#room-title").fill(title);
		await host.page.getByRole("button", { name: "Create Room" }).click();
		await host.page.locator("#lobby").waitFor({ state: "visible" });
		await chooseBots(host.page, 2, "Veteran");

		const guests: Player[] = [];
		for (const name of ["Björn", "北"]) {
			const guest = await open(context, name, "pilot=bot");
			await findRooms(guest, name);
			const room = guest.page.locator("#room-list li", { hasText: title });
			await room.getByRole("button", { name: "Join Room" }).click();
			await guest.page.locator("#lobby").waitFor({ state: "visible" });
			guests.push(guest);
		}
		const players = [host, ...guests];
		await host.page.waitForFunction(() => document.querySelectorAll("#players li").length === 3);
		await guests[1].page.waitForFunction(() => document.querySelectorAll("#players li").length === 3);
		// Each member announces its name when a player joins, so the names arrive a moment later.
		const wanted = "Zoë (host)|Björn|北 (you)";
		const listed = (text: string) => [...document.querySelectorAll("#players .name")].map((name) => name.textContent).join("|") === text;
		await guests[1].page.waitForFunction(listed, wanted, { timeout: 5_000 }).catch(() => {});
		const names = (await lobbyNames(guests[1])).join("|");
		check(names === wanted, `the last guest sees the three names, with their accents (${names})`);
		check(await guests[0].page.locator("#start").isHidden(), "a guest has no start button");
		check((await guests[0].page.locator("#bots-count").textContent()) === "2", "a guest sees the bots of the host");
		await shot(host, "online-lobby");

		await host.page.getByRole("button", { name: "Start Match" }).click();
		for (const player of players) await player.page.locator("#hud").waitFor({ state: "visible" });
		await host.page.waitForTimeout(45_000);
		await shot(guests[0], "online");

		for (const player of players) {
			const stat = last(player);
			const label = player === host ? "the host" : "a guest";
			check(player.errors.length === 0, `${label} reports no error (${player.errors.length})`);
			check(stat.planes === "5", `${label} sees three players and two bots (${stat.planes})`);
			check(Number(stat.fps) >= 45, `${label} draws at least 45 frames a second (${stat.fps})`);
		}

		// The host ends the match from the pause menu, and every player sees the same results.
		await host.page.keyboard.press("Escape");
		await host.page.getByRole("button", { name: "End Match" }).click();
		await host.page.getByRole("button", { name: "Confirm: End Match" }).click();
		const tables: string[] = [];
		for (const player of players) {
			await player.page.locator("#results").waitFor({ state: "visible" });
			tables.push((await player.page.locator("#result-rows tr").allTextContents()).join("|"));
		}
		check(tables[0].includes("Zoë") && tables.every((table) => table === tables[0]), `all pages show the same results (${tables[0]})`);
		await shot(guests[1], "online-results");
		// The pages write their numbers at different moments, so they are compared after the last kill.
		await host.page.waitForTimeout(2_500);
		const kills = players.map((player) => last(player).kills);
		check(Number(kills[0]) > 0 && kills.every((count) => count === kills[0]), `all pages counted the same kills (${kills.join(", ")})`);
	},
});

/** Waits for the next line of numbers of a page, and returns it. */
async function nextStat(player: Player): Promise<Stat> {
	const seen = player.stats.length;
	for (let waited = 0; waited < 60 && player.stats.length === seen; waited++) await player.page.waitForTimeout(100);
	return last(player);
}

Object.assign(scenarios, {
	/** A person at the mouse and the keys: steering, the throttle, the gun, the pause, and a changed key. */
	async controls(browser: Browser) {
		const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 720 } });
		const player = await open(context, "controls", "");
		const { page } = player;
		await page.getByRole("button", { name: "Fly Solo" }).click();
		await chooseBots(page, 0, "Rookie");
		await page.getByRole("button", { name: "Start Match" }).click();
		await page.locator("#hud").waitFor({ state: "visible" });

		// A cursor at the centre flies straight, and a cursor to the right turns the plane to the right.
		await page.mouse.move(640, 360);
		await page.waitForTimeout(1_000);
		const straight = [Number((await nextStat(player)).heading), Number((await nextStat(player)).heading)];
		check(Math.abs(straight[1] - straight[0]) < 3, `a cursor at the centre keeps the heading (${straight.join(" to ")} degrees)`);
		// A cursor a little right of the centre gives a gentle turn, which the numbers can follow.
		await page.mouse.move(720, 360);
		await nextStat(player);
		const turned = Number((await nextStat(player)).heading);
		const right = ((turned - straight[1] + 540) % 360) - 180;
		check(right > 8 && right < 150, `a cursor right of the centre turns the plane to the right (${right.toFixed(0)} degrees in 2 to 4 s)`);
		await page.mouse.move(640, 360);

		// The wheel sets the throttle, and the left button fires the gun.
		const before = Number((await nextStat(player)).throttle);
		await page.mouse.wheel(0, -300);
		await page.mouse.down();
		const firing = await nextStat(player);
		await page.mouse.up();
		check(Number(firing.throttle) > before, `the wheel opens the throttle (${before} to ${firing.throttle})`);
		check(Number(firing.bullets) > 0, `the left button fires the gun (${firing.bullets} bullets fly)`);

		// The flare key releases one of the three flares.
		const full = Number((await nextStat(player)).flares);
		await page.keyboard.press("KeyE");
		const left = Number((await nextStat(player)).flares);
		check(full === 3 && left === 2, `the flare key releases a flare (${full} to ${left} flares)`);
		await shot(player, "display");

		// The pause key stops a solo match, and "Resume" continues it.
		await page.keyboard.press("Escape");
		await page.locator("#pause").waitFor({ state: "visible" });
		await nextStat(player);
		const paused = [await nextStat(player), await nextStat(player)];
		check(paused[1].screen === "Paused" && paused[0].z === paused[1].z, `the plane waits while the menu is open (z ${paused[0].z} and ${paused[1].z})`);

		// The menu switches the page to full screen, and back.
		const isFull = () => page.evaluate(() => document.fullscreenElement !== null);
		await page.getByRole("button", { name: "Full Screen" }).click();
		await page.waitForFunction(() => document.fullscreenElement !== null, null, { timeout: 3_000 }).catch(() => {});
		check(await isFull(), "the button switches the page to full screen");
		await page.getByRole("button", { name: "Full Screen" }).click();
		await page.waitForFunction(() => document.fullscreenElement === null, null, { timeout: 3_000 }).catch(() => {});
		check(!(await isFull()), "the button leaves full screen again");

		// A key changes in the settings, and the new key works.
		await page.getByRole("button", { name: "Settings" }).click();
		const boost = page.locator("#binding-rows tr", { hasText: "Boost" }).getByRole("button");
		const shows = (text: string) => boost.filter({ hasText: text }).waitFor({ timeout: 3_000 }).catch(() => {});
		await boost.click();
		await shows("Press a Key");
		await page.keyboard.press("KeyB");
		await shows("B");
		check((await boost.textContent()) === "B", `the settings show the new key (${await boost.textContent()})`);
		await page.getByRole("button", { name: "Back" }).click();
		await page.getByRole("button", { name: "Resume" }).click();
		await page.locator("#pause").waitFor({ state: "hidden" });
		const resumed = await nextStat(player);
		check(resumed.screen === "Playing" && resumed.z !== paused[1].z, `the match runs again after the pause (z ${resumed.z})`);
		await page.keyboard.down("KeyB");
		await nextStat(player);
		const boosted = Number((await nextStat(player)).speed);
		await page.keyboard.up("KeyB");
		check(boosted > 125, `the new boost key gives speed (${boosted} m/s)`);

		// The choice survives a new visit.
		await page.reload();
		await page.locator("#menu").waitFor({ state: "visible", timeout: 90_000 });
		await page.getByRole("button", { name: "Settings" }).click();
		const kept = await page.locator("#binding-rows tr", { hasText: "Boost" }).getByRole("button").textContent();
		check(kept === "B", `the key is still there after a reload (${kept})`);
		check(player.errors.length === 0, `the page reports no error (${player.errors.length})`);
		await shot(player, "settings");
	},

	/** The lobby of a room: its name, the removal of a player, and a host that leaves during a match. */
	async lobby(browser: Browser) {
		const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 960, height: 540 } });
		const title = `Lobby ${Date.now() % 100000}`;
		const host = await open(context, "host", "pilot=bot");
		await findRooms(host, "Host");
		await host.page.locator("#room-title").fill(title);
		await host.page.getByRole("button", { name: "Create Room" }).click();
		await host.page.locator("#lobby").waitFor({ state: "visible" });
		await chooseBots(host.page, 2, "Pilot");
		const join = async (name: string, room: string): Promise<Player> => {
			const guest = await open(context, name, "pilot=bot");
			await findRooms(guest, name);
			await guest.page.locator("#room-list li", { hasText: room }).getByRole("button", { name: "Join Room" }).click();
			await guest.page.locator("#lobby").waitFor({ state: "visible" });
			return guest;
		};
		const second = await join("Second", title);
		const third = await join("Third", title);

		// The host renames the room. The members and the room list show the new name.
		const renamed = `${title} B`;
		await host.page.locator("#new-title").fill(renamed);
		await host.page.getByRole("button", { name: "Rename Room" }).click();
		await second.page.waitForFunction((want) => document.querySelector("#lobby-title")?.textContent === want, renamed, { timeout: 5_000 }).catch(() => {});
		check((await second.page.locator("#lobby-title").textContent()) === renamed, "a guest sees the new name of the room");
		const watcher = await open(context, "watcher", "");
		await findRooms(watcher, "Watcher");
		const entry = watcher.page.locator("#room-list li", { hasText: renamed });
		await entry.waitFor({ state: "visible", timeout: 5_000 }).catch(() => {});
		const listed = (await entry.textContent().catch(() => "")) ?? "";
		check(listed.includes("3 of 16") && listed.includes("In the lobby, 2 bots (Pilot)"), `the room list shows the room, its players, and what it does (${listed})`);
		check(await second.page.locator("#rename").isHidden(), "a guest cannot rename the room");

		// The host removes the third player, who learns why.
		const row = host.page.locator("#players li", { hasText: "Third" });
		await row.getByRole("button", { name: "Remove Player" }).click();
		await row.getByRole("button", { name: "Confirm Removal" }).click();
		await third.page.locator("#lost").waitFor({ state: "visible", timeout: 5_000 }).catch(() => {});
		const reason = (await third.page.locator("#lost-note").textContent()) ?? "";
		check(reason.includes("The host removed you"), `the removed player learns why (${reason})`);
		await host.page.waitForFunction(() => document.querySelectorAll("#players li").length === 2);

		// The host leaves during a match. The second player hosts on, and flies the bots.
		await host.page.getByRole("button", { name: "Start Match" }).click();
		await second.page.locator("#hud").waitFor({ state: "visible" });
		await second.page.waitForTimeout(6_000);
		check(last(second).host === "false" && last(second).planes === "4", `the guest sees two players and two bots (${last(second).planes})`);
		await host.page.close();
		await second.page.waitForTimeout(6_000);
		const after = last(second);
		check(after.host === "true", `the guest hosts the room after the host left (${after.host})`);
		check(after.planes === "3" && after.screen === "Playing", `the match runs on with one player and two bots (${after.planes}, ${after.screen})`);
		await second.page.keyboard.press("Escape");
		check(await second.page.getByRole("button", { name: "End Match" }).isVisible(), "the new host can end the match");
		check(second.errors.length === 0, `the new host reports no error (${second.errors.length})`);
		await shot(second, "lobby-new-host");
	},
});

Object.assign(scenarios, {
	/** A phone, held sideways: the stick, the buttons, the tilt, and the pause button, all by touch. */
	async phone(browser: Browser) {
		const context = await browser.newContext({
			ignoreHTTPSErrors: true,
			viewport: { width: 915, height: 412 },
			// A phone has about three device pixels for each CSS pixel.
			deviceScaleFactor: 3,
			hasTouch: true,
			isMobile: true,
		});
		const player = await open(context, "phone", "touch=1");
		const { page } = player;
		check(await page.locator("#game").evaluate((canvas) => canvas.classList.contains("resting")), "a phone draws no sky behind its menus");
		const touch = await context.newCDPSession(page);
		const fingers = (type: "touchStart" | "touchMove" | "touchEnd", points: { x: number; y: number; id: number }[]) =>
			touch.send("Input.dispatchTouchEvent", { type, touchPoints: points.map((point) => ({ ...point, radiusX: 8, radiusY: 8 })) });
		const middle = async (selector: string, id: number) => {
			const box = (await page.locator(selector).boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
			return { x: box.x + box.width / 2, y: box.y + box.height / 2, id };
		};

		await page.getByRole("button", { name: "Fly Solo" }).tap();
		await chooseBots(page, 0, "Rookie");
		await page.getByRole("button", { name: "Start Match" }).tap();
		await page.locator("#touch").waitFor({ state: "visible" });
		check(await page.locator("#touch-fire").isVisible(), "the buttons of a phone show in a match");
		const health = (await page.locator("#health").boundingBox()) ?? { x: 999, y: 999, width: 0, height: 0 };
		check(health.x < 40 && health.y < 40 && health.width > 60, `the state of the plane shows at the top left (${health.x.toFixed(0)}, ${health.y.toFixed(0)})`);
		const slider = (await page.locator("#throttle-slide").boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
		check(health.y + health.height <= slider.y, `the state of the plane ends above the throttle slider (${(health.y + health.height).toFixed(0)} and ${slider.y.toFixed(0)})`);
		const sky = (await page.locator("#game").boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
		check(sky.width === 915 && sky.height === 412, `the picture fills the screen of the phone (${sky.width} by ${sky.height})`);
		check((await page.locator("#touch-tilt").getAttribute("aria-pressed")) === "true", "the tilt steers from the start");

		// The stick, pulled to the right, turns the plane to the right.
		const before = Number((await nextStat(player)).heading);
		const stick = await middle("#stick", 1);
		await fingers("touchStart", [stick]);
		await fingers("touchMove", [{ ...stick, x: stick.x + 22 }]);
		await nextStat(player);
		const turned = Number((await nextStat(player)).heading);
		const right = ((turned - before + 540) % 360) - 180;
		check(right > 8, `the stick turns the plane to the right (${right.toFixed(0)} degrees)`);

		// A second finger holds the gun button while the first one steers.
		const gun = await middle("#touch-fire", 2);
		await fingers("touchMove", [{ ...stick, x: stick.x + 22 }, gun]);
		const firing = await nextStat(player);
		check(Number(firing.bullets) > 0, `the gun button fires while the stick steers (${firing.bullets} bullets fly)`);
		await fingers("touchEnd", []);

		// A tap releases a flare, and the slider sets the throttle.
		await page.locator("#touch-flare").tap();
		await page.locator("#touch-throttle").fill("1");
		const tapped = await nextStat(player);
		check(tapped.flares === "2", `a tap on the flare button releases a flare (${tapped.flares} left)`);
		check(tapped.throttle === "1.00", `the slider opens the throttle (${tapped.throttle})`);

		// The tilt of the phone steers, from the pose in which the phone gives its first reading.
		const lean = (beta: number, gamma: number) =>
			page.evaluate(([b, g]) => window.dispatchEvent(new DeviceOrientationEvent("deviceorientation", { alpha: 0, beta: b, gamma: g })), [beta, gamma]);
		// The phone lies on its left side, with its top at the left, and leans back a little.
		await page.locator("#touch-throttle").fill("0.2");
		await lean(0, -60);
		await page.waitForTimeout(300);
		const level = [Number((await nextStat(player)).heading), Number((await nextStat(player)).heading)];
		check(Math.abs(((level[1] - level[0] + 540) % 360) - 180) < 4, `a phone that stays in its pose flies straight (${level.join(" to ")})`);
		// A small roll to the right, as with a steering wheel, turns the plane gently to the right.
		await lean(6, -60);
		await nextStat(player);
		const leaned = Number((await nextStat(player)).heading);
		const swing = ((leaned - level[1] + 540) % 360) - 180;
		check(swing > 4 && swing < 40, `a small roll of the phone turns the plane gently to the right (${swing.toFixed(0)} degrees)`);
		await lean(0, -60);
		await shot(player, "phone");
		// The tilt button turns the tilt steering off, and the stick steers alone.
		await page.locator("#touch-tilt").tap();
		check((await page.locator("#touch-tilt").getAttribute("aria-pressed")) === "false", "the tilt button turns the tilt steering off");

		// The pause button opens the menu, and a tap resumes.
		await page.locator("#touch-pause").tap();
		await page.locator("#pause").waitFor({ state: "visible" });
		check(await page.locator("#touch").isHidden(), "the controls hide while the menu is open");
		await page.getByRole("button", { name: "Resume" }).tap();
		await page.locator("#touch").waitFor({ state: "visible" });
		check(player.errors.length === 0, `the page reports no error (${player.errors.length})`);
	},

	/** A solo match against three bots, and the events that the sound raised in it. */
	async sound(browser: Browser) {
		const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 720 } });
		const player = await open(context, "sound", "pilot=bot");
		// A line is "SOUND <event> <ids of the sounds that started, separated by commas>".
		const raised = new Map<string, number>();
		player.page.on("console", (message) => {
			const [, event] = /^SOUND (\S+)/.exec(message.text()) ?? [];
			if (event) raised.set(event, (raised.get(event) ?? 0) + 1);
		});
		await player.page.getByRole("button", { name: "Fly Solo" }).click();
		await chooseBots(player.page, 3, "Veteran");
		await player.page.getByRole("button", { name: "Start Match" }).click();
		await player.page.locator("#hud").waitFor({ state: "visible" });
		await player.page.waitForTimeout(40_000);

		console.log(`events: ${[...raised].map(([event, count]) => `${event}=${count}`).join(" ")}`);
		for (const event of ["match_start", "engine_loop", "gun_fire", "hit_taken"]) {
			check(raised.has(event), `the sound raised ${event} (${raised.get(event) ?? 0})`);
		}
		const threat = (raised.get("missile_incoming") ?? 0) + (raised.get("enemy_lock_on_me") ?? 0);
		check(threat > 0, `the sound raised missile_incoming or enemy_lock_on_me (${threat})`);
		const sounds = Number(last(player).sounds);
		check(sounds > 50, `more than 50 sounds started (${sounds})`);
		check(player.errors.length === 0, `the page reports no error (${player.errors.length})`);
	},
});

Object.assign(scenarios, {
	/** The entry screen and the menus, on a device that is too slow to draw the sky behind them. */
	async menu(browser: Browser) {
		const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 720 } });
		const player = await open(context, "menu", "rest=1");
		const { page } = player;
		const rests = () => page.evaluate(() => document.querySelector("#game")?.classList.contains("resting") ?? false);

		// The entry screen says what the game has, and offers full screen.
		const features = await page.locator("#features li").count();
		check(features >= 5, `the entry screen lists the features of the game (${features})`);
		check(await page.locator("#fullscreen-menu").isVisible(), "the entry screen offers full screen");
		await page.waitForFunction(() => document.querySelector("#game")?.classList.contains("resting"), null, { timeout: 3_000 }).catch(() => {});
		check(await rests(), "the game draws no sky behind the entry screen of a slow device");
		await shot(player, "entry");
		await page.locator("#fullscreen-menu").click();
		await page.waitForFunction(() => document.fullscreenElement !== null, null, { timeout: 3_000 }).catch(() => {});
		check(await page.evaluate(() => document.fullscreenElement !== null), "the button of the entry screen switches to full screen");
		check((await page.locator("#fullscreen-menu").textContent()) === "Leave Full Screen", "the button then offers the way back");
		await page.locator("#fullscreen-menu").click();
		await page.waitForFunction(() => document.fullscreenElement === null, null, { timeout: 3_000 }).catch(() => {});

		// "How to Play" explains the game, and leads back.
		await page.getByRole("button", { name: "How to Play" }).click();
		await page.locator("#help").waitFor({ state: "visible" });
		check((await page.locator("#help li:visible").count()) >= 5, "the help names the controls of this device");
		await page.getByRole("button", { name: "Back" }).click();
		await page.locator("#menu").waitFor({ state: "visible" });

		// The lobby answers at once, and the match draws the sky again.
		await page.getByRole("button", { name: "Fly Solo" }).click();
		await chooseBots(page, 2, "Pilot");
		check((await page.locator("#bots-count").textContent()) === "2", "the lobby of a slow device takes the choice of the bots");
		await page.getByRole("button", { name: "Start Match" }).click();
		await page.locator("#hud").waitFor({ state: "visible" });
		await nextStat(player);
		const stat = await nextStat(player);
		check(!(await rests()), "the game draws the sky in a match");
		check(stat.screen === "Playing" && stat.planes === "3", `the match runs with the player and two bots (${stat.screen}, ${stat.planes})`);
		check(player.errors.length === 0, `the page reports no error (${player.errors.length})`);
	},
});

Object.assign(scenarios, {
	/** The sky, the clouds, and the effects: a solo match on each client, with pictures early and late. */
	async sky(browser: Browser) {
		for (const [client, hooks, adapter] of [
			["webgpu", "pilot=bot", "BrowserWebGpu"],
			["webgl2", "pilot=bot&backend=webgl2", "Gl"],
		]) {
			const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 720 } });
			const player = await open(context, `sky-${client}`, hooks);
			await player.page.getByRole("button", { name: "Fly Solo" }).click();
			await chooseBots(player.page, 3, "Veteran");
			await player.page.getByRole("button", { name: "Start Match" }).click();
			await player.page.locator("#hud").waitFor({ state: "visible" });
			await player.page.waitForTimeout(8_000);
			await shot(player, `sky-${client}-1`);
			await player.page.waitForTimeout(22_000);
			await shot(player, `sky-${client}-2`);

			const stat = last(player);
			const backend = /backend: (\w+)/.exec(player.adapter)?.[1] ?? "none named";
			check(backend === adapter, `the ${client} client draws with ${adapter} (${backend})`);
			check(player.errors.length === 0, `the ${client} client reports no error (${player.errors.length})`);
			check(Number(stat.fps) >= 50, `the ${client} client draws at least 50 frames a second (${stat.fps})`);
			check(Number(stat.kills) > 0, `planes are destroyed in 30 s on the ${client} client (${stat.kills})`);
			await context.close();
		}
	},
});

const run = scenarios[scenario];
if (!run) {
	console.log(`Unknown scenario "${scenario}". The scenarios are: ${Object.keys(scenarios).join(", ")}.`);
	process.exit(2);
}
const browser = await chromium.launch({
	channel: "chromium",
	args: ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-angle=vulkan", "--ignore-gpu-blocklist"],
});
try {
	await run(browser);
} catch (error) {
	failures.push(String(error));
	console.log(`FAIL ${String(error).slice(0, 600)}`);
} finally {
	await browser.close();
}
console.log(failures.length === 0 ? "PASS" : `FAILED: ${failures.length} of the checks`);
process.exit(failures.length === 0 ? 0 : 1);
