// 网易 Minecraft 存档转换器 — Tauri 前端
// 与原 Swing 版行为一致：选 ZIP → 识别 → 选目标版本 →（降级需确认）转换 → 保存。

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;
const { getCurrentWebview } = window.__TAURI__.webview;

const $ = (id) => document.getElementById(id);
const els = {
  input: $("input-field"),
  choose: $("choose-btn"),
  detection: $("detection-label"),
  details: $("details-label"),
  target: $("target-box"),
  warning: $("warning-label"),
  stage: $("stage-label"),
  bar: $("progress-bar"),
  progressText: $("progress-text"),
  progressTrack: document.querySelector(".progress-track"),
  resourceMetrics: $("resource-metrics"),
  metricElapsed: $("metric-elapsed"),
  metricCpu: $("metric-cpu"),
  metricMemory: $("metric-memory"),
  log: $("log-area"),
  start: $("start-btn"),
  save: $("save-btn"),
  report: $("report-btn"),
  backendNote: $("backend-note"),
  dropOverlay: $("drop-overlay"),
  backdrop: $("modal-backdrop"),
  modalTitle: $("modal-title"),
  modalBody: $("modal-body"),
  modalOk: $("modal-ok"),
  modalCancel: $("modal-cancel"),
};

let session = null;      // { sessionId, type, supported, targets, ... }
let busy = false;        // 分析或转换进行中（两者都独占 UI 与后端流水线）
let converting = false;  // 仅转换进行中
let canCancel = false;   // 后台首次进度表示转换已取得独占权
let cancelRequested = false;
let errorReportPath = null;
let modalResolve = null;
let logBuffer = [];
let telemetryTimer = null;
let telemetryStartedAt = 0;
let phase = "idle";
let savedPath = null;
let ready = false;
let modalFocus = null;
const MAX_LOG_LINES = 1500;

// ---------- 日志与进度 ----------

function appendLog(line) {
  if (!line) return;
  const follow = els.log.scrollHeight - els.log.scrollTop - els.log.clientHeight < 40;
  logBuffer.push(line);
  if (logBuffer.length > MAX_LOG_LINES) {
    logBuffer.splice(0, logBuffer.length - MAX_LOG_LINES);
  }
  els.log.textContent = logBuffer.join("\n") + "\n";
  if (follow) els.log.scrollTop = els.log.scrollHeight;
}

// 后端错误统一为 {"code":"...","message":"..."} JSON 字符串；纯文本旧格式兜底
function parseError(err) {
  if (typeof err === "string") {
    try {
      const parsed = JSON.parse(err);
      if (parsed && typeof parsed === "object" && typeof parsed.code === "string") {
        return { code: parsed.code, message: parsed.message ?? err };
      }
    } catch (e) { /* 纯文本错误 */ }
    return { code: "error", message: err };
  }
  return {
    code: (err && err.code) || "error",
    message: (err && err.message) || String(err),
  };
}

function setProgress(percent) {
  const p = Math.max(0, Math.min(100, percent));
  els.bar.style.width = p + "%";
  els.progressTrack.setAttribute("aria-valuenow", p);
  els.progressText.textContent = p + "%";
}

function setStage(text, cls) {
  els.stage.textContent = text;
  els.stage.className = "stage" + (cls ? " " + cls : "");
}

// ---------- 模态框 ----------

function showModal(title, body, okText, cancelText) {
  if (modalResolve) return Promise.resolve(false);
  modalFocus = document.activeElement;
  els.modalTitle.textContent = title;
  els.modalBody.textContent = body;
  els.modalOk.textContent = okText || "确定";
  els.modalCancel.textContent = cancelText || "取消";
  els.modalCancel.classList.toggle("hidden", !cancelText);
  els.backdrop.showModal();
  (cancelText ? els.modalCancel : els.modalOk).focus();
  return new Promise((resolve) => { modalResolve = resolve; });
}

function closeModal(confirmed) {
  els.backdrop.close();
  const resolve = modalResolve;
  modalResolve = null;
  modalFocus?.focus();
  resolve?.(confirmed);
}
els.modalOk.addEventListener("click", () => closeModal(true));
els.modalCancel.addEventListener("click", () => closeModal(false));
els.backdrop.addEventListener("cancel", (event) => {
  event.preventDefault();
  closeModal(false);
});

// ---------- 状态刷新 ----------

function resetUiForAnalyze() {
  setBusy(false);
  resetTelemetry();
  session = null;
  errorReportPath = null;
  cancelRequested = false;
  savedPath = null;
  phase = "analyzing";
  $("result-card").classList.add("hidden");
  logBuffer = [];
  els.target.innerHTML = "";
  els.target.value = "";
  els.target.disabled = true;
  els.save.disabled = true;
  els.report.disabled = true;
  els.start.disabled = true;
  els.detection.textContent = "正在安全解压并识别……";
  els.detection.className = "detection";
  els.details.textContent = "";
  els.log.textContent = "";
  refreshDowngradeWarning();
  setProgress(0);
  setStage("读取 ZIP · 计算 SHA-256");
}

function setBusy(isBusy) {
  busy = isBusy;
  els.choose.disabled = isBusy || !ready;
  els.input.disabled = isBusy;
  els.target.disabled = isBusy || !session?.supported || !session.targets.length;
  els.start.disabled = converting ? cancelRequested || !canCancel : isBusy || !session?.supported || !els.target.value;
  els.start.textContent = converting ? (cancelRequested ? "正在取消…" : canCancel ? "取消转换" : "正在准备…") : session?.result ? "重新转换" : "开始转换";
  els.start.classList.toggle("plain", converting || !!session?.result);
  els.start.classList.toggle("primary", !converting && !session?.result);
  els.save.disabled = isBusy || !session?.result;
  els.report.disabled = isBusy || !errorReportPath;
  els.choose.textContent = session ? "更换存档" : "选择存档";
  els.save.textContent = savedPath ? "另存一份 ZIP" : "保存 ZIP";
  $("workflow").dataset.step = session?.result ? "3" : session?.supported ? "2" : "1";
  document.querySelectorAll(".flow-step").forEach((step, index) => {
    if (index + 1 === Number($("workflow").dataset.step)) step.setAttribute("aria-current", "step");
    else step.removeAttribute("aria-current");
  });
  $("flow-hint").textContent = isBusy ? "正在处理，请稍候" : session?.result ?
    (savedPath ? "已保存，可以导入 Minecraft 了" : "转换完成，保存后即可使用") : session?.supported ?
    "选择你准备游玩的 Java 版本" : "从一个世界，开始新的旅程";
  els.dropOverlay.classList.add("hidden");
  if (isBusy && (phase === "analyzing" || converting)) {
    if (!telemetryTimer) startTelemetry();
  } else if (telemetryTimer) stopTelemetry();
}

function resetTelemetry() {
  stopTelemetry();
  telemetryStartedAt = 0;
  els.metricElapsed.textContent = "等待任务";
  els.metricCpu.textContent = "CPU --";
  els.metricMemory.textContent = "内存 --";
}

function startTelemetry() {
  telemetryStartedAt = Date.now();
  els.resourceMetrics.classList.add("running");
  els.bar.classList.add("active");
  refreshTelemetry();
  telemetryTimer = setInterval(refreshTelemetry, 1000);
}

function stopTelemetry() {
  if (telemetryTimer) clearInterval(telemetryTimer);
  telemetryTimer = null;
  els.resourceMetrics.classList.remove("running");
  els.bar.classList.remove("active");
  if (telemetryStartedAt) {
    els.metricElapsed.textContent = "用时 " + formatElapsed(Date.now() - telemetryStartedAt);
  }
}

async function refreshTelemetry() {
  const startedAt = telemetryStartedAt;
  els.metricElapsed.textContent = "已运行 " + formatElapsed(Date.now() - startedAt);
  try {
    const usage = await invoke("resource_usage");
    if (!busy || telemetryStartedAt !== startedAt) return;
    els.metricCpu.textContent = `CPU ${Math.round(usage.cpuPercent)}%`;
    els.metricMemory.textContent = "内存 " + formatBytes(usage.memoryBytes);
  } catch (e) { /* 资源统计失败不影响转换 */ }
}

function formatElapsed(milliseconds) {
  const seconds = Math.floor(milliseconds / 1000);
  return String(Math.floor(seconds / 60)).padStart(2, "0") + ":" + String(seconds % 60).padStart(2, "0");
}

function applyProgress(p) {
  if (acceptEvent(p) && Number.isFinite(p.percent)) {
    if (converting && !canCancel) { canCancel = true; setBusy(true); }
    setProgress(p.percent);
    setStage(p.stage + (p.detail ? " · " + p.detail : ""));
  }
}

function acceptEvent(payload) {
  return (phase === "analyzing" && busy) || (converting && payload.sessionId === session?.sessionId);
}

function detectionOk(text) { els.detection.textContent = text; els.detection.className = "detection ok"; }
function detectionFail(text) { els.detection.textContent = text; els.detection.className = "detection error"; }

async function refreshDowngradeWarning() {
  const current = session;
  const target = els.target.value;
  els.warning.className = "warning";
  els.warning.textContent = "识别存档后，将显示可用的目标版本。";
  if (!current?.supported || !target) return;
  try {
    const downgrade = await invoke("is_downgrade", { sessionId: current.sessionId, target });
    if (session !== current || els.target.value !== target) return;
    els.warning.className = downgrade ? "warning severe" : "warning neutral";
    els.warning.textContent = downgrade ? "降级转换：新方块、物品和实体可能无法保留。开始前会再次确认。" :
      "输出为 Java 存档 ZIP；原始存档保留，可放心另存。";
  } catch (err) {
    if (session === current && els.target.value === target) els.warning.textContent = "版本检查失败：" + parseError(err).message;
  }
}

function confirmDiscard() {
  return !session?.result || savedPath ? Promise.resolve(true) : showModal(
    "还有未保存的转换结果", "继续后将离开当前结果。建议先保存 ZIP，以免需要重新转换。", "继续", "返回保存");
}

// ---------- 分析 ----------

async function analyze(path) {
  if (busy || modalResolve || !ready) return;
  setBusy(true);
  if (!await confirmDiscard()) { setBusy(false); return; }
  resetUiForAnalyze();
  els.input.value = path;
  setBusy(true);
  try {
    const result = await invoke("analyze", { path });
    session = result;
    errorReportPath = result.errorReport || null;
    if (result.supported) {
      detectionOk("✓ " + result.typeName + " · " + result.detectedVersion);
      setProgress(12);
      setStage("识别完成 · 选择目标版本后开始转换", "ok");
      for (const t of result.targets) {
        const opt = document.createElement("option");
        opt.value = t.displayName;
        opt.textContent = t.displayName;
        els.target.append(opt);
      }
      els.target.disabled = false;
      els.target.selectedIndex = 0;
      els.start.disabled = false;
      phase = "ready";
    } else {
      detectionFail("✕ " + result.typeName + " · " + result.detectedVersion);
      phase = "unsupported";
      setStage("旧版 AES 加密无法离线转换" + (errorReportPath ? " · 可打开诊断报告查看详情" : ""), "error");
    }
    els.details.textContent =
      `世界：${result.worldName}　文件：${result.fileCount}　大小：${formatBytes(result.byteCount)}`;
    for (const note of result.notes) appendLog("[INFO] " + note);
    if (errorReportPath) {
      els.report.disabled = false;
      appendLog("[INFO] 错误报告：" + errorReportPath);
    }
    refreshDowngradeWarning();
  } catch (err) {
    phase = "error";
    await handleAnalysisFailure(path, err);
  } finally {
    setBusy(false);
  }
}

async function handleAnalysisFailure(path, err) {
  const parsed = parseError(err);
  detectionFail("✕ 无法解析存档");
  setStage(parsed.message, "error");
  setProgress(0);
  els.progressText.textContent = "解析失败";
  appendLog("ERROR: " + parsed.message);
  try {
    const report = await invoke("export_analysis_error", { path, message: parsed.message });
    if (report) {
      errorReportPath = report;
      els.report.disabled = false;
      appendLog("错误报告：" + report);
    }
  } catch (e) { /* 报告导出失败则忽略 */ }
  await showModal("解析失败", "存档解析失败。\n" + parsed.message + (errorReportPath ? "\n\n错误报告：\n" + errorReportPath : ""), "确定");
}

// ---------- 转换 ----------

els.start.addEventListener("click", async () => {
  if (converting) {
    if (cancelRequested || !canCancel) return;
    cancelRequested = true;
    setBusy(true);
    setStage("正在取消 · 等待后台任务结束", "warn");
    try { await invoke("cancel", { sessionId: session.sessionId }); }
    catch (err) {
      if (converting) {
        cancelRequested = false;
        setBusy(true);
        setStage("取消失败，可重试：" + parseError(err).message, "error");
      }
    }
    return;
  }
  if (busy || modalResolve || !session?.supported || !els.target.value) return;
  setBusy(true);
  const target = els.target.value;
  try {
  if (!await confirmDiscard()) return;
  const downgrade = await invoke("is_downgrade", { sessionId: session.sessionId, target });
  if (downgrade) {
    const ok = await showModal(
      "确认降级转换",
      `这是降级转换（${session.sourceVersion} → ${target}）。\n\n` +
      "旧版本无法表示后来加入的方块、物品、生物和数据组件；" +
      "无法映射的源实体、POI 与玩家文件会保存在输出 ZIP 的 _NWC_preserved_source 中。\n\n仍要继续吗？",
      "继续", "取消"
    );
    if (!ok) return;
  }
  await startConversion(target);
  } catch (err) {
    await showModal("无法开始转换", parseError(err).message, "确定");
  } finally { setBusy(false); }
});

async function startConversion(target) {
  if (converting || !session?.supported) return;
  converting = true;
  canCancel = false;
  phase = "converting";
  cancelRequested = false;
  errorReportPath = null;
  session.result = null;
  savedPath = null;
  $("result-card").classList.add("hidden");
  setBusy(true); // 转换期间禁用选择/拖放，避免两条流水线的进度与日志交叉污染
  els.save.disabled = true;
  els.report.disabled = true;
  els.target.disabled = true;
  setProgress(13);
  setStage("准备转换到 " + target);
  els.detection.className = "detection idle";
  try {
    const result = await invoke("convert", { sessionId: session.sessionId, target });
    phase = "complete";
    setProgress(100);
    els.progressText.textContent = "转换成功";
    setStage("转换完成 · 请保存 ZIP，完成最后一步", "ok");
    session.result = result;
    $("result-card").classList.remove("hidden");
    $("result-title").textContent = "你的世界，已准备就绪";
    $("result-details").textContent = `${result.targetVersion} · ${result.regionFiles} 个区域 · ${result.regionChunks} 条区域记录`;
    $("result-note").textContent = result.regionNote || "输出已通过区域结构验证";
  } catch (err) {
    const parsed = parseError(err);
    converting = false;
    setBusy(true);
    const cancelled = parsed.code === "cancelled";
    phase = cancelled ? "cancelled" : "error";
    if (cancelled) {
      setStage("已取消 · 原始存档未修改，可以重新开始", "warn");
      els.progressText.textContent = "已取消";
    } else {
      setStage(parsed.message, "error");
      els.progressText.textContent = "转换失败";
      try {
        const report = await invoke("export_conversion_error", { sessionId: session.sessionId, message: parsed.message });
        if (report) { errorReportPath = report; els.report.disabled = false; }
      } catch (e) { /* ignore */ }
      await showModal("转换失败", "转换失败。原始 ZIP 没有被修改。\n\n" + parsed.message +
        (errorReportPath ? "\n\n错误报告：\n" + errorReportPath : ""), "确定");
    }
  } finally {
    converting = false;
    cancelRequested = false;
    setBusy(false);
    if (session.result) els.save.focus();
  }
}

// ---------- 保存 / 报告 ----------

els.save.addEventListener("click", async () => {
  if (busy || modalResolve || !session?.result) return;
  setBusy(true);
  try {
    const dest = await invoke("pick_save_path", { defaultName: session.result.fileName });
    if (!dest) return;
    const saved = await invoke("save_result", { sessionId: session.sessionId, destination: dest });
    savedPath = saved;
    setStage("已保存：" + saved, "ok");
    $("result-title").textContent = "已保存，去探索你的世界吧";
    $("result-note").textContent = "保存位置：" + saved;
  } catch (err) {
    await showModal("保存失败", "保存失败：" + parseError(err).message, "确定");
  } finally { setBusy(false); }
});

els.report.addEventListener("click", async () => {
  if (busy || modalResolve || !errorReportPath) return;
  try {
    await invoke("open_path", { path: errorReportPath });
  } catch (err) {
    await showModal("打开失败", "无法打开错误报告：\n" + parseError(err).message, "确定");
  }
});

// ---------- 选择文件 ----------

els.choose.addEventListener("click", async () => {
  if (busy || modalResolve || !ready) return;
  setBusy(true);
  let path;
  try { path = await invoke("pick_input_path"); }
  catch (err) { await showModal("无法选择存档", parseError(err).message, "确定"); }
  finally { setBusy(false); }
  if (path) await analyze(path);
});

// ---------- 拖放 ----------

if (getCurrentWebview) {
  getCurrentWebview().onDragDropEvent((event) => {
    const type = event.payload?.type;
    if (busy || modalResolve || !ready) { els.dropOverlay.classList.add("hidden"); return; }
    if (type === "enter" || type === "over") {
      els.dropOverlay.classList.remove("hidden");
    } else if (type === "leave") {
      els.dropOverlay.classList.add("hidden");
    } else if (type === "drop") {
      els.dropOverlay.classList.add("hidden");
      const paths = event.payload.paths || [];
      if (paths.length > 1) {
        showModal("一次处理一个世界", "请只拖入一个 .zip 或 .mcworld 存档文件。", "确定");
      } else if (paths.length) {
        const lower = paths[0].toLowerCase();
        if (lower.endsWith(".zip") || lower.endsWith(".mcworld")) {
          analyze(paths[0]);
        } else {
          showModal("拖放失败", "请拖入 .zip 或 .mcworld 存档文件。", "确定");
        }
      }
    }
  });
}

// ---------- 关闭确认 ----------

if (getCurrentWindow) {
  getCurrentWindow().onCloseRequested(async (event) => {
    event.preventDefault();
    if (modalResolve) return;
    // 保存过程中不可销毁临时结果；分析尚未返回会话，也需等待其结束。
    if (busy && !converting) {
      await showModal("请稍候", "当前操作完成后即可退出。", "确定");
      return;
    }
    if (converting && !await showModal("确认退出", "转换仍在进行，退出将取消当前任务。", "退出", "返回")) return;
    // 转换可能在退出确认期间完成，需要再次保护尚未保存的结果。
    if (!await confirmDiscard()) return;
    try {
      await invoke("shutdown_cleanup");
      await getCurrentWindow().destroy();
    } catch (err) { await showModal("无法退出", parseError(err).message, "确定"); }
  });
}

// ---------- 事件与初始化 ----------

(async () => {
  setBusy(false);
  try {
  await listen("nwc://progress", (event) => applyProgress(event.payload));
  await listen("nwc://log", (event) => { if (acceptEvent(event.payload)) appendLog(event.payload.line); });
  ready = true;
  setBusy(false);
  } catch (err) {
    setStage("初始化失败，请重启应用：" + parseError(err).message, "error");
    return;
  }
  els.target.addEventListener("change", refreshDowngradeWarning);
  try {
    const status = await invoke("backend_status");
    if (status.ok) {
      els.backendNote.title = `Java: ${status.java}\nChunker: ${status.chunker}\nJE2BE: ${status.b2j}`;
    } else {
      els.backendNote.textContent = "后端缺失：" + status.message;
      els.backendNote.style.color = "var(--error)";
      els.backendNote.title = status.message;
    }
  } catch (e) { /* 忽略 */ }
})();

function formatBytes(bytes) {
  let value = bytes;
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
  return value.toFixed(2) + " " + units[unit];
}
