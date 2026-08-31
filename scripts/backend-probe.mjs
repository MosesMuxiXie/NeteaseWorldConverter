// backend-probe.mjs — 查询运行中应用的 backend_status（用于验证安装/便携布局的资源定位）。
const port = process.argv[2] || "9224";
const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = list.find((p) => p.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
ws.onmessage = (ev) => {
  const data = JSON.parse(ev.data);
  if (data.id === 1) {
    console.log("backend_status:", JSON.stringify(data.result?.result?.value ?? data.result?.exceptionDetails ?? data.result));
    ws.close();
    process.exit(0);
  }
};
ws.send(JSON.stringify({
  id: 1,
  method: "Runtime.evaluate",
  params: {
    expression: `(async () => { const s = await window.__TAURI__.core.invoke("backend_status"); return JSON.stringify(s); })()`,
    returnByValue: true,
    awaitPromise: true,
  },
}));
