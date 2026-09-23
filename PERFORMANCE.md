# 转换耗时复测

以下命令在 Windows x64 PowerShell 中运行。使用同一份输入存档和相同的目标版本；测速时不要同时运行构建或其他转换。b2j 测速的结果目录必须不存在，脚本会保留每次的后端输出和校验日志。

若本机没有更新签名私钥，可用 `npm run build -- --config scripts/tauri-unsigned-local.json` 验证 MSI 和 NSIS 构建；这个配置关闭更新产物生成。

## 应用端到端

先构建应用、准备 `src-tauri/backends` 和 `src-tauri/runtime`，以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` 启动应用。随后运行：

```powershell
node scripts/profile-conversion.mjs 9223 <输入.zip> 'Java 1.21.10' <输出.zip> <报告.json>
```

报告分别记录分析、密钥恢复、Bedrock 准备、b2j、验证和 ZIP 打包时间，并校验源 ZIP 的 SHA-256 未变化。用 `python -m zipfile -t <输出.zip>` 检查 ZIP CRC；应用转换过程会验证 `level.dat`、区域结构和区块数量。

## b2j 交错测试

先将存档解压并解密为固定的 Bedrock 输入，再编译校验工具：

```powershell
cargo run --release --manifest-path src-tauri/Cargo.toml --example conversion-bench -- prepare <输入.zip> <新的工作目录>
cargo build --release --manifest-path src-tauri/Cargo.toml --example conversion-bench
node scripts/bench-b2j.mjs <工作目录>/bedrock vendor/backends-windows/b2j.exe - src-tauri/target/release/examples/conversion-bench.exe <新的结果目录> 4
```

`-` 表示仅比较原版后端的 8 与 12 线程；提供候选 `b2j.exe` 路径时，脚本交错运行原版与候选版，每种线程数各四次。每次都检查 b2j 转换区块数、Terraform 区块数和应用验证器的区域记录数。`summary.json` 给出各组中位数，并比较两种二进制各自最快的线程设置。只有候选版的最佳中位数至少快 10% 且每次输出通过校验，才能考虑替换发布包中的二进制。线程数也应以这份结果决定。

脚本保留每次约数百 MB 的世界输出，方便逐次核查；请为结果目录预留足够空间。

## 上游源码构建

`scripts/build-b2j-windows.ps1` 固定拉取 je2be-core `web-4.3.0`（本次解析到 `b3facb832c39222d3d6f9bcb5192e4c0b1bd0691`），采用稀疏检出避开 Windows 路径过长的测试数据，应用 `scripts/patches/je2be-web-4.3.0-msvc-build.patch`，并在新的工作目录中构建 Windows x64 `b2j`。可通过 `-CandidatePatch` 追加性能补丁。脚本只生成候选文件，不替换 `vendor/backends-windows/b2j.exe`。

在本次测试机的 MSVC 19.44 上，`java-entity.cpp` 触发编译器内部错误 C1001。Visual Studio 生成器与 Ninja 生成器均复现，单独对该文件使用 `/Od` 仍失败。因此本轮未生成可校验的源码候选版，发布包保留原版 b2j。

## 本次实测结果

输入为提供的 141,159,498 字节存档，目标为 Java 1.21.10。下表的两个应用端到端数据各只有一次运行，文件缓存和运行顺序不同，因此只能作为耗时对照，不能将差值归因于代码改动。旧数据来自先前的 `profile-summary.json`，其中部分分段为近似值。

| 阶段 | 先前单次（秒） | 本次单次（秒） |
| --- | ---: | ---: |
| 分析 | 6.015 | 4.282 |
| 密钥恢复 | 未单独记录 | 0.015 |
| Bedrock 准备 | 3.226 | 0.328 |
| b2j | 465.014 | 336.675 |
| 验证 | 约 5.803 | 1.423 |
| ZIP 打包 | 约 5.058 | 1.778 |
| 转换总计 | 480.340 | 340.211 |

原版 b2j 在同一个预备 Bedrock 世界上交错运行四轮，每次都转换和 Terraform 25,057 个区块；应用验证器确认 190 个 `.mca`、50,382 条区域记录、`Version.Name=1.21.10`、`DataVersion=4556`。

| 线程 | 四次耗时（秒，运行顺序） | 中位数（秒） |
| --- | --- | ---: |
| 8 | 248.272、245.973、243.645、171.726 | 244.809 |
| 12 | 208.939、214.458、178.631、153.774 | 193.785 |

在这份存档上，12 线程中位数比 8 线程短 20.84%；应用原本就使用本机的 12 个可用线程，因此没有调整默认值。后几次运行明显变快，结果只适用于这台机器与该存档。端到端输出的 ZIP CRC、190 个 `.mca`、`level.dat`、目标版本及 DataVersion 均已校验，原存档 SHA-256 在转换前后相同。
