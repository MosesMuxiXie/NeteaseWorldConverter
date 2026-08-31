// backends.rs — 对应 Backends.java：定位并调度 Java / Chunker / b2j 外部后端。

use crate::archive::file_name;
use crate::error::{conv, ConversionError, Result};
use crate::models::{BackendStatusDto, TargetVersion};
use crate::sink::Sink;
use crate::version::chunker_format;
use regex::Regex;
use std::collections::{BTreeSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::Manager;

/// 所有由本模块启动的子进程，shutdown_cleanup / cancel 时统一清理。
/// 使用 Arc<Mutex<Child>> 使运行中的 run_process 与全局清理共享同一句柄。
static CHILDREN: Mutex<Vec<Arc<Mutex<Child>>>> = Mutex::new(Vec::new());

fn track(child: Arc<Mutex<Child>>) {
    CHILDREN.lock().unwrap().push(child);
}

/// 终止所有仍在运行的子进程（结果无效也不影响取消流程）。
pub fn kill_all() {
    let children = CHILDREN.lock().unwrap();
    for child in children.iter() {
        if let Ok(mut guard) = child.lock() {
            let _ = guard.kill();
        }
    }
}

#[derive(Clone)]
pub struct BackendPaths {
    pub java: Option<PathBuf>,
    pub chunker: Option<PathBuf>,
    pub b2j: Option<PathBuf>,
}

/// 在资源目录、可执行文件目录、开发目录中查找后端。
pub fn locate(app: &tauri::AppHandle) -> BackendPaths {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(resource) = app.path().resource_dir() {
        dirs.push(resource.join("backends"));
        dirs.push(resource.join("runtime"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.to_path_buf();
            for sub in [
                "",
                "backends",
                "runtime",
                "app",
                "app/backends",
                "app/runtime",
                "app/native",
                "resources",
                "resources/backends",
                "resources/runtime",
                "_up_/resources",
                "_up_/resources/backends",
                "_up_/resources/runtime",
            ] {
                if sub.is_empty() {
                    dirs.push(p.clone());
                } else {
                    dirs.push(p.join(sub));
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.clone());
        dirs.push(cwd.join("src-tauri/backends"));
        dirs.push(cwd.join("src-tauri/runtime"));
    }

    let java = dirs
        .iter()
        .map(|dir| dir.join("bin"))
        .flat_map(|bin| ["java.exe", "java"].map(|name| bin.join(name)))
        .find(|candidate| candidate.is_file());

    let chunker = dirs
        .iter()
        .find_map(|dir| {
            let direct = dir.join("chunker-cli.jar");
            if direct.is_file() {
                return Some(direct);
            }
            if let Ok(entries) = std::fs::read_dir(dir) {
                return entries.filter_map(|entry| entry.ok()).map(|entry| entry.path()).find(
                    |path| {
                        path.is_file()
                            && path.file_name().is_some_and(|name| {
                                name.to_string_lossy().to_lowercase() == "chunker-cli.jar"
                            })
                    },
                );
            }
            None
        });

    let b2j = dirs
        .iter()
        .flat_map(|dir| ["b2j.exe", "b2j"].map(|name| dir.join(name)))
        .find(|candidate| candidate.is_file());

    BackendPaths { java, chunker, b2j }
}

/// 后端可用性探测（前端初始化时调用）。
pub fn status(app: &tauri::AppHandle) -> BackendStatusDto {
    let paths = locate(app);
    let mut missing: Vec<&str> = Vec::new();

    let java_version = match &paths.java {
        Some(java) => java_version(java),
        None => probe_system_java(),
    };
    if java_version.is_none() {
        missing.push("Java 运行时");
    }
    if paths.chunker.is_none() {
        missing.push("Chunker CLI");
    }
    if paths.b2j.is_none() {
        missing.push("b2j（Bedrock→Java）");
    }

    let ok = missing.is_empty();
    BackendStatusDto {
        ok,
        java: java_version.unwrap_or_else(|| "未找到".to_string()),
        chunker: paths
            .chunker
            .as_ref()
            .map(|path| file_name(path))
            .unwrap_or_else(|| "未找到".to_string()),
        b2j: paths
            .b2j
            .as_ref()
            .map(|path| file_name(path))
            .unwrap_or_else(|| "未找到".to_string()),
        message: if ok {
            String::new()
        } else {
            format!(
                "缺少 {}。请运行 npm run prepare:win（或 prepare:unix）填充 backends 与 runtime 目录。",
                missing.join("、")
            )
        },
    }
}

fn java_version(java: &Path) -> Option<String> {
    let output = Command::new(java).arg("-version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout)
    } else {
        stderr
    };
    text.lines().next().map(|line| line.trim().to_string())
}

fn probe_system_java() -> Option<String> {
    let output = Command::new("java").arg("-version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout)
    } else {
        stderr
    };
    text.lines().next().map(|line| line.trim().to_string())
}

fn java_available(paths: &BackendPaths) -> bool {
    paths.java.as_ref().map(|j| java_version(j).is_some()).unwrap_or(false) || probe_system_java().is_some()
}

/// 可用目标版本列表：优先询问 Chunker，失败回退内置清单。
pub fn list_target_versions(app: &tauri::AppHandle) -> Vec<TargetVersion> {
    let paths = locate(app);
    if let Some(chunker) = &paths.chunker {
        if java_available(&paths) {
            if let Some(list) = query_chunker(paths.java.as_deref(), chunker) {
                return list;
            }
        }
    }
    builtin_targets()
}

static TARGET_RE: OnceLock<Regex> = OnceLock::new();

fn query_chunker(java: Option<&Path>, chunker: &Path) -> Option<Vec<TargetVersion>> {
    let re = TARGET_RE.get_or_init(|| Regex::new(r"JAVA_(?:26|1)_\d+(?:_\d+){0,2}").expect("后端版本正则"));
    let mut command = java_command(java);
    command.arg("-jar").arg(chunker).arg("-f").arg("?");
    if let Some(parent) = chunker.parent() {
        command.current_dir(parent);
    }
    let output = command.output().ok()?;
    // `-f ?` 会以非零退出码在 stderr 中打印完整枚举值，两路输出都要解析
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut targets: Vec<((i32, i32, i32), String)> = Vec::new();
    for matched in re.find_iter(&text) {
        let token = matched.as_str().to_string();
        if !seen.insert(token.clone()) {
            continue;
        }
        if let Some(parsed) = parse_target_token(&token) {
            targets.push((parsed, token));
        }
    }
    if targets.is_empty() {
        return None;
    }
    targets.sort_by(|a, b| b.0.cmp(&a.0));
    Some(
        targets
            .into_iter()
            .map(|(version, format)| TargetVersion {
                display_name: display_version(version),
                chunker_format: format,
            })
            .collect(),
    )
}

/// JAVA_26_2 / JAVA_1_21_10 / JAVA_1_12 → (major, minor, patch)；26.x ≤2，1.12–1.21。
fn parse_target_token(token: &str) -> Option<(i32, i32, i32)> {
    let parts: Vec<&str> = token.split('_').collect();
    if parts.len() < 3 {
        return None;
    }
    let major: i32 = parts[1].parse().ok()?;
    let minor: i32 = parts[2].parse().ok()?;
    let patch: i32 = parts.get(3).and_then(|p| p.parse().ok()).unwrap_or(0);
    match major {
        26 if minor <= 2 => Some((major, minor, patch)),
        1 if (12..=21).contains(&minor) => Some((major, minor, patch)),
        _ => None,
    }
}

fn display_version(version: (i32, i32, i32)) -> String {
    let (major, minor, patch) = version;
    if patch > 0 {
        format!("Java {major}.{minor}.{patch}")
    } else {
        format!("Java {major}.{minor}")
    }
}

/// 内置回退清单：与 Chunker 1.19.1 实际枚举对齐（26.x ≤2；1.12–1.21），按版本号降序。
fn builtin_targets() -> Vec<TargetVersion> {
    let mut list: Vec<((i32, i32, i32), String)> = Vec::new();
    let modern: &[(i32, i32, i32)] = &[(26, 2, 0), (26, 1, 2), (26, 1, 1), (26, 1, 0)];
    for (major, minor, patch) in modern {
        let format = if *patch > 0 {
            format!("JAVA_{major}_{minor}_{patch}")
        } else {
            format!("JAVA_{major}_{minor}")
        };
        list.push(((*major, *minor, *patch), format));
    }
    let patches: &[(i32, i32)] = &[
        (21, 11),
        (20, 6),
        (19, 4),
        (18, 2),
        (17, 1),
        (16, 5),
        (15, 2),
        (14, 4),
        (13, 2),
        (12, 2),
    ];
    for (minor, max_patch) in patches {
        for patch in (0..=*max_patch).rev() {
            list.push(((1, *minor, patch), chunker_format(crate::version::Version { major: 1, minor: *minor, patch })));
        }
    }
    list.sort_by(|a, b| b.0.cmp(&a.0));
    list.into_iter()
        .map(|(version, format)| TargetVersion {
            display_name: display_version(version),
            chunker_format: format,
        })
        .collect()
}

fn java_command(java: Option<&Path>) -> Command {
    match java {
        Some(java) => Command::new(java),
        None => Command::new("java"),
    }
}

/// 基岩→Java 1.21.10（进度 31→64，每行输出 +1，到顶回卷）。
pub fn run_je2be(b2j: &Path, input: &Path, output: &Path, sink: &Sink) -> Result<()> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 16);
    let working_dir = b2j.parent().unwrap_or_else(|| Path::new("."));
    let args = vec![
        "-i".to_string(),
        input.display().to_string(),
        "-o".to_string(),
        output.display().to_string(),
        "-n".to_string(),
        threads.to_string(),
    ];
    let mut progress = 30i32;
    run_process(
        b2j,
        &args,
        working_dir,
        Some(working_dir),
        sink,
        Some(&mut |_line| {
            progress += 1;
            if progress > 64 {
                progress = 31;
            }
            sink.update(progress, "JE2BE 基岩→Java", "b2j 正在转换");
            Ok(())
        }),
    )?;
    if !output.join("level.dat").is_file() {
        return conv("b2j 进程已结束，但输出目录缺少 level.dat");
    }
    Ok(())
}

/// Chunker 跨版本转换（进度 64→84，按输出行内百分比换算）。
pub fn run_chunker(
    java: Option<&Path>,
    chunker: &Path,
    input: &Path,
    output: &Path,
    format: &str,
    sink: &Sink,
) -> Result<()> {
    let system = sysinfo::System::new();
    let total_gb = system.total_memory() / (1024 * 1024 * 1024);
    let heap_gb = (((total_gb as f64) * 0.7).round() as u64).clamp(2, 12);
    let args = vec![
        "-Xms512m".to_string(),
        format!("-Xmx{heap_gb}G"),
        "-jar".to_string(),
        chunker.display().to_string(),
        "-i".to_string(),
        input.display().to_string(),
        "-o".to_string(),
        output.display().to_string(),
        "-f".to_string(),
        format.to_string(),
    ];
    let working_dir = chunker.parent().unwrap_or_else(|| Path::new("."));
    let program = java.map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("java"));
    run_process(
        &program,
        &args,
        working_dir,
        None,
        sink,
        Some(&mut |line| {
            for token in line.split_whitespace() {
                if token.ends_with('%') {
                    let number = token.trim_end_matches('%');
                    if let Ok(percent) = number.parse::<u32>() {
                        let mapped = 64 + ((percent * 20 / 100).min(20)) as i32;
                        sink.update(mapped, "Chunker 跨版本转换", &format!("{percent}%"));
                        return Ok(());
                    }
                }
            }
            Ok(())
        }),
    )?;
    if !output.join("level.dat").is_file() {
        return conv("Chunker 进程已结束，但输出目录缺少 level.dat");
    }
    Ok(())
}

/// 通用子进程执行：合并 stdout/stderr 逐行写日志；250ms 轮询取消；可取消。
#[allow(clippy::too_many_arguments)]
pub fn run_process(
    program: &Path,
    args: &[String],
    working_dir: &Path,
    prepend_path: Option<&Path>,
    sink: &Sink,
    mut on_line: Option<&mut dyn FnMut(&str) -> Result<()>>,
) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = prepend_path {
        if let Some(current) = std::env::var_os("PATH") {
            let mut paths: Vec<PathBuf> = std::env::split_paths(&current).collect();
            paths.insert(0, dir.to_path_buf());
            if let Ok(joined) = std::env::join_paths(paths) {
                command.env("PATH", joined);
            }
        }
    }
    let child = command.spawn().map_err(|error| {
        ConversionError(format!(
            "无法启动 {}：{error}",
            program.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| program.display().to_string())
        ))
    })?;
    let child = Arc::new(Mutex::new(child));
    track(child.clone());
    let (stdout, stderr) = {
        let mut guard = child.lock().unwrap();
        (
            guard.stdout.take().expect("stdout 已配置为管道"),
            guard.stderr.take().expect("stderr 已配置为管道"),
        )
    };

    let (sender, receiver) = mpsc::channel::<String>();
    let sender_out = sender.clone();
    let reader_out = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            if sender_out.send(line).is_err() {
                break;
            }
        }
    });
    let sender_err = sender;
    let reader_err = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            if sender_err.send(line).is_err() {
                break;
            }
        }
    });

    let program_name = program
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| program.display().to_string());
    let mut last_lines: VecDeque<String> = VecDeque::with_capacity(30);
    let exit_status: Option<std::process::ExitStatus> = loop {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(line) => {
                last_lines.push_back(line.clone());
                if last_lines.len() > 30 {
                    last_lines.pop_front();
                }
                sink.log.info(&line);
                if let Some(callback) = on_line.as_deref_mut() {
                    if let Err(error) = callback(&line) {
                        terminate(&child);
                        return Err(error);
                    }
                }
                if let Err(error) = sink.check_cancel() {
                    terminate(&child);
                    return Err(error);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // 两个读线程都已退出，等待子进程结束
                let waited = child.lock().unwrap().wait();
                match waited {
                    Ok(status) => break Some(status),
                    Err(_) => break None,
                }
            }
        }
        let waited = child.lock().unwrap().try_wait();
        match waited {
            Ok(Some(status)) => {
                while let Ok(line) = receiver.try_recv() {
                    sink.log.info(&line);
                    if let Some(callback) = on_line.as_deref_mut() {
                        let _ = callback(&line);
                    }
                }
                break Some(status);
            }
            Ok(None) => {}
            Err(_) => break None,
        }
        if sink.is_cancelled() {
            terminate(&child);
            return conv("操作已取消");
        }
    };
    let _ = reader_out.join();
    let _ = reader_err.join();

    match exit_status {
        Some(status) if status.success() => Ok(()),
        Some(status) => {
            let tail: Vec<String> = last_lines.into_iter().collect();
            conv(format!(
                "{program_name} 退出码 {}；最近输出：\n{}",
                status.code().unwrap_or(-1),
                tail.join("\n")
            ))
        }
        None => conv(format!("{program_name} 进程异常结束")),
    }
}

/// kill 后最多等待 3 秒，超时强制结束。
fn terminate(child: &Mutex<Child>) {
    if let Ok(mut guard) = child.lock() {
        let _ = guard.kill();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if guard.try_wait().ok().flatten().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
