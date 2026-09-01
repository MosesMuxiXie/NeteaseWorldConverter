// 对应 AppLog.java：线程安全的日志文件 + UI 回调。

use crate::error::ConversionError;
use chrono::Local;
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type Listener = Box<dyn Fn(&str) + Send + Sync>;

pub struct AppLog {
    file: PathBuf,
    // 持久句柄：后端逐行输出频繁，避免每行一次 open/close 系统调用
    handle: Mutex<Option<std::fs::File>>,
    listener: Mutex<Option<Listener>>,
    write_lock: Mutex<()>,
}

impl AppLog {
    pub fn new(file: &Path) -> std::io::Result<AppLog> {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let handle = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)?;
        Ok(AppLog {
            file: file.to_path_buf(),
            handle: Mutex::new(Some(handle)),
            listener: Mutex::new(None),
            write_lock: Mutex::new(()),
        })
    }

    pub fn set_listener(&self, listener: Option<Listener>) {
        *self.listener.lock().unwrap() = listener;
    }

    pub fn info(&self, message: &str) {
        self.write("INFO", message, &[]);
    }

    pub fn warn(&self, message: &str) {
        self.write("WARN", message, &[]);
    }

    pub fn error(&self, message: &str, error: &ConversionError) {
        let mut lines = Vec::new();
        lines.push(format!("ConversionError: {}", error.message));
        let mut cause = error.source();
        while let Some(current) = cause {
            lines.push(format!("Caused by: {current}"));
            cause = current.source();
        }
        self.write("ERROR", message, &lines);
    }

    fn write(&self, level: &str, message: &str, extra: &[String]) {
        let _guard = self.write_lock.lock().unwrap();
        let timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S%:z");
        self.append(&format!("{timestamp} [{level}] {message}"));
        for line in extra {
            self.append(line);
        }
    }

    fn append(&self, line: &str) {
        let payload = format!("{line}\n").into_bytes();
        let ok = {
            let mut guard = self.handle.lock().unwrap();
            match guard.as_mut() {
                Some(file) => file.write_all(&payload).is_ok(),
                None => false,
            }
        };
        if !ok {
            // 句柄失效（外部删除等）时重开一次；仍失败则放弃该行
            if let Ok(file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.file)
            {
                *self.handle.lock().unwrap() = Some(file);
            }
            return;
        }
        if let Ok(listener) = self.listener.lock() {
            if let Some(callback) = listener.as_ref() {
                callback(line);
            }
        }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
}
