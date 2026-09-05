// Run against a debug build with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223.
import assert from 'node:assert/strict';
const pages = await (await fetch('http://127.0.0.1:9223/json')).json();
const ws = new WebSocket(pages.find(p => p.type === 'page').webSocketDebuggerUrl);
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
let id = 0;
const pending = new Map();
ws.onmessage = event => { const data = JSON.parse(event.data); if (data.id) pending.get(data.id)?.(data); };
function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const n = ++id;
    const timer = setTimeout(() => { pending.delete(n); reject(new Error(method + ' timeout')); }, 15000);
    pending.set(n, data => { clearTimeout(timer); pending.delete(n); resolve(data); });
    ws.send(JSON.stringify({ id: n, method, params }));
  });
}
async function evaluate(expression, awaitPromise = true) {
  const data = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise });
  if (data.result?.exceptionDetails) throw new Error(JSON.stringify(data.result.exceptionDetails));
  return data.result?.result?.value;
}
async function until(expression) {
  for (let i = 0; i < 100; i++) {
    if (await evaluate(expression)) return;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  throw new Error('condition timed out: ' + expression);
}
async function reload() {
  await send('Page.reload');
  await until('typeof ready !== "undefined" && ready');
}
async function checkLayout() {
  await send('Emulation.setDeviceMetricsOverride', { width: 880, height: 680, deviceScaleFactor: 1, mobile: false });
  await evaluate('new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))');
  const layout = await evaluate(`({width: document.documentElement.scrollWidth, bottom: document.querySelector('.actions').getBoundingClientRect().bottom})`);
  console.log('minimum window layout:', layout);
  assert.ok(layout.width <= 880 && layout.bottom <= 680, 'actions fit minimum window');
  await send('Emulation.clearDeviceMetricsOverride');
}
try {
  await checkLayout();
  await reload();
  await evaluate(`analyze(${JSON.stringify(process.cwd() + '/e2e-flow-world.zip')})`);
  await evaluate("(els.target.value = 'Java 1.12', refreshDowngradeWarning())");
  assert.match(await evaluate('els.warning.textContent'), /降级/);
  await evaluate('els.start.click()', false);
  await until('els.backdrop.open');
  assert.match(await evaluate('els.modalTitle.textContent'), /降级/);
  assert.equal(await evaluate('els.choose.disabled'), true);
  await evaluate('els.modalCancel.click()');
  await until('!busy');
  assert.equal(await evaluate('els.start.disabled'), false);
  await evaluate('invoke("cancel", {sessionId: session.sessionId})');
  await evaluate('startConversion("Java 1.21")');
  assert.equal(await evaluate('els.save.disabled'), false, 'cancelled session can retry');
  await checkLayout();
  await evaluate(`invoke('save_result', {sessionId: session.sessionId, destination: ${JSON.stringify(process.cwd() + '/e2e-flow-retry.zip')}})`);
  await evaluate('invoke("shutdown_cleanup")');
  await reload();
  await evaluate(`analyze(${JSON.stringify(process.cwd() + '/e2e-missing.zip')})`, false);
  await until('els.backdrop.open');
  assert.match(await evaluate('els.modalTitle.textContent'), /解析失败/);
  await evaluate('els.modalOk.click()');
  await until('!busy');
  assert.equal(await evaluate('els.start.disabled'), true);
  assert.equal(await evaluate('els.save.disabled'), true);
  await reload();
  console.log('WebView checks passed: layout, downgrade confirmation, cancelled session retry, missing input recovery.');
} finally {
  await send('Emulation.clearDeviceMetricsOverride');
  ws.close();
}
