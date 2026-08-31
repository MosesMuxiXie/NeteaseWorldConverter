# NeteaseWorldConverter 1.0.0（Tauri 移植版）发布说明

## 平台与形态

| 平台 | 安装包 | 说明 |
|---|---|---|
| Windows x64 | NSIS（`*-setup.exe`）/ MSI / 便携 ZIP | 本机构建并完成全部端到端实测 |
| Windows arm64 | NSIS（`*-arm64-setup.exe`） | 本机构建；b2j 与 jlink runtime 为 x64，经 Windows 11 ARM 模拟层运行，需在 ARM 设备上自测 |
| macOS x64 / arm64 | `.app` + `.dmg` | 需在对应架构的 macOS 机器或 GitHub Actions（`.github/workflows/release.yml`）上构建；b2j 从 je2be-core 源码本机构建 |

## 这是什么

网易 Minecraft 存档转换器的 Tauri 2 跨平台移植（Rust + WebView）。与原版 Java Swing 程序行为一一对应：
同一套 ZIP 安全解压、存档识别、网易基岩解密（80 1D 30 01 循环 XOR 密钥恢复）、
b2j / Chunker 外部后端、Anvil 逐区域验证与 `_NWC_preserved_source` 降级保留策略。

## 相对原版的变化

- **跨平台**：Windows（NSIS / MSI）、macOS（.app / .dmg，b2j 需按 README 从 je2be-core 构建）
- 界面由 Swing 改为 WebView，交互与状态机一致（拖放、降级确认、取消转换、错误报告导出、逐行日志、0-100 进度）
- 后端资源随安装包分发：`chunker-cli.jar`、`b2j.exe`（含 LLVM DLL）、jlink Java 26 运行时（Chunker 子进程用）
- 目标版本列表改为**实时询问 Chunker**（1.21.11 → 1.12、26.2 / 26.1.x），失败时回退内置清单

## 已知限制（继承自原版）

- 网易基岩**旧版**（90 1D 30 01，AES-CFB8）无法离线恢复密钥；识别后自动导出诊断报告
- 降级转换无法无损表达新内容；不可映射的实体/POI/玩家保存在输出 ZIP 的 `_NWC_preserved_source/`
- Chunker 子进程需要随包 jlink 运行时或系统 Java 17+

## 安装

1. 双击 `NeteaseWorldConverter_1.0.0_x64-setup.exe`（可选每用户 / 每机器安装，简体中文 / English）
   或使用 MSI 包。
2. 首次启动若提示缺少后端资源，说明打包资源缺失——请从源码构建：
   `npm install && npm run prepare:win && npm run build`。
3. 数据全程本机处理，原始 ZIP 永不修改。

## 校验

SHA-256 见随发布附带的 `SHA256SUMS.txt`。

## 端到端验证记录（本机实测）

- **后端探测**：随包 jlink Java 26.0.2 + chunker-cli.jar + b2j.exe 全部定位成功；目标版本列表实时询问 Chunker（53 项，26.2 → 1.12）
- **Java 同版本保真复制**：真实存档 Redstone（1.20.2，8 MB）→ 1.20.2：4 个区域文件、2025 个 chunk 全部通过逐区域结构验证，输出 ZIP 与源逐字节一致
- **Java 降级转换（Chunker）**：Redstone 1.20.2 → 1.16.5：Chunker 子进程转换成功，level.dat 版本号 1.16.5 / DataVersion 2586，玩家数据与统计正确保留至 `_NWC_preserved_source/`
- **网易基岩解密**：合成 80 1D 30 01 加密 LevelDB → 密钥恢复与合成密钥完全一致（footer 校验 1/1），3 个加密文件全部解密并通过自检，随后正确调度 b2j
- **异常路径**：0 字节空区域文件（原版存档常见占位）按空区域跳过；畸形 NBT / 扇区重叠 / 区域越界均被验证器拒绝并导出错误报告

## 许可证

GPL-3.0-or-later。Chunker（MIT）、je2be-core（GPL-3.0）。
源码：本仓库（含 `ANALYSIS.md` 逆向报告与 GPL 源码义务对应关系）。
