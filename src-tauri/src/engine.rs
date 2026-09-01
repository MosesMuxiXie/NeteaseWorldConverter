// engine.rs — 对应 ConversionEngine.java：
// analyze → targets → convert → validate → zip 的编排核心与会话管理。

use crate::archive::{
    copy_tree, create_zip, delete_tree, extract_zip, file_name, safe_folder_name, sha256,
    strip_extension,
};
use crate::backends;
use crate::decrypt;
use crate::detect;
use crate::entity;
use crate::error::{conv, ConversionError, Result};
use crate::log::AppLog;
use crate::models::{
    AnalysisDto, ConversionResultDto, LogPayload, TargetVersion, WorldInfo, WorldType,
};
use crate::sink::Sink;
use crate::validate;
use crate::version::{chunker_format, is_downgrade_opt, parse_version, Version};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter};
use tempfile::TempDir;

pub struct Session {
    pub session_id: String,
    pub input_zip: PathBuf,
    pub temp_dir: TempDir,
    pub extracted: PathBuf,
    pub world: WorldInfo,
    pub targets: Vec<TargetVersion>,
    pub cancel: Arc<AtomicBool>,
    pub log: Arc<AppLog>,
    pub result: Mutex<Option<StoredResult>>,
}

#[derive(Clone)]
pub struct StoredResult {
    pub result_zip: PathBuf,
    pub file_name: String,
    pub target_version: String,
    pub region_files: i64,
    pub region_chunks: i64,
    pub region_note: String,
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<Session>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<Session>>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn new_session_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S%3f");
    format!("{stamp}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn find_session(session_id: &str) -> Result<Arc<Session>> {
    sessions()
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .ok_or_else(|| ConversionError(format!("转换会话不存在：{session_id}")))
}

fn new_sink(app: &AppHandle, session_id: &str, cancel: &Arc<AtomicBool>, log: &Arc<AppLog>) -> Sink {
    let emit_app = app.clone();
    let emit_session = session_id.to_string();
    log.set_listener(Some(Box::new(move |line: &str| {
        let _ = emit_app.emit(
            "nwc://log",
            LogPayload {
                session_id: emit_session.clone(),
                line: line.to_string(),
            },
        );
    })));
    let progress_app = app.clone();
    Sink::new(
        session_id.to_string(),
        cancel.clone(),
        log.clone(),
        move |payload| {
            let _ = progress_app.emit("nwc://progress", payload);
        },
    )
}

// ---------- 分析 ----------

pub fn analyze(app: &AppHandle, input: &Path) -> Result<AnalysisDto> {
    let input = input.to_path_buf();
    let name = file_name(&input);
    let lower = name.to_lowercase();
    if !lower.ends_with(".zip") && !lower.ends_with(".mcworld") {
        return conv("仅支持 .zip 或 .mcworld 存档文件");
    }
    if !input.is_file() {
        return conv("文件不存在");
    }

    let session_id = new_session_id();
    let cancel = Arc::new(AtomicBool::new(false));
    let temp = tempfile::Builder::new()
        .prefix("NeteaseWorldConverter-")
        .tempdir()?;
    let temp_path = temp.path().to_path_buf();
    let log = Arc::new(AppLog::new(&temp_path.join("conversion.log"))?);
    let sink = new_sink(app, &session_id, &cancel, &log);

    sink.update(1, "读取 ZIP", "计算 SHA-256");
    log.info(&format!("输入文件：{}", input.display()));
    let hash = sha256(&input)?;
    log.info(&format!("SHA-256：{hash}"));

    let extracted = temp_path.join("extracted");
    extract_zip(&input, &extracted, &sink)?;

    sink.update(12, "识别存档", "正在分析目录结构");
    let world = detect::detect(&extracted, &log)?;
    sink.update(12, "解析成功", &world.detected_version);

    let targets = backends::list_target_versions(app);
    log.info(&format!("可用目标版本：{} 个", targets.len()));

    let supported = match world.world_type {
        WorldType::NeteaseBedrockLegacyAes => false,
        WorldType::Java if targets.is_empty() => false,
        _ => true,
    };

    let error_report = if !supported && world.world_type == WorldType::NeteaseBedrockLegacyAes {
        log.warn("旧版 AES-CFB8 加密无法离线恢复密钥，导出诊断报告");
        match export_error_report(log.file(), &input, "检测到旧版 AES-CFB8 加密，需要外部账号密钥") {
            Ok(path) => Some(path),
            Err(error) => {
                log.error("错误报告导出失败", &error);
                None
            }
        }
    } else {
        None
    };

    let session = Arc::new(Session {
        session_id: session_id.clone(),
        input_zip: input.clone(),
        temp_dir: temp,
        extracted,
        world: world.clone(),
        targets: targets.clone(),
        cancel,
        log,
        result: Mutex::new(None),
    });
    sessions()
        .lock()
        .unwrap()
        .insert(session_id.clone(), session);

    Ok(AnalysisDto {
        session_id,
        type_name: world.world_type.display_name().to_string(),
        detected_version: world.detected_version.clone(),
        world_name: world.world_name.clone(),
        file_count: world.file_count,
        byte_count: world.byte_count,
        notes: world.notes,
        supported,
        targets,
        source_version: world.detected_version,
        error_report,
    })
}

// ---------- 转换 ----------

pub fn convert(app: &AppHandle, session_id: &str, target: &str) -> Result<ConversionResultDto> {
    let session = find_session(session_id)?;
    if session.cancel.load(Ordering::Relaxed) {
        return conv("操作已取消");
    }
    if session.world.world_type == WorldType::NeteaseBedrockLegacyAes {
        return conv("该存档使用旧版 AES-CFB8 加密，密钥不在存档中，无法离线转换");
    }
    let target_version = target.to_string();
    let sink = new_sink(app, &session.session_id, &session.cancel, &session.log);
    sink.update(13, "准备转换", &format!("目标：{target_version}"));

    let chunker_format = session
        .targets
        .iter()
        .find(|t| t.display_name == target_version)
        .map(|t| t.chunker_format.clone())
        .or_else(|| {
            parse_version(&target_version).map(chunker_format)
        })
        .ok_or_else(|| ConversionError(format!("未知的目标版本：{target_version}")))?;

    let work = session.temp_dir.path().join("work");
    delete_tree(&work);
    fs::create_dir_all(&work)?;
    let bedrock_out = work.join("bedrock");
    let je2be_out = work.join("je2be");
    let chunker_out = work.join("chunker");
    let mut notes: Vec<String> = Vec::new();

    let final_world: PathBuf = match session.world.world_type {
        WorldType::Java => {
            let source_version = &session.world.detected_version;
            let same_version = match (parse_version(source_version), parse_version(&target_version)) {
                (Some(source), Some(target)) => source == target,
                _ => false,
            };
            if same_version {
                sink.log.info(&format!(
                    "源版本与目标一致（{target_version}），逐文件保真复制"
                ));
                copy_tree(&session.world.root, &chunker_out, &sink, 32, 84)?;
            } else {
                let paths = backends::locate(app);
                let chunker = paths
                    .chunker
                    .as_ref()
                    .ok_or_else(|| ConversionError("未找到 chunker-cli.jar，请先运行资源准备脚本".into()))?;
                backends::run_chunker(
                    paths.java.as_deref(),
                    chunker,
                    &session.world.root,
                    &chunker_out,
                    &chunker_format,
                    &sink,
                )?;
                if is_downgrade_opt(source_version, &target_version).unwrap_or(false) {
                    if entity::preserve(&session.world.root, &chunker_out, source_version, &target_version, &sink)? {
                        notes.push("降级转换：不可映射的实体/POI/玩家已保存到 _NWC_preserved_source".into());
                    }
                } else {
                    entity::preserve(&session.world.root, &chunker_out, source_version, &target_version, &sink)?;
                }
            }
            chunker_out
        }
        _ => {
            decrypt::prepare(&session.world, &bedrock_out, &sink)?;
            let paths = backends::locate(app);
            let b2j = paths
                .b2j
                .as_ref()
                .ok_or_else(|| ConversionError("未找到 b2j 后端，请先运行资源准备脚本".into()))?;
            backends::run_je2be(b2j, &bedrock_out, &je2be_out, &sink)?;
            preserve_bedrock_assets(&bedrock_out, &je2be_out, &sink)?;
            if parse_version(&target_version) == Some(Version { major: 1, minor: 21, patch: 10 }) {
                je2be_out
            } else {
                let chunker = paths
                    .chunker
                    .as_ref()
                    .ok_or_else(|| ConversionError("未找到 chunker-cli.jar，请先运行资源准备脚本".into()))?;
                backends::run_chunker(
                    paths.java.as_deref(),
                    chunker,
                    &je2be_out,
                    &chunker_out,
                    &chunker_format,
                    &sink,
                )?;
                let source_version = "1.21.10";
                if is_downgrade_opt(source_version, &target_version).unwrap_or(false) {
                    if entity::preserve(&je2be_out, &chunker_out, source_version, &target_version, &sink)? {
                        notes.push("降级转换：不可映射的实体/POI/玩家已保存到 _NWC_preserved_source".into());
                    }
                } else {
                    entity::preserve(&je2be_out, &chunker_out, source_version, &target_version, &sink)?;
                }
                chunker_out
            }
        }
    };

    sink.update(85, "验证输出", "检查 Anvil 区域结构");
    let validation = validate::validate(&final_world, &sink)?;

    // 与原版一致：转换统计写入输出世界根目录，随 ZIP 一起交付
    let region_note = if notes.is_empty() {
        format!("{} 个区域文件全部通过结构验证", validation.region_files)
    } else {
        notes.join("；")
    };
    let report = [
        "Netease World Converter 转换报告".to_string(),
        format!("时间: {}", chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z")),
        format!("输入 ZIP: {}", session.input_zip.display()),
        format!("识别类型: {}", session.world.world_type.display_name()),
        format!("检测版本: {}", session.world.detected_version),
        format!("目标版本: Java {target_version}"),
        format!("区域文件: {}", validation.region_files),
        format!("区域记录: {}", validation.region_chunks),
        format!("输出文件: {}", validation.files),
        format!("输出字节: {}", validation.bytes),
        format!("输出 DataVersion: {}", validation.data_version),
        format!("输出 Version.Name: {}", validation.level_version),
        format!("实体/POI/玩家处理: {region_note}"),
        String::new(),
        "说明：跨版本降级无法表示目标版本尚不存在的内容；原始 ZIP 从未被修改。".to_string(),
    ];
    fs::write(
        final_world.join("NeteaseWorldConverter-report.txt"),
        report.join("\n") + "\n",
    )?;
    sink.log.info("已写入转换报告");

    let folder_name = safe_folder_name(&session.world.world_name);
    let base = safe_folder_name(&strip_extension(&file_name(&session.input_zip)));
    let zip_name = format!("{base}-{}.zip", target_version.replace(' ', "_"));
    let result_zip = session.temp_dir.path().join(&zip_name);
    create_zip(&final_world, &result_zip, &folder_name, &sink)?;
    sink.update(100, "完成", "转换成功");

    sink.log.info(&format!(
        "转换完成：{}（区域文件 {}，chunk {}）",
        result_zip.display(),
        validation.region_files,
        validation.region_chunks
    ));

    let stored = StoredResult {
        result_zip: result_zip.clone(),
        file_name: zip_name.clone(),
        target_version: target_version.clone(),
        region_files: validation.region_files,
        region_chunks: validation.region_chunks,
        region_note: region_note.clone(),
    };
    *session.result.lock().unwrap() = Some(stored);

    Ok(ConversionResultDto {
        result_zip: result_zip.display().to_string(),
        file_name: zip_name,
        target_version,
        region_files: validation.region_files,
        region_chunks: validation.region_chunks,
        region_note,
    })
}

/// 基岩根目录里对 Java 版仍有意义的附件：datapacks / resources.zip / icon.png。
fn preserve_bedrock_assets(bedrock: &Path, java_out: &Path, sink: &Sink) -> Result<()> {
    for item in ["datapacks", "resources.zip", "icon.png"] {
        let source = bedrock.join(item);
        if !source.exists() {
            continue;
        }
        let destination = java_out.join(item);
        if source.is_dir() {
            delete_tree(&destination);
            copy_tree(&source, &destination, sink, 64, 65)?;
        } else if source.is_file() {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source, &destination)?;
            sink.log.info(&format!("保留基岩附件：{item}"));
        }
    }
    Ok(())
}

// ---------- 保存 / 报告 ----------

pub fn save_result(session_id: &str, destination: &str) -> Result<String> {
    let session = find_session(session_id)?;
    let stored = session
        .result
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| ConversionError("当前没有可保存的转换结果".into()))?;
    let mut destination = PathBuf::from(destination);
    if destination.extension().is_none() {
        destination.set_extension("zip");
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&stored.result_zip, &destination)?;
    Ok(destination.display().to_string())
}

pub fn export_analysis_error(_app: &AppHandle, path: &str, message: &str) -> Result<Option<String>> {
    let input = PathBuf::from(path);
    let existing_log = sessions()
        .lock()
        .unwrap()
        .values()
        .find(|session| session.input_zip == input)
        .map(|session| session.log.file().to_path_buf());
    match existing_log {
        Some(log_file) => Ok(Some(export_error_report(&log_file, &input, message)?)),
        None => {
            let temp = tempfile::Builder::new()
                .prefix("NeteaseWorldConverter-")
                .tempdir()?;
            let log_file = temp.path().join("conversion.log");
            let log = AppLog::new(&log_file)?;
            log.info(&format!("输入文件：{input:?}"));
            log.error("存档解析失败", &ConversionError(message.to_string()));
            Ok(Some(export_error_report(&log_file, &input, message)?))
        }
    }
}

pub fn export_conversion_error(session_id: &str, message: &str) -> Result<Option<String>> {
    let session = find_session(session_id)?;
    Ok(Some(export_error_report(
        session.log.file(),
        &session.input_zip,
        message,
    )?))
}

/// 复制 conversion.log 到输入 ZIP 旁边（不可写则回退桌面），文件名 `<base>-error-<时间戳>.log`。
fn export_error_report(log_file: &Path, input: &Path, message: &str) -> Result<String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let base = safe_folder_name(&strip_extension(&file_name(input)));
    let name = format!("{base}-error-{stamp}.log");
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(parent) = input.parent() {
        candidates.push(parent.join(&name));
    }
    if let Some(desktop) = desktop_dir() {
        candidates.push(desktop.join(name));
    }
    let mut last_error: Option<std::io::Error> = None;
    for destination in candidates {
        match fs::copy(log_file, &destination) {
            Ok(_) => {
                use std::io::Write;
                if let Ok(mut file) = fs::OpenOptions::new().append(true).open(&destination) {
                    let _ = writeln!(file, "[REPORT] {message}");
                }
                return Ok(destination.display().to_string());
            }
            Err(error) => last_error = Some(error),
        }
    }
    conv(format!(
        "错误报告导出失败：{}",
        last_error.map(|error| error.to_string()).unwrap_or_default()
    ))
}

fn desktop_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
    Some(home.join("Desktop"))
}

// ---------- 控制 ----------

pub fn is_downgrade(session_id: &str, target: &str) -> Result<bool> {
    let session = find_session(session_id)?;
    // 与原版一致：基岩存档以 JE2BE 的中间版本 1.21.10 作为源版本参与比较
    let source = if session.world.world_type == WorldType::Java {
        session.world.detected_version.clone()
    } else {
        "1.21.10".to_string()
    };
    Ok(is_downgrade_opt(&source, target).unwrap_or(false))
}

pub fn cancel(session_id: &str) -> Result<()> {
    let session = find_session(session_id)?;
    session.cancel.store(true, Ordering::Relaxed);
    backends::kill_all();
    Ok(())
}

pub fn shutdown_cleanup() -> Result<()> {
    backends::kill_all();
    let mut all = sessions().lock().unwrap();
    for session in all.values() {
        session.cancel.store(true, Ordering::Relaxed);
    }
    all.clear(); // TempDir 析构时自动删除临时目录
    Ok(())
}

// ---------- 文件对话框 / 打开 ----------

pub fn pick_input_path() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Minecraft 存档", &["zip", "mcworld"])
        .set_title("选择存档 ZIP")
        .pick_file()
        .map(|path| path.display().to_string())
}

pub fn pick_save_path(default_name: &str) -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("ZIP 存档", &["zip"])
        .set_file_name(default_name)
        .set_title("保存转换结果")
        .save_file()
        .map(|path| path.display().to_string())
}

pub fn open_path(path: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
            .map_err(|error| ConversionError(format!("无法打开文件位置：{error}")))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|error| ConversionError(format!("无法打开文件位置：{error}")))?;
    }
    #[cfg(target_os = "linux")]
    {
        let target = Path::new(path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(path));
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|error| ConversionError(format!("无法打开文件位置：{error}")))?;
    }
    Ok(())
}
