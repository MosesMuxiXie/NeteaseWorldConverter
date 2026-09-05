// devtools-e2e.mjs — 通过 WebView2 调试协议驱动应用执行完整转换流程。
import { readFileSync, existsSync, statSync } from "node:fs";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";

const port = process.argv[2] || "9223";
const inputZip = process.argv[3];
const outputZip = process.argv[4];
const target = process.argv[5] || "Java 1.21";
const inputHash = createHash('sha256').update(readFileSync(inputZip)).digest('hex');

const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = list.find((p) => p.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
ws.onmessage = (ev) => {
  const data = JSON.parse(ev.data);
  if (data.id && pending.has(data.id)) { pending.get(data.id)(data); pending.delete(data.id); }
};
function send(method, params, timeoutMs = 300000) {
  return new Promise((res, rej) => {
    const msgId = ++id;
    const timer = setTimeout(() => { pending.delete(msgId); rej(new Error(`timeout: ${method}`)); }, timeoutMs);
    pending.set(msgId, (data) => { clearTimeout(timer); res(data); });
    ws.send(JSON.stringify({ id: msgId, method, params }));
  });
}
async function evaluate(expression, awaitPromise = true) {
  const res = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise });
  if (res.result?.exceptionDetails) {
    throw new Error(JSON.stringify(res.result.exceptionDetails, null, 2).slice(0, 2000));
  }
  return res.result?.result?.value;
}

const j = (v) => JSON.stringify(v);

// 0. 关掉可能残留的模态框
await evaluate(`(document.getElementById("modal-ok").click(), document.getElementById("modal-cancel").click(), "dismissed")`, false);

// 1. 后端状态
const status = await evaluate(`window.__TAURI__.core.invoke("backend_status")`);
console.log("backend_status:", j(status));

// 2. 通过页面全局 analyze()（选择按钮的同一入口）走完整分析流程
const analysis = await evaluate(`(async () => {
  await analyze(${j(inputZip)});
  return {
    detection: document.getElementById("detection-label").textContent,
    detectionClass: document.getElementById("detection-label").className,
    targets: [...document.querySelectorAll("#target-box option")].map(o => o.value),
    targetDisabled: document.getElementById("target-box").disabled,
    startDisabled: document.getElementById("start-btn").disabled,
    sessionId: session ? session.sessionId : null,
    supported: session ? session.supported : null,
    sourceVersion: session ? session.sourceVersion : null,
    worldName: session ? session.worldName : null,
  };
})()`);
console.log("analysis:", j(analysis));
assert.equal(analysis.supported, true);
assert.equal(analysis.startDisabled, false);

// 3. 降级判断
const downgrade = await evaluate(`window.__TAURI__.core.invoke("is_downgrade", { sessionId: session.sessionId, target: ${j(target)} })`);
console.log("is_downgrade:", j(downgrade));

// 4. 转换（页面同一入口 startConversion；成功弹窗在等待期间点掉）
await evaluate(`(document.getElementById("target-box").value = ${j(target)}, window.__convDone = null, window.__convError = null, startConversion(${j(target)}).then(() => { window.__convDone = true; }).catch((e) => { window.__convError = String(e && e.message || e); }), "started")`, false);
{
  const deadline = Date.now() + 300000;
  let modalClicked = false;
  for (;;) {
    const st = await evaluate(`JSON.stringify({
      modalVisible: document.getElementById("modal-backdrop").open,
      done: window.__convDone === true,
      error: window.__convError,
      stage: document.getElementById("stage-label").textContent,
    })`);
    const s = JSON.parse(st);
    if (s.modalVisible && !modalClicked) {
      await evaluate(`(document.getElementById("modal-ok").click(), "clicked")`, false);
      modalClicked = true;
    }
    if (s.done || s.error || Date.now() > deadline) {
      if (s.error) throw new Error(`conversion failed: ${s.error}`);
      if (Date.now() > deadline) throw new Error("conversion timed out");
      break;
    }
    await new Promise((r) => setTimeout(r, 500));
  }
}
const conversion = await evaluate(`JSON.stringify({
  stage: document.getElementById("stage-label").textContent,
  progressText: document.getElementById("progress-text").textContent,
  saveDisabled: document.getElementById("save-btn").disabled,
  result: session ? session.result : null,
  logTail: document.getElementById("log-area").textContent.slice(-800),
})`);
console.log("conversion:", conversion);
assert.equal(JSON.parse(conversion).saveDisabled, false);
assert.equal(JSON.parse(conversion).result.targetVersion, target);

const protectedInput = await evaluate(`window.__TAURI__.core.invoke("save_result", { sessionId: session.sessionId, destination: ${j(inputZip)} }).then(() => false, () => true)`);
assert.equal(protectedInput, true, 'saving must never overwrite the source archive');

// 5. 保存结果
const saved = await evaluate(`window.__TAURI__.core.invoke("save_result", { sessionId: session.sessionId, destination: ${j(outputZip)} })`);
console.log("save_result:", j(saved));
assert.equal(createHash('sha256').update(readFileSync(inputZip)).digest('hex'), inputHash);

// 6. 清理
await evaluate(`window.__TAURI__.core.invoke("shutdown_cleanup")`).catch(() => {});
ws.close();

if (!existsSync(outputZip)) {
  console.error("E2E FAILED: output zip missing");
  process.exit(1);
}
console.log("output size:", statSync(outputZip).size, "bytes");
console.log("E2E OK");
