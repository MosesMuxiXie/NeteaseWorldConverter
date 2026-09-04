# NeteaseWorldConverter-Tauri

网易 Minecraft 存档转换器（`NeteaseWorldConverter.exe`，Java 17 + Swing，GPL-3.0）的 **Tauri 2 跨平台移植**。

仓库：<https://github.com/MosesMuxiXie/NeteaseWorldConverter>（发布包见 Releases）

输入网易 Minecraft 存档的 `.zip` / `.mcworld` 文件后，程序在本机完成：

1. **安全解压**（条目数 / 解压体积上限、Zip Slip、绝对路径、重复路径全部拒绝，原 ZIP 永不修改）
2. **自动识别**：Java Anvil / 标准基岩 LevelDB / 网易基岩新版（`80 1D 30 01` 循环 XOR，可离线恢复密钥）/ 网易基岩旧版（`90 1D 30 01` AES-CFB8，不可离线解密）
3. **转换**：
   - 网易基岩 → 恢复密钥解密 LevelDB → 规范化 → `b2j`（je2be-core）→ Java 1.21.10
   - Java → `Chunker` 跨版本转换（同版本逐文件保真复制）
   - 降级时不可映射的实体 / POI / 玩家数据保留到 `_NWC_preserved_source/`，绝不静默丢弃
4. **逐区域 Anvil 结构验证**（位置表、扇区重叠、压缩类型、外部 `.mcc`、NBT 树）
5. **打包输出 ZIP**，手动选择保存位置

## 与原版的对应关系

| 原版（Java） | 移植（Rust） |
|---|---|
| App.java（Swing UI） | `src/`（WebView 前端，无框架） |
| ConversionEngine.java | `src-tauri/src/engine.rs` |
| ArchiveTools.java | `src-tauri/src/archive.rs` |
| WorldDetector.java | `src-tauri/src/detect.rs` |
| NeteaseBedrockDecryptor.java | `src-tauri/src/decrypt.rs` |
| Backends.java | `src-tauri/src/backends.rs` |
| EntityPreserver.java | `src-tauri/src/entity.rs` |
| JavaWorldValidator.java | `src-tauri/src/validate.rs` |
| Nbt.java | `src-tauri/src/nbt.rs` |
| AppLog.java | `src-tauri/src/log.rs` |
| jpackage 打包 | Tauri bundler（NSIS / MSI / macOS .app+.dmg） |

逆向分析报告见 [`ANALYSIS.md`](ANALYSIS.md)。

## 环境要求

- Rust stable（MSVC 工具链 + VS Build Tools）
- Node.js 20+（仅构建期使用 `@tauri-apps/cli`）
- macOS 构建需自行从 [je2be-core](https://github.com/kbinani/je2be-core) 用 CMake 构建 `b2j` 目标（见 `src-tauri/backends/README.md`）

## 构建

```powershell
# 1. 安装依赖
npm install

# 2. 准备后端资源（复制 chunker-cli.jar / b2j.exe / LLVM DLL / jlink runtime）
npm run prepare:win      # Windows；来源默认是同级目录的原版便携包 NeteaseWorldConverter
npm run prepare:unix     # macOS/Linux

# 3. 开发调试
npm run dev

# 4. 发布打包（产物在 src-tauri/target/release/bundle/）
npm run build
```

`backends/` 与 `runtime/` 内容不入库（见 `.gitignore`），构建前必须运行 prepare 脚本。
`runtime/` 缺失时程序会自动回退到系统 `java`（PATH 或 JAVA_HOME）。

## 发布产物

| 平台 | 形态 | 状态 |
|---|---|---|
| Windows x64 | NSIS 安装器 / MSI / 便携 ZIP | ✅ 已构建并实测 |
| Windows arm64 | NSIS 安装器（b2j x64 经 Win11 模拟运行） | CI 构建（`windows-arm64` 任务） |
| macOS x64（Intel） | .app + .dmg（b2j 本机构建） | CI 构建（`macos-x64` 任务，需 macOS 运行器） |
| macOS arm64（Apple Silicon） | .app + .dmg（b2j 本机构建） | CI 构建（`macos-arm64` 任务，需 macOS 运行器） |

**一键出全部四个平台**：推送到 GitHub 后，在 Actions → *Release builds* → Run workflow 手动触发（产物在各任务 Artifacts 下载）；打 `v*` tag 会自动跑并创建 GitHub Release。工作流见 `.github/workflows/release.yml`。

## 测试

- **单元测试**：`cargo test --manifest-path src-tauri\Cargo.toml`（解密密钥恢复、Anvil 验证器、版本比较、XOR 字块对拍、多命名空间实体保留、ZIP 嗅探、资源统计，13 项）。
- **端到端（推荐）**：用 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` 启动应用后：
  - `node scripts/make-test-world.mjs` — 生成最小 Java 1.21 测试世界 ZIP
  - `node scripts/devtools-e2e.mjs 9223 <输入.zip> <输出.zip> <目标版本>` — 走完整 analyze→convert→save 流程
  - `node scripts/devtools-analyze.mjs 9223 <输入.zip>` — 仅识别
  - `node scripts/backend-probe.mjs 9223` — 查询后端定位结果

## 许可

GPL-3.0-or-later（继承原版）。第三方：

- [Chunker](https://github.com/HiveGamesOSS/Chunker) — MIT（跨版本转换后端）
- [je2be-core](https://github.com/kbinani/je2be-core) — GPL-3.0（b2j：基岩→Java）
- 网易 LevelDB 解密逻辑参考 [NeteaseMcDencrypter](https://github.com/Sumandora/NeteaseMcDencrypter) — GPL-3.0
