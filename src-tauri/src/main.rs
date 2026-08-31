// main.rs — 二进制入口（Windows 发布版不弹出控制台窗口）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nwc_lib::run();
}
