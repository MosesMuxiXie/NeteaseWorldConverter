// engine.rs — 对应 ConversionEngine.java：
// analyze → targets → convert → validate → zip 的编排核心与会话管理。

use crate::archive::{
    copy_tree, create_zip, delete_tree, ensure_free_space, extract_zip, file_name, peek_legacy_aes,
    safe_folder_name, sha256, strip_extension,
};
use crate::backends;
use crate::decrypt;
use crate::detect;
use crate::entity;
use crate::error::{conv, conv_code, ConversionError, Result, CODE_CANCELLED};
use crate::log::AppLog;
use crate::models::{
    AnalysisDto, ConversionResultDto, LogPayload, TargetVersion, WorldInfo, WorldType,
};
use crate::sink::Sink;
use crate::validate;
use crate::version::{chunker_format, is_downgrade_opt, parse_version, JE2BE_INTERMEDIATE};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tempfile::TempDir;

/// 最多同时保留的会话数；超出的最旧非转换中会话会被回收（含其临时目录）。
const MAX_SESSIONS: usize = 3;

pub struct Session {
    pub session_id: String,
    pub input_zip: PathBuf,
    pub temp_dir: TempDir,
    pub extracted: PathBuf,
    pub world: WorldInfo,
    pub targets: Vec<TargetVersion>,
    pub cancel: Arc<AtomicBool>,
    /// 该会话是否正在转换：转换期间禁止并发 convert 与会话回收。
    converting: AtomicBool,
    pub log: Arc<AppLog>,
    pub result: Mutex<Option<StoredResult>>,
}

/// convert 返回（无论成功失败）后复位 converting 标记。
struct ConvertGuard<'a>(&'a AtomicBool);

impl Drop for ConvertGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
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
        .ok_or_else(|| ConversionError::from(format!("转换会话不存在：{session_id}")))
}

/// 会话回收：超出上限时删除最旧且空闲（未在转换）的会话。
/// session_id 带时间戳前缀，字符串排序即时间排序。
fn reap_sessions() {
    let mut all = sessions().lock().unwrap();
    if all.len() <= MAX_SESSIONS {
        return;
    }
    let mut ids: Vec<String> = all.keys().cloned().collect();
    ids.sort();
    for id in ids {
        if all.len() <= MAX_SESSIONS {
            break;
        }
        let converting = all
            .get(&id)
            .is_some_and(|session| session.converting.load(Ordering::SeqCst));
        if converting {
            continue;
        }
        if let Some(session) = all.remove(&id) {
            session.log.info("会话已由新的分析回收，临时目录即将释放");
        }
    }
}

fn new_sink(
    app: &AppHandle,
    session_id: &str,
    cancel: &Arc<AtomicBool>,
    log: &Arc<AppLog>,
) -> Sink {
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

    // 快速嗅探：旧版 AES-CFB8 无法离线解密，是最常见的失败场景，
    // 未解压前即可判定时跳过全量解压（大 ZIP 可省数十秒）。
    let world = if peek_legacy_aes(&input) {
        log.warn("ZIP 内 db 条目呈 90 1D 30 01 旧版 AES-CFB8 头；跳过全量解压");
        sink.update(12, "识别存档", "旧版 AES 加密（快速嗅探）");
        WorldInfo {
            root: extracted.clone(),
            database_directory: None,
            world_type: WorldType::NeteaseBedrockLegacyAes,
            detected_version: "基岩 LevelDB（旧版 AES-CFB8 加密）".into(),
            world_name: strip_extension(&name),
            file_count: 0,
            byte_count: 0,
            notes: vec!["在解压前即嗅探到 90 1D 30 01 旧版加密头；该格式无法离线恢复密钥".into()],
        }
    } else {
        let zip_size = fs::metadata(&input).map(|m| m.len()).unwrap_or(0);
        ensure_free_space(&temp_path, zip_size * 3 + 256 * 1024 * 1024, "解压临时目录")?;
        extract_zip(&input, &extracted, &sink)?;

        sink.update(12, "识别存档", "正在分析目录结构");
        detect::detect(&extracted, &log)?
    };
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
        match export_error_report(
            log.file(),
            &input,
            "检测到旧版 AES-CFB8 加密，需要外部账号密钥",
        ) {
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
        converting: AtomicBool::new(false),
        log,
        result: Mutex::new(None),
    });
    sessions()
        .lock()
        .unwrap()
        .insert(session_id.clone(), session);
    reap_sessions();

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
        return conv_code(CODE_CANCELLED, "操作已取消");
    }
    if session
        .converting
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return conv("该会话正在转换中，请等待完成或取消");
    }
    let _convert_guard = ConvertGuard(&session.converting);

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
        .or_else(|| parse_version(&target_version).map(chunker_format))
        .ok_or_else(|| ConversionError::from(format!("未知的目标版本：{target_version}")))?;

    // 工作目录需要容纳解密输出、je2be 输出、chunker 输出与最终 ZIP；
    // 以源世界字节数的三倍加 1 GiB 做保守预检。
    ensure_free_space(
        session.temp_dir.path(),
        session.world.byte_count * 3 + 1024 * 1024 * 1024,
        "转换工作目录",
    )?;

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
            let same_version = match (
                parse_version(source_version),
                parse_version(&target_version),
            ) {
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
                let chunker = paths.chunker.as_ref().ok_or_else(|| {
                    ConversionError::from("未找到 chunker-cli.jar，请先运行资源准备脚本")
                })?;
                backends::run_chunker(
                    paths.java.as_deref(),
                    chunker,
                    &session.world.root,
                    &chunker_out,
                    &chunker_format,
                    &sink,
                )?;
                if is_downgrade_opt(source_version, &target_version).unwrap_or(false) {
                    if entity::preserve(
                        &session.world.root,
                        &chunker_out,
                        source_version,
                        &target_version,
                        &sink,
                    )? {
                        notes.push(
                            "降级转换：不可映射的实体/POI/玩家已保存到 _NWC_preserved_source"
                                .into(),
                        );
                    }
                } else {
                    entity::preserve(
                        &session.world.root,
                        &chunker_out,
                        source_version,
                        &target_version,
                        &sink,
                    )?;
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
                .ok_or_else(|| ConversionError::from("未找到 b2j 后端，请先运行资源准备脚本"))?;
            backends::run_je2be(b2j, &bedrock_out, &je2be_out, &sink)?;
            preserve_bedrock_assets(&bedrock_out, &je2be_out, &sink, false)?;
            if parse_version(&target_version) == Some(JE2BE_INTERMEDIATE) {
                je2be_out
            } else {
                let chunker = paths.chunker.as_ref().ok_or_else(|| {
                    ConversionError::from("未找到 chunker-cli.jar，请先运行资源准备脚本")
                })?;
                backends::run_chunker(
                    paths.java.as_deref(),
                    chunker,
                    &je2be_out,
                    &chunker_out,
                    &chunker_format,
                    &sink,
                )?;
                // Chunker 不保证透传 datapacks/resources.zip/icon.png，
                // 最终输出缺失时从解密输出补齐，避免附件静默丢失。
                preserve_bedrock_assets(&bedrock_out, &chunker_out, &sink, true)?;
                let source_version = JE2BE_INTERMEDIATE.to_string();
                if is_downgrade_opt(&source_version, &target_version).unwrap_or(false) {
                    if entity::preserve(
                        &je2be_out,
                        &chunker_out,
                        &source_version,
                        &target_version,
                        &sink,
                    )? {
                        notes.push(
                            "降级转换：不可映射的实体/POI/玩家已保存到 _NWC_preserved_source"
                                .into(),
                        );
                    }
                } else {
                    entity::preserve(
                        &je2be_out,
                        &chunker_out,
                        &source_version,
                        &target_version,
                        &sink,
                    )?;
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
        format!(
            "时间: {}",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z")
        ),
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
/// `only_if_missing` 为 true 时仅补齐输出中缺失的项（Chunker 已透传的不覆盖）。
fn preserve_bedrock_assets(
    bedrock: &Path,
    java_out: &Path,
    sink: &Sink,
    only_if_missing: bool,
) -> Result<()> {
    for item in ["datapacks", "resources.zip", "icon.png"] {
        let source = bedrock.join(item);
        if !source.exists() {
            continue;
        }
        let destination = java_out.join(item);
        if only_if_missing && destination.exists() {
            continue;
        }
        if source.is_dir() {
            delete_tree(&destination);
            copy_tree(&source, &destination, sink, 64, 65)?;
            sink.log.info(&format!("保留基岩附件：{item}"));
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
        .ok_or_else(|| ConversionError::from("当前没有可保存的转换结果"))?;
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

pub fn export_analysis_error(
    _app: &AppHandle,
    path: &str,
    message: &str,
) -> Result<Option<String>> {
    let input = PathBuf::from(path);
    let existing_log = sessions()
        .lock()
        .unwrap()
        .values()
        .filter(|session| session.input_zip == input)
        .max_by(|a, b| a.session_id.cmp(&b.session_id))
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
            log.error("存档解析失败", &ConversionError::from(message));
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
        last_error
            .map(|error| error.to_string())
            .unwrap_or_default()
    ))
}

fn desktop_dir() -> Option<PathBuf> {
    // dirs 走系统 Known Folder API，兼容 OneDrive 重定向桌面；失败回退传统路径
    dirs::desktop_dir().or_else(|| {
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
        Some(home.join("Desktop"))
    })
}

// ---------- 控制 ----------

pub fn is_downgrade(session_id: &str, target: &str) -> Result<bool> {
    let session = find_session(session_id)?;
    // 与原版一致：基岩存档以 JE2BE 的中间版本作为源版本参与比较
    let source = if session.world.world_type == WorldType::Java {
        session.world.detected_version.clone()
    } else {
        JE2BE_INTERMEDIATE.to_string()
    };
    Ok(is_downgrade_opt(&source, target).unwrap_or(false))
}

pub fn cancel(session_id: &str) -> Result<()> {
    let session = find_session(session_id)?;
    session.cancel.store(true, Ordering::Relaxed);
    backends::kill_all();
    Ok(())
}

/// 退出清理：立即返回（不阻塞关窗）。
/// 临时目录先同卷 rename 成孤儿名（O(1)），删除交给后台线程；
/// 进程若随即退出导致线程中断，残留孤儿目录由下次启动的 cleanup_stale_temp 兜底。
pub fn shutdown_cleanup() -> Result<()> {
    backends::kill_all();
    let mut all = sessions().lock().unwrap();
    let mut orphans: Vec<PathBuf> = Vec::new();
    for session in all.values() {
        session.cancel.store(true, Ordering::Relaxed);
        let path = session.temp_dir.path();
        let Some(parent) = path.parent() else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let orphan = parent.join(format!(".{name}-orphan-{}", std::process::id()));
        if fs::rename(path, &orphan).is_ok() {
            orphans.push(orphan);
        }
    }
    // clear() 析构 TempDir：已 rename 的目录检测到不存在即跳过，
    // rename 失败的（极少，如句柄未释放）保持旧的同步删除行为。
    all.clear();
    std::thread::spawn(move || {
        for orphan in orphans {
            let _ = fs::remove_dir_all(orphan);
        }
    });
    Ok(())
}

/// 启动时清扫历史实例残留的临时目录：
/// - 孤儿目录（仅由已退出实例的 shutdown 创建）直接删除；
/// - 普通前缀目录需修改时间超过 2 小时，避免误删并行实例的工作目录。
pub fn cleanup_stale_temp() {
    std::thread::spawn(|| {
        let temp_root = std::env::temp_dir();
        let Ok(entries) = fs::read_dir(&temp_root) else {
            return;
        };
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            let stale = if name.starts_with(".NeteaseWorldConverter-") && name.contains("-orphan-")
            {
                true
            } else if name.starts_with("NeteaseWorldConverter-") {
                entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age > Duration::from_secs(2 * 3600))
            } else {
                continue;
            };
            if stale {
                let _ = fs::remove_dir_all(&path);
            }
        }
    });
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
            .map_err(|error| ConversionError::from(format!("无法打开文件位置：{error}")))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|error| ConversionError::from(format!("无法打开文件位置：{error}")))?;
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
            .map_err(|error| ConversionError::from(format!("无法打开文件位置：{error}")))?;
    }
    Ok(())
}
