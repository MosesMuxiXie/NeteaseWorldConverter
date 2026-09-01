// 进度/取消抽象：对应 Java 的 ProgressSink + SwingWorker 取消机制。
// 进度通过回调发出（引擎注入 Tauri 事件发射器，测试注入空操作），不绑定具体运行时。

use crate::error::{conv_code, Result, CODE_CANCELLED};
use crate::log::AppLog;
use crate::models::ProgressPayload;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Sink {
    pub session_id: String,
    pub cancel: Arc<AtomicBool>,
    pub log: Arc<AppLog>,
    on_progress: Box<dyn Fn(ProgressPayload) + Send + Sync>,
}

impl Sink {
    pub fn new(
        session_id: String,
        cancel: Arc<AtomicBool>,
        log: Arc<AppLog>,
        on_progress: impl Fn(ProgressPayload) + Send + Sync + 'static,
    ) -> Sink {
        Sink {
            session_id,
            cancel,
            log,
            on_progress: Box::new(on_progress),
        }
    }

    pub fn update(&self, percent: i32, stage: &str, detail: &str) {
        let percent = percent.clamp(0, 100) as u32;
        (self.on_progress)(ProgressPayload {
            session_id: self.session_id.clone(),
            percent,
            stage: stage.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn check_cancel(&self) -> Result<()> {
        if self.is_cancelled() {
            return conv_code(CODE_CANCELLED, "操作已取消");
        }
        Ok(())
    }
}
