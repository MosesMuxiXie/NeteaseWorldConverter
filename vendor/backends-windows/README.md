# vendor/backends-windows — 预编译 Windows 后端（b2j）

本目录存放经过端到端实测的 Windows x64 b2j 及 LLVM 运行库，供 CI 与本地构建直接复用：

- `b2j.exe` — je2be-core 的 Bedrock→Java 命令行（来源：NeteaseWorldConverter 1.0.0 原版便携包 `app/native/b2j.exe`）
- `libc++.dll` / `libunwind.dll` — b2j 的 LLVM 运行时依赖（同原包）

## 为什么不从源码构建

GitHub windows-latest runner 上的 MSVC 19.51 编译 je2be-core（tag `web-4.3.0`）时会稳定崩溃
（cl.exe 0xC0000005 / D8040），clang-cl 则无法通过其源码；macOS CI 上的源码构建正常
（见 `scripts/build-b2j-macos.sh`），故 Windows 侧采用预编译产物以保证发布可复现。

## GPL-3.0 合规

- 上游项目：https://github.com/kbinani/je2be-core（GPL-3.0）
- 对应源码 tag：`web-4.3.0`（`https://github.com/kbinani/je2be-core/tree/web-4.3.0`）
- 许可证全文：`src-tauri/backends/../licenses/` 或上游仓库 `LICENSE` 文件
- 二进制未做任何修改，直接取自原版便携包

## Windows arm64 说明

je2be-core 未发布 Windows arm64 二进制；arm64 安装包携带本 x64 版本，
在 Windows 11 ARM 上经系统 x64 模拟层运行（其余部分——应用本体与 Java 运行时——均为原生 arm64）。
