# backends 目录（构建时由脚本填充，勿提交实际文件）

`npm run prepare:win` / `npm run prepare:unix` 会把以下文件放入本目录：

- `chunker-cli.jar` — Chunker CLI（HiveGamesOSS/Chunker，MIT）。原包使用 1.19.1。
  下载地址：<https://github.com/HiveGamesOSS/Chunker/releases/tag/1.19.1>
- `b2j.exe`（Windows）/ `b2j`（macOS/Linux）— je2be-core 的 Bedrock→Java 命令行（GPL-3.0）。
  - Windows x64：复用原便携包 `app/native/b2j.exe`（连同 `libc++.dll`、`libunwind.dll`）
  - Windows arm64：复用 x64 版 b2j（Windows 11 ARM 提供 x64 模拟层；je2be-core 暂未发布 Windows arm64 二进制）
  - macOS：`bash scripts/build-b2j-macos.sh` 从源码 CMake 构建（Intel / Apple Silicon 均为本机构建）
- `runtime/` — jlink Java 运行时（Chunker 子进程使用，同目录 `../runtime`）。生成方式见 `src-tauri/runtime/README.md`。

CI（`.github/workflows/release.yml`）在 macOS 运行器上自动执行上述步骤后调用 `tauri build --target <arch>`。
