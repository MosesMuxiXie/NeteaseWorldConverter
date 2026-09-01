// 网易 Minecraft 存档转换器 — Tauri 前端
// 与原 Swing 版行为一致：选 ZIP → 识别 → 选目标版本 →（降级需确认）转换 → 保存。

const { invoke, listen } = window.__TAURI__.core;
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
let cancelRequested = false;
let errorReportPath = null;
let modalResolve = null;
let logBuffer = [];
const MAX_LOG_LINES = 1500;

// ---------- 日志与进度 ----------

function appendLog(line) {
  if (!line) return;
  logBuffer.push(line);
  if (logBuffer.length > MAX_LOG_LINES) {
    logBuffer.splice(0, logBuffer.length - MAX_LOG_LINES);
  }
  els.log.textContent = logBuffer.join("\n") + "\n";
  els.log.scrollTop = els.log.scrollHeight;
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
    message: (err && (err.message ?? err.message)) || String(err),
  };
}

function setProgress(percent) {
  const p = Math.max(0, Math.min(100, percent));
  els.bar.style.width = p + "%";
  els.progressText.textContent = p + "%";
}

function setStage(text, cls) {
  els.stage.textContent = text;
  els.stage.className = "stage" + (cls ? " " + cls : "");
}

// ---------- 模态框 ----------

function showModal(title, body, okText, cancelText) {
  els.modalTitle.textContent = title;
  els.modalBody.textContent = body;
  els.modalOk.textContent = okText || "确定";
  els.modalCancel.textContent = cancelText || "取消";
  els.modalCancel.classList.toggle("hidden", !cancelText);
  els.backdrop.classList.remove("hidden");
  return new Promise((resolve) => { modalResolve = resolve; });
}

els.modalOk.addEventListener("click", () => {
  els.backdrop.classList.add("hidden");
  if (modalResolve) { modalResolve(true); modalResolve = null; }
});
els.modalCancel.addEventListener("click", () => {
  els.backdrop.classList.add("hidden");
  if (modalResolve) { modalResolve(false); modalResolve = null; }
});

// ---------- 状态刷新 ----------

function resetUiForAnalyze() {
  setBusy(false);
  session = null;
  errorReportPath = null;
  cancelRequested = false;
  logBuffer = [];
  els.target.innerHTML = "";
  els.target.disabled = true;
  els.save.disabled = true;
  els.report.disabled = true;
  els.start.disabled = true;
  els.detection.textContent = "正在安全解压并识别……";
  els.detection.className = "detection";
  els.details.textContent = "";
  els.log.textContent = "";
  setProgress(0);
  setStage("读取 ZIP · 计算 SHA-256");
}

function setBusy(isBusy) {
  busy = isBusy;
  els.choose.disabled = isBusy;
  els.input.disabled = isBusy;
}

function applyProgress(p) {
  if (typeof p.percent === "number") {
    setProgress(p.percent);
    setStage(p.stage + (p.detail ? " · " + p.detail : ""));
  }
}

function detectionOk(text) { els.detection.textContent = text; els.detection.className = "detection ok"; }
function detectionFail(text) { els.detection.textContent = text; els.detection.className = "detection error"; }

function refreshDowngradeWarning() {
  const v = els.target.value || "";
  if (/^1\.1[2-6](\D|$)/.test(v)) {
    els.warning.textContent = `⚠ 大跨度降级：新版本内容无法在 ${v} 中表示；请务必保留原 ZIP。`;
    els.warning.className = "warning severe";
  } else if (v) {
    els.warning.textContent = "降级时，目标版本不存在的新方块、物品和实体无法无损表达；原 ZIP 始终保留。";
    els.warning.className = "warning";
  } else {
    els.warning.textContent = "降级到旧版本时，目标版本不存在的新方块、物品和实体无法无损表达。";
    els.warning.className = "warning";
  }
}

// ---------- 分析 ----------

async function analyze(path) {
  if (busy) return;
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
      setStage("解析成功");
      for (const t of result.targets) {
        const opt = document.createElement("option");
        opt.value = t.displayName;
        opt.textContent = t.displayName;
        els.target.append(opt);
      }
      els.target.disabled = false;
      els.target.selectedIndex = 0;
      els.start.disabled = false;
    } else {
      detectionFail("✕ " + result.typeName + " · " + result.detectedVersion);
      setStage("已识别，但该旧加密格式需要外部账号密钥；诊断报告已导出", "error");
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
    cancelRequested = true;
    await invoke("cancel", { sessionId: session.sessionId });
    return;
  }
  if (!session || !session.supported || !els.target.value) return;
  const target = els.target.value;
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
});

async function startConversion(target) {
  converting = true;
  cancelRequested = false;
  errorReportPath = null;
  setBusy(true); // 转换期间禁用选择/拖放，避免两条流水线的进度与日志交叉污染
  els.save.disabled = true;
  els.report.disabled = true;
  els.target.disabled = true;
  els.start.textContent = "取消转换";
  els.start.disabled = false;
  setProgress(13);
  setStage("准备转换到 Java " + target);
  els.detection.classList.add("idle");
  try {
    const result = await invoke("convert", { sessionId: session.sessionId, target });
    converting = false;
    setBusy(false);
    els.start.textContent = "开始转换";
    els.target.disabled = false;
    setProgress(100);
    els.progressText.textContent = "转换成功";
    setStage("✓ 输出已完成并通过逐区域结构验证", "ok");
    session.result = result;
    els.save.disabled = false;
    await showModal("转换完成",
      `转换成功！\n\n目标：${result.targetVersion}\n区域文件：${result.regionFiles}\n区域记录：${result.regionChunks}` +
      `\n\n点击"下载 / 保存 ZIP"选择保存位置。`, "确定");
  } catch (err) {
    converting = false;
    setBusy(false);
    els.start.textContent = "开始转换";
    els.target.disabled = false;
    const parsed = parseError(err);
    const cancelled = cancelRequested || parsed.code === "cancelled";
    if (cancelled) {
      setStage("转换已取消；原始 ZIP 未修改", "warn");
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
  }
}

// ---------- 保存 / 报告 ----------

els.save.addEventListener("click", async () => {
  if (!session || !session.result) return;
  const dest = await invoke("pick_save_path", { defaultName: session.result.fileName });
  if (!dest) return;
  try {
    const saved = await invoke("save_result", { sessionId: session.sessionId, destination: dest });
    setStage("已保存：" + saved, "ok");
    await showModal("保存成功", "已保存：\n" + saved, "确定");
  } catch (err) {
    await showModal("保存失败", "保存失败：" + parseError(err).message, "确定");
  }
});

els.report.addEventListener("click", async () => {
  if (errorReportPath) await invoke("open_path", { path: errorReportPath });
});

// ---------- 选择文件 ----------

els.choose.addEventListener("click", async () => {
  const path = await invoke("pick_input_path");
  if (path) analyze(path);
});

// ---------- 拖放 ----------

if (getCurrentWebview) {
  getCurrentWebview().onDragDropEvent((event) => {
    const type = event.payload?.type;
    if (type === "enter" || type === "over") {
      els.dropOverlay.classList.remove("hidden");
    } else if (type === "leave") {
      els.dropOverlay.classList.add("hidden");
    } else if (type === "drop") {
      els.dropOverlay.classList.add("hidden");
      const paths = event.payload.paths || [];
      if (paths.length && !busy) {
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
    if (busy || converting) {
      event.preventDefault();
      const ok = await showModal("确认退出", "转换仍在进行。确定取消并退出吗？", "退出", "返回");
      if (ok) {
        cancelRequested = true;
        if (session) { try { await invoke("cancel", { sessionId: session.sessionId }); } catch (e) {} }
        await invoke("shutdown_cleanup");
        getCurrentWindow().destroy();
      }
    } else {
      await invoke("shutdown_cleanup");
    }
  });
}

// ---------- 事件与初始化 ----------

(async () => {
  await listen("nwc://progress", (event) => applyProgress(event.payload));
  await listen("nwc://log", (event) => appendLog(event.payload.line));
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
