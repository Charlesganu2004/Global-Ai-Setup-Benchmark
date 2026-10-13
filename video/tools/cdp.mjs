// A small DevTools-protocol driver: starts a headless Chromium-family browser with its own profile folder
// under video/, opens one page and exposes send(). Uses Node's built-in WebSocket client, no packages.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';

export const VIDEO_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const CANDIDATES = [
  process.env.PGR_BROWSER,
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  '/usr/bin/chromium', '/usr/bin/google-chrome', '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
].filter(Boolean);

export function findBrowser() {
  for (const c of CANDIDATES) if (fs.existsSync(c)) return c;
  throw new Error('No Edge or Chrome found. Set PGR_BROWSER to the browser executable.');
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const LIVE = new Set();   // profile folders of the browsers this process has open

async function waitForJson(url, tries = 80) {
  for (let i = 0; i < tries; i++) {
    try {
      const r = await fetch(url);
      if (r.ok) return await r.json();
    } catch { /* not up yet */ }
    await sleep(150);
  }
  throw new Error('browser did not open its debugging port: ' + url);
}

export async function launchPage({ width, height, scale = 1, profile = 'edge-profile' }) {
  const exe = findBrowser();
  // A fresh profile folder for every browser and a port the browser picks itself: no clash with a window
  // left over from a killed run.
  const dir = path.join(VIDEO_DIR, '.' + profile + '-' + randomBytes(3).toString('hex'));
  for (const old of fs.readdirSync(VIDEO_DIR)) {   // folders of earlier browsers that outlived their window
    if (old.startsWith('.' + profile + '-') && !LIVE.has(path.join(VIDEO_DIR, old))) { try { fs.rmSync(path.join(VIDEO_DIR, old), { recursive: true, force: true }); } catch { /* still locked */ } }
  }
  fs.mkdirSync(dir, { recursive: true });
  LIVE.add(dir);
  const args = [
    '--headless=new', '--remote-debugging-port=0', `--user-data-dir=${dir}`,
    `--window-size=${width},${height}`, '--hide-scrollbars', '--mute-audio', '--no-first-run',
    '--no-default-browser-check', '--disable-extensions', '--disable-sync', '--disable-background-networking',
    '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows',
    '--allow-file-access-from-files', '--force-color-profile=srgb', '--disable-lcd-text',
    '--font-render-hinting=none', '--disable-features=CalculateNativeWinOcclusion,Translate', 'about:blank',
  ];
  const child = spawn(exe, args, { stdio: 'ignore', windowsHide: true });
  const exited = new Promise((r) => child.once('exit', r));
  let port = 0;
  for (let i = 0; i < 160 && !port; i++) {
    try { port = Number(fs.readFileSync(path.join(dir, 'DevToolsActivePort'), 'utf8').split(/\r?\n/)[0]); } catch { /* not written yet */ }
    if (!port) await sleep(150);
  }
  if (!port) throw new Error('the browser did not report its debugging port');
  await waitForJson(`http://127.0.0.1:${port}/json/version`);
  const list = await waitForJson(`http://127.0.0.1:${port}/json/list`);
  const target = list.find((t) => t.type === 'page') || list[0];
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = () => rej(new Error('websocket failed')); });
  let id = 0;
  const pending = new Map();
  const listeners = new Map();
  ws.onmessage = (ev) => {
    const msg = JSON.parse(typeof ev.data === 'string' ? ev.data : Buffer.from(ev.data).toString('utf8'));
    if (msg.id && pending.has(msg.id)) {
      const { res, rej } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) rej(new Error(msg.error.message + ' (' + JSON.stringify(msg.error.data || '') + ')')); else res(msg.result);
    } else if (msg.method && listeners.has(msg.method)) {
      for (const fn of listeners.get(msg.method)) fn(msg.params);
    }
  };
  const send = (method, params = {}) => new Promise((res, rej) => {
    const n = ++id;
    pending.set(n, { res, rej });
    ws.send(JSON.stringify({ id: n, method, params }));
  });
  const on = (method, fn) => { if (!listeners.has(method)) listeners.set(method, []); listeners.get(method).push(fn); };

  await send('Page.enable');
  await send('Runtime.enable');
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: scale, mobile: false });
  await send('Emulation.setDefaultBackgroundColorOverride', { color: { r: 10, g: 10, b: 12, a: 1 } });

  const evaluate = async (expression, awaitPromise = true) => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise, returnByValue: true });
    if (r.exceptionDetails) {
      const d = r.exceptionDetails;
      throw new Error('page error: ' + (d.exception && d.exception.description || d.text) + ' at line ' + d.lineNumber);
    }
    return r.result.value;
  };

  const logs = [];
  on('Runtime.consoleAPICalled', (p) => logs.push(p.type + ': ' + p.args.map((a) => a.value ?? a.description).join(' ')));
  on('Runtime.exceptionThrown', (p) => logs.push('exception: ' + (p.exceptionDetails.exception?.description || p.exceptionDetails.text)));

  const close = async () => {
    // Edge hands the real browser to another process, so the one this script started cannot be killed: ask the
    // browser itself to quit over its own debugging connection.
    try {
      const v = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      const bws = new WebSocket(v.webSocketDebuggerUrl);
      await new Promise((r) => { bws.onopen = r; bws.onerror = r; });
      bws.send(JSON.stringify({ id: 1, method: 'Browser.close' }));
      await sleep(700);
    } catch { /* already gone */ }
    try { ws.close(); } catch { /* already closed */ }
    if (process.platform === 'win32') spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' }); else child.kill();
    await Promise.race([exited, sleep(2000)]);
    LIVE.delete(dir);
    try { fs.rmSync(dir, { recursive: true, force: true, maxRetries: 12, retryDelay: 500 }); } catch { /* profile still locked, removed by the next run */ }
  };
  return { send, on, evaluate, close, logs };
}

export async function openAndWait(page, url, readyExpr = 'window.PGR && window.PGR.ready') {
  await page.send('Page.navigate', { url });
  for (let i = 0; i < 200; i++) {
    await sleep(100);
    let ok = false;
    try { ok = await page.evaluate(`!!(${readyExpr})`); } catch { /* page still loading */ }
    if (ok) return;
  }
  throw new Error('page never became ready: ' + url + '\n' + page.logs.join('\n'));
}

