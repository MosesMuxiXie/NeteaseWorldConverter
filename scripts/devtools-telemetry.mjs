// 通过 WebView2 调试协议验证任务资源监控。
import assert from "node:assert/strict";

const port = process.argv[2] || "9223";
const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = pages.find((item) => item.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
let id = 0;
const pending = new Map();
ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    pending.get(message.id)(message);
    pending.delete(message.id);
  }
};

function evaluate(expression) {
  return new Promise((resolve) => {
    const messageId = ++id;
    pending.set(messageId, (message) => resolve(message.result.result.value));
    ws.send(JSON.stringify({
      id: messageId,
      method: "Runtime.evaluate",
      params: { expression, returnByValue: true, awaitPromise: true },
    }));
  });
}

const result = JSON.parse(await evaluate(`(async () => {
  setBusy(true);
  await new Promise(resolve => setTimeout(resolve, 1200));
  const usage = await window.__TAURI__.core.invoke("resource_usage");
  const state = {
    running: document.getElementById("resource-metrics").classList.contains("running"),
    animated: document.getElementById("progress-bar").classList.contains("active"),
    elapsed: document.getElementById("metric-elapsed").textContent,
    cpu: document.getElementById("metric-cpu").textContent,
    memory: document.getElementById("metric-memory").textContent,
    usage,
  };
  setBusy(false);
  state.stopped = !document.getElementById("resource-metrics").classList.contains("running")
    && !document.getElementById("progress-bar").classList.contains("active");
  return JSON.stringify(state);
})()`));
ws.close();

assert.equal(result.running, true);
assert.equal(result.animated, true);
assert.equal(result.stopped, true);
assert.match(result.elapsed, /^已运行 \d\d:\d\d$/);
assert.match(result.cpu, /^CPU \d+%$/);
assert.match(result.memory, /^内存 .+iB$/);
assert.equal(typeof result.usage.cpuPercent, "number");
assert.equal(typeof result.usage.memoryBytes, "number");
console.log("telemetry:", result);
