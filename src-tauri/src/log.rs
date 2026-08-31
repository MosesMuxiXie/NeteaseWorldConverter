// 对应 AppLog.java：线程安全的日志文件 + UI 回调。

use crate::error::ConversionError;
use chrono::Local;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type Listener = Box<dyn Fn(&str) + Send + Sync>;

pub struct AppLog {
    file: PathBuf,
    listener: Mutex<Option<Listener>>,
    write_lock: Mutex<()>,
}

impl AppLog {
    pub fn new(file: &Path) -> std::io::Result<AppLog> {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file, b"")?;
        Ok(AppLog {
            file: file.to_path_buf(),
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
        lines.push(format!("ConversionError: {}", error.0));
        let mut cause = error.source();
        while let Some(current) = cause {
            lines.push(format!("Caused by: {}: {}", current, current));
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
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.file) {
            let _ = writeln!(file, "{line}");
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
