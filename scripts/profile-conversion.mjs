// Profile an already running Tauri app through its WebView2 DevTools port.
// node scripts/profile-conversion.mjs <port> <input.zip> <target> <output.zip> <report.json>
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function timingFromLines(lines) {
  const timings = {};
  for (const line of lines) {
    const perf = line.match(/\bPERF ([a-z0-9_]+)=([0-9]+)/);
    if (perf) timings[perf[1]] = Number(perf[2]);
    const phase = line.match(/\bNWC_PHASE (init|convert|terraform) ([0-9]+)/);
    if (phase) timings[`b2j.${phase[1]}_ms`] = Number(phase[2]);
  }
  return timings;
}

async function main() {
  const [port, inputPath, target, outputPath, reportPath] = process.argv.slice(2);
  if (!port || !inputPath || !target || !outputPath || !reportPath) {
    throw new Error('usage: node scripts/profile-conversion.mjs <port> <input.zip> <target> <output.zip> <report.json>');
  }
  const inputHash = createHash('sha256').update(readFileSync(inputPath)).digest('hex');
  const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const page = pages.find((entry) => entry.type === 'page');
  if (!page) throw new Error('DevTools page not found');
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((done, reject) => {
    ws.onopen = done;
    ws.onerror = reject;
  });
  let nextId = 0;
  const pending = new Map();
  ws.onmessage = ({ data }) => {
    const response = JSON.parse(data);
    const callback = pending.get(response.id);
    if (callback) {
      pending.delete(response.id);
      callback(response);
    }
  };
  function send(method, params) {
    return new Promise((done, reject) => {
      const id = ++nextId;
      const timeout = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`${method} timed out`));
      }, 40 * 60 * 1000);
      pending.set(id, (response) => {
        clearTimeout(timeout);
        if (response.error) reject(new Error(JSON.stringify(response.error)));
        else done(response);
      });
      ws.send(JSON.stringify({ id, method, params }));
    });
  }
  async function evaluate(expression) {
    const response = await send('Runtime.evaluate', {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (response.result?.exceptionDetails) {
      throw new Error(JSON.stringify(response.result.exceptionDetails));
    }
    return response.result?.result?.value;
  }
  const arg = JSON.stringify;
  try {
    await evaluate(`(async () => {
      window.__conversionProfile = { start: performance.now(), stages: [], timingLines: [] };
      await window.__TAURI__.event.listen('nwc://progress', ({ payload }) => {
        const stages = window.__conversionProfile.stages;
        if (stages.at(-1)?.stage !== payload.stage) {
          stages.push({ ms: performance.now() - window.__conversionProfile.start, stage: payload.stage });
        }
      });
      await window.__TAURI__.event.listen('nwc://log', ({ payload }) => {
        if (payload.line.includes('PERF ') || payload.line.includes('NWC_PHASE ')) {
          window.__conversionProfile.timingLines.push(payload.line);
        }
      });
      return true;
    })()`);
    const analysis = await evaluate(`(async () => {
      const start = performance.now();
      const value = await window.__TAURI__.core.invoke('analyze', { path: ${arg(inputPath)} });
      return { ms: performance.now() - start, value };
    })()`);
    if (!analysis.value.supported) throw new Error('input archive is not supported');
    if (!analysis.value.targets.some((item) => item.displayName === target)) {
      throw new Error(`target unavailable: ${target}`);
    }
    const conversion = await evaluate(`(async () => {
      const start = performance.now();
      const value = await window.__TAURI__.core.invoke('convert', {
        sessionId: ${arg(analysis.value.sessionId)}, target: ${arg(target)}
      });
      return { ms: performance.now() - start, value };
    })()`);
    const save = await evaluate(`(async () => {
      const start = performance.now();
      await window.__TAURI__.core.invoke('save_result', {
        sessionId: ${arg(analysis.value.sessionId)}, destination: ${arg(outputPath)}
      });
      return { ms: performance.now() - start };
    })()`);
    const events = await evaluate('window.__conversionProfile');
    if (!existsSync(outputPath)) throw new Error('output ZIP was not created');
    const sourceUnchanged = createHash('sha256').update(readFileSync(inputPath)).digest('hex') === inputHash;
    if (!sourceUnchanged) throw new Error('source ZIP changed during conversion');
    const report = {
      inputPath,
      inputSha256: inputHash,
      target,
      sourceUnchanged,
      outputPath,
      outputBytes: statSync(outputPath).size,
      outputSha256: createHash('sha256').update(readFileSync(outputPath)).digest('hex'),
      analysisMs: analysis.ms,
      conversionMs: conversion.ms,
      saveMs: save.ms,
      worldType: analysis.value.typeName,
      worldBytes: analysis.value.byteCount,
      regionFiles: conversion.value.regionFiles,
      regionChunks: conversion.value.regionChunks,
      timingsMs: timingFromLines(events.timingLines),
      stages: events.stages,
    };
    writeFileSync(reportPath, JSON.stringify(report, null, 2));
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } finally {
    await evaluate("window.__TAURI__.core.invoke('shutdown_cleanup')").catch(() => {});
    ws.close();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
