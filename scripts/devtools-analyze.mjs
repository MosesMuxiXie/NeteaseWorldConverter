// devtools-analyze.mjs — 仅执行 analyze 并打印识别结果。
const port = process.argv[2] || "9223";
const inputZip = process.argv[3];
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
function send(method, params, timeoutMs = 180000) {
  return new Promise((res, rej) => {
    const msgId = ++id;
    const timer = setTimeout(() => { pending.delete(msgId); rej(new Error(`timeout: ${method}`)); }, timeoutMs);
    pending.set(msgId, (data) => { clearTimeout(timer); res(data); });
    ws.send(JSON.stringify({ id: msgId, method, params }));
  });
}
async function evaluate(expression, awaitPromise = true) {
  const res = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise });
  if (res.result?.exceptionDetails) throw new Error(JSON.stringify(res.result.exceptionDetails).slice(0, 1500));
  return res.result?.result?.value;
}
// 关掉残留模态框
await evaluate(`(document.getElementById("modal-ok").click(), document.getElementById("modal-cancel").click(), "ok")`, false);
const result = await evaluate(`(async () => {
  await analyze(${JSON.stringify(inputZip)});
  return JSON.stringify({
    detection: document.getElementById("detection-label").textContent,
    session: session ? { sessionId: session.sessionId, sourceVersion: session.sourceVersion, supported: session.supported, worldName: session.worldName, typeName: session.typeName } : null,
    errorModal: document.getElementById("modal-body").textContent,
  });
})()`);
console.log(result);
ws.close();
