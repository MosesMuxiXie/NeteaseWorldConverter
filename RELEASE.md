# NeteaseWorldConverter 2.0.0 发布说明

## 2.0.0：新的界面，更完整的转换流程

- 樱色与奶油白双栏界面，按“选择存档 → 设置版本 → 保存世界”引导操作；小窗口也能显示关键操作。
- 转换完成后显示结果卡片，明确区分“转换完成”和“已保存”，减少重复弹窗。
- 修复 Tauri 进度与日志事件订阅，补齐主窗口所需事件和关闭权限。
- 版本检查、文件选择与保存期间锁定当前会话，防止重复点击和文件切换导致状态串扰。
- 取消后可重新转换；更换存档、重新转换或退出时保护尚未保存的结果。
- 保存禁止覆盖原始存档，采用同目录临时文件写入后替换，失败时保留已有目标文件。
- 降级提示使用后端版本判断；模态框支持键盘焦点、Escape 和屏幕阅读器，日志阅读不再被强制滚动打断。
- 支持 Java 26.3 目标，修复后端退出时日志收尾、输出目录准备和错误信息保留。
- 增加前端流程回归、真实 WebView 流程与保存安全测试；修正测试世界的 ZIP/NBT 生成。

本地验证：18 项 Rust 单元测试、前端流程回归、Java 测试世界识别/转换/保存端到端，以及最小窗口布局、降级确认、取消后重试与解析失败恢复。原生文件对话框输入未完成自动化验证。

## 平台与形态

| 平台 | 安装包 | 说明 |
|---|---|---|
| Windows x64 | NSIS（`*-setup.exe`）/ MSI | CI 构建 |
| Windows arm64 | NSIS（`*-arm64-setup.exe`） | 原生 arm64 应用与 Java 运行时；b2j 优先源码构建原生 arm64 版，失败回退 vendored x64（经 Win11 ARM 模拟层运行） |
| macOS x64 / arm64 | `.dmg` | CI 构建；b2j 从 je2be-core 源码本机构建 |

四个平台统一由 `.github/workflows/release.yml` 构建：`verify` 任务先跑前端流程回归、`cargo fmt --check`、`cargo clippy -D warnings` 和 `cargo test`，全部通过后才开始出安装包。

## 1.2.1 相对 1.2.0 的变化

- 转换期间实时显示已运行时间、主程序与 b2j/Java 后端的合计 CPU 使用率和内存占用
- 静默阶段使用流动进度条与活动指示灯，明确区分“仍在运行”和“已经卡死”
- 进度条补充 ARIA 状态，并遵循系统“减少动态效果”设置
- 新增 WebView2 资源监控回归脚本与 Rust 资源聚合单元测试

## 1.2.0 相对 1.0.x 的变化

### 数据与资源安全

- **会话回收**：最多保留 3 个分析会话，超出的最旧空闲会话连同临时目录自动释放；转换中的会话受保护
- **退出不卡窗**：退出时临时目录先同卷改名（O(1)）再由后台线程删除；应用启动时自动清扫历史实例残留的临时目录
- **磁盘空间预检**：解压前按 ZIP×3+256MiB、转换前按世界×3+1GiB 检查临时盘可用空间，不足时明确报错
- **实体保留覆盖全部命名空间**：`dimensions/<任意命名空间>/<任意维度>/entities|poi`（含模组与数据驱动维度）与旧式 `world/DIM-1/DIM1` 布局在降级时都会进入 `_NWC_preserved_source/`，升级时按原相对路径落回
- **基岩附件不再静默丢失**：基岩→非 1.21.10 目标经 Chunker 转换后，若 datapacks / resources.zip / icon.png 未被透传，自动从解密输出补齐
- **子进程进程树终止**：Windows 上每个后端子进程绑定 KILL_ON_JOB_CLOSE 的 Job Object，取消时整棵进程树一并终止；运行结束回收跟踪句柄，不再累积

### 性能

- **区域验证并行化**（rayon，按区域文件并行）
- **LevelDB 解密并行化**（rayon，按文件并行）+ **XOR 密钥按 u64 字块向量化**（与逐字节实现对拍验证等价）
- **旧版 AES 快速嗅探**：不解压、仅扫 ZIP central directory 读 db 条目头部；最常见的"旧版加密无法离线转换"场景从数十秒降到秒级
- **sysinfo 最小刷新**（仅刷内存信息）、release profile 改为 `opt-level=3 + lto="thin" + codegen-units=1`
- 流水线基准（`cargo run --release --example pipeline-bench`）：解压 210–250 MiB/s、打包 150–280 MiB/s、解密 0.6–2.3 GiB/s、验证 0.6–1.5 GiB/s（i7-1365U 实测）

### 健壮性与工程化

- **结构化错误**：IPC 错误统一为 `{"code","message"}`（error / cancelled / timeout），前端按 code 分类，不再字符串匹配
- **后端心跳超时**：后端超过 10 分钟无输出视为挂死，自动终止并报错
- **前端状态机修复**：转换期间锁定选择/拖放入口，消除并发分析与进度交叉污染；日志区上限 1500 行
- **MANIFEST 按 LevelDB 编号数值取最大**（修复非零填充编号下字典序选错，如 MANIFEST-10 < MANIFEST-2）
- 桌面目录走系统 Known Folder API（兼容 OneDrive 重定向）；日志文件持久句柄；`1.21.10` 中间版本收敛为 `JE2BE_INTERMEDIATE` 常量
- 应用标识改为 `io.github.mosesmuxixie.nwc`（从旧版本升级会被视为新应用，旧版需手动卸载）
- 修复 `scripts/make-test-world.mjs` 生成损坏 ZIP 的问题（`deflateSync` → `deflateRawSync`）

## 已知限制（继承自原版）

- 网易基岩**旧版**（90 1D 30 01，AES-CFB8）无法离线恢复密钥；识别后自动导出诊断报告
- 降级转换无法无损表达新内容；不可映射的实体/POI/玩家保存在输出 ZIP 的 `_NWC_preserved_source/`
- Chunker 子进程需要随包 jlink 运行时或系统 Java 17+

## 安装

1. 双击 `NeteaseWorldConverter_2.0.0_x64-setup.exe`（可选每用户 / 每机器安装，简体中文 / English）或使用 MSI 包。
2. 数据全程本机处理，原始 ZIP 永不修改。

## 校验

GitHub Release 记录每个附件的 SHA-256 digest，可通过 Assets API 核验。

## 许可证

GPL-3.0-or-later。Chunker（MIT）、je2be-core（GPL-3.0）。
源码：本仓库（含 `ANALYSIS.md` 逆向报告与 GPL 源码义务对应关系）。
