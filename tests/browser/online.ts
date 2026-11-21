import { chromium } from 'playwright';
import type { ConsoleMessage } from 'playwright';

const baseUrl = process.env.GAME_BASE_URL ?? 'https://127.0.0.1:5201';
const seconds = Number(process.env.SECONDS ?? '20');

const browser = await chromium.launch({
  headless: true,
  args: ['--enable-unsafe-webgpu', '--ignore-gpu-blocklist', '--use-angle=metal'],
});

const names = ['Alpha', 'Renée', 'Charlie'];
const errors: string[] = [];

async function runPage(index: number): Promise<string> {
  const context = await browser.newContext({
    ignoreHTTPSErrors: true,
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  let resolveStats!: (value: string) => void;
  const statsPromise = new Promise<string>(resolve => {
    resolveStats = resolve;
  });
  function inspect(message: ConsoleMessage) {
    const text = message.text();
    if (text.startsWith('MATCH_STATS ')) {
      console.log(`[page-${index}] ${text}`);
      resolveStats(text);
    }
    if (message.type() === 'error') {
      errors.push(`page-${index}: ${text}`);
    }
  }
  page.on('console', inspect);
  page.on('pageerror', error => {
    errors.push(`page-${index}: ${String(error)}`);
  });
  const url = `${baseUrl}/?auto=online&room=0&bots=2&skill=veteran&pilot=bot&seconds=${seconds}&name=${encodeURIComponent(names[index])}`;
  await page.goto(url, { waitUntil: 'domcontentloaded' });
  const timeout = page.waitForTimeout((seconds + 35) * 1000).then(() => {
    throw new Error(`page-${index} did not report its stats`);
  });
  const result = await Promise.race([statsPromise, timeout]);
  if (index === 0) {
    await page.screenshot({ path: '/tmp/gamejam-online.png' });
  }
  await context.close();
  return result;
}

const results = await Promise.all([runPage(0), runPage(1), runPage(2)]);

function parseStat(line: string, key: string): number {
  const match = line.match(new RegExp(`${key}=([0-9.]+)`));
  return match ? Number(match[1]) : 0;
}

const kills = results.map(r => parseStat(r, 'kills'));
const hitsSent = results.reduce((sum, r) => sum + parseStat(r, 'hits_sent'), 0);
const hitsApplied = results.reduce((sum, r) => sum + parseStat(r, 'hits_applied'), 0);

console.log(`ONLINE_SUMMARY kills=${kills.join('/')} hits_sent=${hitsSent} hits_applied=${hitsApplied}`);
if (kills[0] !== kills[1] || kills[1] !== kills[2]) {
  throw new Error(`pages disagreed on kills: ${kills.join(', ')}`);
}
if (hitsSent === 0 || hitsSent !== hitsApplied) {
  throw new Error(`hits mismatch: sent=${hitsSent} applied=${hitsApplied}`);
}

// Verify room rename, full room (16 max, 17th refused with 4002), and host handover on leave.
const roomCtx = await browser.newContext({ ignoreHTTPSErrors: true });
const roomPage = await roomCtx.newPage();
await roomPage.goto(`${baseUrl}/`, { waitUntil: 'domcontentloaded' });
const roomCheck = await roomPage.evaluate(async () => {
  const host = location.host;
  const openSocket = (query: string) =>
    new Promise<{ ws: WebSocket; firstText: string }>((resolve, reject) => {
      const ws = new WebSocket(`wss://${host}/ws${query}`);
      ws.onmessage = ev => {
        if (typeof ev.data === 'string') {
          resolve({ ws, firstText: ev.data });
        }
      };
      ws.onerror = err => reject(err);
    });

  const first = await openSocket('?create=TestRoom');
  const status1 = JSON.parse(first.firstText.slice(5));
  const roomId = status1.id as string;
  const renamedPromise = new Promise<string>(resolve => {
    first.ws.onmessage = ev => {
      if (typeof ev.data === 'string') resolve(ev.data);
    };
  });
  first.ws.send('title:Renamed Room');
  const renamedRaw = await renamedPromise;
  const renamedStatus = JSON.parse(renamedRaw.slice(5));

  const others: WebSocket[] = [];
  for (let i = 1; i < 16; i++) {
    const next = await openSocket(`?room=${roomId}`);
    others.push(next.ws);
  }

  const closeCode = await new Promise<number>(resolve => {
    const extra = new WebSocket(`wss://${host}/ws?room=${roomId}`);
    extra.onclose = ev => resolve(ev.code);
  });

  const secondOwnerPromise = new Promise<number>(resolve => {
    others[0].onmessage = ev => {
      if (typeof ev.data === 'string' && ev.data.startsWith('room:')) {
        const st = JSON.parse(ev.data.slice(5));
        resolve(st.owner);
      }
    };
  });
  first.ws.close();
  const newOwner = await secondOwnerPromise;
  for (const ws of others) ws.close();
  return {
    oldOwner: status1.owner,
    renamedTitle: renamedStatus.title,
    fullCloseCode: closeCode,
    newOwner,
  };
});
console.log('ROOM_CHECK', JSON.stringify(roomCheck));
if (roomCheck.renamedTitle !== 'Renamed Room') {
  throw new Error(`rename failed: ${roomCheck.renamedTitle}`);
}
if (roomCheck.fullCloseCode !== 4002) {
  throw new Error(`expected 4002 on full room, got ${roomCheck.fullCloseCode}`);
}
if (roomCheck.newOwner === roomCheck.oldOwner) {
  throw new Error('host did not transfer when owner left');
}
await roomCtx.close();

// Verify settings key rebinding + reload persistence, and solo lobby flow.
const uiCtx = await browser.newContext({
  ignoreHTTPSErrors: true,
  viewport: { width: 1280, height: 720 },
});
const uiPage = await uiCtx.newPage();
await uiPage.goto(`${baseUrl}/?name=Ren%C3%A9e&screen=settings`, { waitUntil: 'domcontentloaded' });
await uiPage.waitForTimeout(1500);
// Click a keybind button on the settings screen and press KeyG.
await uiPage.mouse.click(640, 95);
await uiPage.waitForTimeout(200);
await uiPage.keyboard.press('KeyG');
await uiPage.waitForTimeout(200);
const savedBindings = await uiPage.evaluate(() => localStorage.getItem('gamejam.bindings') ?? '');
console.log('SAVED_BINDINGS', savedBindings);
if (!savedBindings.includes('=KeyG')) {
  throw new Error(`expected KeyG in stored bindings, got: ${savedBindings}`);
}
await uiPage.reload({ waitUntil: 'domcontentloaded' });
await uiPage.waitForTimeout(1000);
const afterReload = await uiPage.evaluate(() => localStorage.getItem('gamejam.bindings') ?? '');
if (!afterReload.includes('=KeyG')) {
  throw new Error(`bindings did not survive reload: ${afterReload}`);
}
await uiCtx.close();

// Verify phone touch mode (915x412, touch=1).
const phoneCtx = await browser.newContext({
  ignoreHTTPSErrors: true,
  viewport: { width: 915, height: 412 },
  hasTouch: true,
});
const phonePage = await phoneCtx.newPage();
let phoneStats = '';
const phonePromise = new Promise<string>(resolve => {
  phonePage.on('console', msg => {
    if (msg.text().startsWith('MATCH_STATS ')) {
      phoneStats = msg.text();
      resolve(phoneStats);
    }
  });
});
await phonePage.goto(`${baseUrl}/?auto=solo&bots=2&touch=1&seconds=8&name=Ren%C3%A9e`, {
  waitUntil: 'domcontentloaded',
});
await phonePage.waitForTimeout(1500);
await phonePage.mouse.click(800, 380);
await Promise.race([
  phonePromise,
  phonePage.waitForTimeout(20000).then(() => {
    throw new Error('phone match did not report stats');
  }),
]);
console.log('PHONE_STATS', phoneStats);
await phoneCtx.close();

await browser.close();
if (errors.length) {
  throw new Error(`browser errors: ${errors.join(' | ')}`);
}
