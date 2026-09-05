// Run: node scripts/ui-flow-test.mjs (no browser or dependencies required).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const elements = new Map();
function element(id) {
  if (!elements.has(id)) {
    const classes = new Set();
    elements.set(id, {
      id, value: '', textContent: '', disabled: false, style: {}, dataset: {},
      children: [], handlers: {}, scrollTop: 0, scrollHeight: 0, clientHeight: 0,
      classList: { add: x => classes.add(x), remove: x => classes.delete(x),
        contains: x => classes.has(x), toggle(x, on) { on ? classes.add(x) : classes.delete(x); } },
      setAttribute() {}, removeAttribute() {}, focus() {},
      addEventListener(name, fn) { this.handlers[name] = fn; },
      append(child) { this.children.push(child); if (!this.value) this.value = child.value; },
      replaceChildren(...children) { this.children = children; this.value = children[0]?.value || ''; },
      showModal() { this.open = true; }, close() { this.open = false; },
    });
  }
  return elements.get(id);
}
const calls = [], listeners = new Map(), deferred = new Map();
const analysis = { sessionId: 's1', supported: true, sourceVersion: 'Java 1.21',
  detectedVersion: 'Java 1.21', typeName: 'Java', worldName: 'Test', fileCount: 2,
  byteCount: 1024, notes: [], targets: [{ displayName: 'Java 1.21' }, { displayName: 'Java 1.12' }] };
const invoke = async (name, args) => {
  calls.push({ name, args });
  if (deferred.has(name)) return deferred.get(name);
  if (name === 'analyze') return structuredClone(analysis);
  if (name === 'backend_status') return { ok: true };
  if (name === 'resource_usage') return { cpuPercent: 0, memoryBytes: 0 };
  if (name === 'is_downgrade') return false;
  if (name === 'convert') return { fileName: 'test.zip', targetVersion: 'Java 1.21', regionFiles: 1, regionChunks: 1 };
  if (name === 'save_result') return 'saved.zip';
  return null;
};
const context = vm.createContext({ console, setInterval: () => 1, clearInterval() {}, setTimeout,
  document: { getElementById: element, querySelector: element, querySelectorAll: () => [],
    createElement: () => element(Symbol()), addEventListener() {}, activeElement: element('choose-btn') },
  window: { __TAURI__: { core: { invoke }, event: { listen: async (name, fn) => listeners.set(name, fn) },
    window: { getCurrentWindow: () => ({ onCloseRequested() {} }) },
    webview: { getCurrentWebview: () => ({ onDragDropEvent() {} }) } } },
});
const run = code => vm.runInContext(code, context);
run(readFileSync(new URL('../src/main.js', import.meta.url), 'utf8'));
await new Promise(resolve => setTimeout(resolve, 0));
assert.equal(listeners.size, 2, 'Tauri progress and log listeners must initialize');
await run("analyze('test.zip')");
assert.equal(element('start-btn').disabled, false);

let resolveCheck;
deferred.set('is_downgrade', new Promise(resolve => { resolveCheck = resolve; }));
const starting = element('start-btn').handlers.click();
assert.equal(element('choose-btn').disabled, true, 'lock input before asynchronous preflight');
await element('start-btn').handlers.click();
resolveCheck(false);
await starting;
assert.equal(calls.filter(call => call.name === 'convert').length, 1, 'double click starts only one conversion');
assert.equal(element('save-btn').disabled, false);
assert.match(element('stage-label').textContent, /保存/, 'completion must explain that saving is still required');

let resolveSave;
deferred.set('pick_save_path', new Promise(resolve => { resolveSave = resolve; }));
const saving = element('save-btn').handlers.click();
assert.equal(element('choose-btn').disabled, true, 'save dialog must retain the current session');
resolveSave(null);
await saving;
assert.equal(element('save-btn').disabled, false, 'cancelling save keeps the output available');

const priorStage = element('stage-label').textContent;
listeners.get('nwc://progress')({ payload: { sessionId: 'old', percent: 2, stage: 'stale' } });
assert.equal(element('stage-label').textContent, priorStage, 'old events must not replace completion');

const replacement = run("analyze('replacement.zip')");
assert.equal(element('modal-backdrop').open, true, 'replacing an unsaved result asks first');
element('modal-cancel').handlers.click();
await replacement;
assert.equal(run('session.result.fileName'), 'test.zip', 'declining replacement retains the result');
deferred.set('pick_save_path', Promise.resolve('saved.zip'));
await element('save-btn').handlers.click();
assert.match(element('result-title').textContent, /已保存/);

let rejectConversion;
deferred.set('convert', new Promise((_, reject) => { rejectConversion = reject; }));
const cancelling = element('start-btn').handlers.click();
await new Promise(resolve => setTimeout(resolve, 0));
assert.equal(element('start-btn').disabled, true, 'wait for backend start before offering cancellation');
listeners.get('nwc://progress')({ payload: { sessionId: 's1', percent: 13, stage: '准备转换' } });
assert.equal(element('start-btn').disabled, false);
await element('start-btn').handlers.click();
assert.equal(element('start-btn').disabled, true, 'cancel is sent once');
rejectConversion(JSON.stringify({ code: 'cancelled', message: '操作已取消' }));
await cancelling;
assert.match(element('stage-label').textContent, /已取消/);
assert.equal(element('start-btn').disabled, false, 'cancelled conversions can be retried');
assert.equal(element('save-btn').disabled, true, 'cancel must not expose an old output');
deferred.delete('convert');
await element('start-btn').handlers.click();
assert.equal(element('save-btn').disabled, false, 'retry completes normally');
console.log('UI flow checks passed: events, preflight lock, double click, save, unsaved results, stale events, cancel and retry.');
