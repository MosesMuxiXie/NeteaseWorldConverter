// commands.rs — Tauri IPC 命令层：前端 main.js 的 12 个 invoke 入口。

use crate::engine;
use crate::models::{AnalysisDto, BackendStatusDto, ConversionResultDto};
use std::path::Path;
use tauri::AppHandle;

/// 重型操作放入阻塞线程池，避免卡住主线程与事件循环。
async fn run_blocking<T, F>(app: AppHandle, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle) -> crate::error::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || task(&app).map_err(|error| error.0))
        .await
        .map_err(|error| format!("后台任务执行失败：{error}"))?
}

#[tauri::command]
pub async fn analyze(app: AppHandle, path: String) -> Result<AnalysisDto, String> {
    run_blocking(app, move |app| engine::analyze(app, Path::new(&path))).await
}

#[tauri::command]
pub async fn convert(
    app: AppHandle,
    session_id: String,
    target: String,
) -> Result<ConversionResultDto, String> {
    run_blocking(app, move |app| engine::convert(app, &session_id, &target)).await
}

#[tauri::command]
pub async fn backend_status(app: AppHandle) -> Result<BackendStatusDto, String> {
    tauri::async_runtime::spawn_blocking(move || crate::backends::status(&app))
        .await
        .map_err(|error| format!("后台任务执行失败：{error}"))
}

#[tauri::command]
pub async fn pick_input_path() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(engine::pick_input_path)
        .await
        .map_err(|error| format!("文件对话框失败：{error}"))
}

#[tauri::command]
pub async fn pick_save_path(default_name: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || engine::pick_save_path(&default_name))
        .await
        .map_err(|error| format!("文件对话框失败：{error}"))
}

#[tauri::command]
pub async fn save_result(session_id: String, destination: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || engine::save_result(&session_id, &destination))
        .await
        .map_err(|error| format!("保存任务执行失败：{error}"))?
        .map_err(|error| error.0)
}

#[tauri::command]
pub async fn export_analysis_error(
    app: AppHandle,
    path: String,
    message: String,
) -> Result<Option<String>, String> {
    run_blocking(app, move |app| engine::export_analysis_error(app, &path, &message)).await
}

#[tauri::command]
pub async fn export_conversion_error(
    session_id: String,
    message: String,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || engine::export_conversion_error(&session_id, &message))
        .await
        .map_err(|error| format!("报告导出失败：{error}"))?
        .map_err(|error| error.0)
}

#[tauri::command]
pub fn cancel(session_id: String) -> Result<(), String> {
    engine::cancel(&session_id).map_err(|error| error.0)
}

#[tauri::command]
pub fn is_downgrade(session_id: String, target: String) -> Result<bool, String> {
    engine::is_downgrade(&session_id, &target).map_err(|error| error.0)
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    engine::open_path(&path).map_err(|error| error.0)
}

#[tauri::command]
pub fn shutdown_cleanup() -> Result<(), String> {
    engine::shutdown_cleanup().map_err(|error| error.0)
}
