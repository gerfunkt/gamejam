import { chromium } from 'playwright';
import type { ConsoleMessage } from 'playwright';

const url = process.env.GAME_URL
  ?? 'https://127.0.0.1:5201/?auto=solo&bots=4&skill=veteran&pilot=bot&seconds=25&stat=1';
const seconds = Number(new URL(url).searchParams.get('seconds') ?? '25');

const browser = await chromium.launch({
  headless: true,
  args: ['--enable-unsafe-webgpu', '--ignore-gpu-blocklist', '--use-angle=metal'],
});
const page = await browser.newPage({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 720 } });

let stats: string | undefined;
let resolveStats!: (value: string) => void;
const statsPromise = new Promise<string>(resolve => { resolveStats = resolve; });
const errors: string[] = [];
function inspect(message: ConsoleMessage) {
  const text = message.text();
  console.log(`[browser:${message.type()}] ${text}`);
  if (text.startsWith('MATCH_STATS ')) {
    stats = text;
    resolveStats(text);
  }
  if (message.type() === 'error') errors.push(text);
}
page.on('console', inspect);
page.on('pageerror', error => {
  errors.push(String(error));
  console.error(`[pageerror] ${error}`);
});

await page.goto(url, { waitUntil: 'domcontentloaded' });
const timeout = page.waitForTimeout((seconds + 30) * 1000).then(() => {
  throw new Error('the match did not report its stats');
});
await Promise.race([statsPromise, timeout]);
await page.screenshot({ path: process.env.SHOT ?? '/tmp/gamejam-match.png' });
console.log(stats ?? 'no stats');
if (errors.length) throw new Error(`browser errors: ${errors.join(' | ')}`);
await browser.close();
