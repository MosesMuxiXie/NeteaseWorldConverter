// devtools-probe.mjs — 通过 WebView2 远程调试协议读取页面 DOM 文本。
const port = process.argv[2] || "9223";
const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = list.find((p) => p.type === "page");
console.log("page:", page.title, page.url);
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
function send(method, params) {
  return new Promise((res) => {
    const msgId = ++id;
    const onMsg = (ev) => {
      const data = JSON.parse(ev.data);
      if (data.id === msgId) { ws.removeEventListener("message", onMsg); res(data.result); }
    };
    ws.addEventListener("message", onMsg);
    ws.send(JSON.stringify({ id: msgId, method, params }));
  });
}
const evalRes = await send("Runtime.evaluate", {
  expression: `JSON.stringify({ title: document.title, text: document.body ? document.body.innerText.slice(0, 2000) : null, html: document.documentElement ? document.documentElement.outerHTML.slice(0, 1500) : null, ta: typeof window.__TAURI__ })`,
  returnByValue: true,
});
console.log(JSON.stringify(JSON.parse(evalRes.result.value), null, 2).slice(0, 4000));
ws.close();
