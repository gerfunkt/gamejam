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
}

const failures: string[] = [];

function check(condition: boolean, message: string): void {
	console.log(`${condition ? "ok  " : "FAIL"} ${message}`);
	if (!condition) failures.push(message);
}

/** Opens the game with the given hooks in its address, and waits for the menu. */
async function open(context: BrowserContext, label: string, hooks: string): Promise<Player> {
	const page = await context.newPage();
	const player: Player = { page, stats: [], errors: [] };
	page.on("console", (message) => {
		const text = message.text();
		if (text.startsWith("STAT ")) {
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
		check(player.errors.length === 0, `the page reports no error (${player.errors.length})`);
		check(stat.screen === "Playing", `the match runs (${stat.screen})`);
		check(stat.planes === "4", `the player and three bots fly (${stat.planes})`);
		check(Number(stat.fps) >= 50, `the page draws at least 50 frames a second (${stat.fps})`);
		check(Number(stat.kills) > 0, `planes are destroyed in 45 s (${stat.kills})`);
		check(Number(stat.hits_sent) > 0, `the bot that flies the player hits (${stat.hits_sent})`);
	},
};

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
