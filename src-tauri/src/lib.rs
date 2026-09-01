// lib.rs — 库根：模块声明 + Tauri 应用入口。

pub mod archive;
pub mod backends;
pub mod commands;
pub mod decrypt;
pub mod detect;
pub mod engine;
pub mod entity;
pub mod error;
pub mod log;
pub mod models;
pub mod nbt;
pub mod sink;
pub mod validate;
pub mod version;

pub fn run() {
    // 清扫历史实例残留的临时目录（孤儿目录与超龄会话目录）
    engine::cleanup_stale_temp();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::analyze,
            commands::export_analysis_error,
            commands::cancel,
            commands::is_downgrade,
            commands::convert,
            commands::export_conversion_error,
            commands::pick_save_path,
            commands::save_result,
            commands::open_path,
            commands::pick_input_path,
            commands::shutdown_cleanup,
            commands::backend_status,
        ])
        .run(tauri::generate_context!())
        .expect("NeteaseWorldConverter 启动失败");
}
