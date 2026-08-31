# NeteaseWorldConverter 逆向分析报告

分析对象：`NeteaseWorldConverter.exe`（jpackage 便携包，Windows x64，1.0.0 版）
分析方式：包内自带源码压缩包（GPL-3.0 要求提供对应源代码），全部源码逐文件审计 + 后端二进制哈希比对。

## 一、程序身份

| 项目 | 内容 |
|---|---|
| 名称 | 网易 Minecraft 存档转换器 (Netease World Converter) |
| 版本 | 1.0.0 |
| 技术 | Java 17 字节码（`--release 17` 编译）+ Swing GUI，jpackage 以 JDK 26.0.2 打包，自带 76MB jlink 运行时 |
| 许可 | GPL-3.0-or-later（第三方：Chunker MIT / je2be-core GPL-3.0 / NeteaseMcDencrypter GPL-3.0 参考） |
| 体积 | EXE 启动器 512KB + app 43MB + runtime 76MB |

## 二、目录结构（jpackage app-image 布局）

```
NeteaseWorldConverter/
├── NeteaseWorldConverter.exe        # jpackage 启动器，读取 .cfg 后拉起自带 JRE
├── app/
│   ├── NeteaseWorldConverter.jar    # 主程序（com.openai.nwc.App，12 个类，约 2730 行）
│   ├── chunker-cli.jar              # HiveGamesOSS/Chunker 1.19.1 CLI（Java 跨版本转换后端）
│   ├── native/b2j.exe               # kbinani/je2be-core 的 b2j 命令行（基岩→Java C++ 后端）
│   ├── native/libc++.dll/libunwind.dll  # LLVM 运行时（b2j 依赖）
│   ├── NeteaseWorldConverter-1.0.0-source.zip  # GPL 义务：随包源码
│   ├── .jpackage.xml / *.cfg        # jpackage 状态文件
│   └── licenses/ + THIRD-PARTY-NOTICES.txt
└── runtime/                         # jlink JRE（含被补回的 bin/java.exe，供 Chunker 子进程用）
```

## 三、模块逐一分析（Java 源码 → 职责）

### 1. App.java（619 行）— Swing UI 层
- 窗口 920×720，最小 820×650。界面元素：存档 ZIP 选择行（支持拖放 TransferHandler）、识别结果、目标 Java 下拉框、降级警告行、进度条（0-100 带阶段文字）、等宽日志区（JSplitPane 分隔）、按钮组（开始转换/下载保存/打开错误报告）。
- 状态机：`analyzeZip()` → SwingWorker 后台解析；`startConversion()` → 降级需确认对话框；转换中"开始转换"按钮变"取消转换"；关闭窗口时若在转换中需确认并取消。
- 降级确认文案与 `_NWC_preserved_source` 保留策略在 UI 层提示。
- 大跨度降级（目标 ≤1.16.x）警告变红色。

### 2. ConversionEngine.java（208 行）— 编排核心
流程 `analyze → targets → convert → validate → zip`：
1. **analyze**：校验扩展名 `.zip/.mcworld` → 建临时目录 `NeteaseWorldConverter-*` → 写 conversion.log → 计算 SHA-256 → 安全解压（进度 1→12）→ WorldDetector 识别（进度 12）。
2. **targets**：向 Backends 询问可用目标版本。
3. **convert**：
   - 基岩类：`NeteaseBedrockDecryptor.prepare()` 解密/规范化（13→30）→ `Backends.runJe2be()` 基岩→Java 1.21.10（31→64）→ 保留 datapacks/resources.zip/icon.png。
   - 目标就是 1.21.10 且输入为基岩：直接把 JE2BE 输出当作结果。
   - Java 输入且版本相同：逐文件保真复制（32→84）。
   - 其余：`Backends.runChunker()` 跨版本转换（64→84）→ `EntityPreserver` 处理实体/POI/玩家（升级原样迁移交 DataFixer；降级放入 `_NWC_preserved_source`）。
   - `JavaWorldValidator.validate()`（85→94）→ 写转换报告 → `ArchiveTools.createZip()`（94→100）。
4. **降级判断** `isDowngrade`：正则提取 `(1|26).\d+(.\d+)?` 版本号做三段比较。
5. **exportErrorReport**：把 conversion.log 复制到输入 ZIP 旁边（不可写则回退桌面），文件名 `<base>-error-<yyyyMMdd-HHmmss>.log`。

### 3. ArchiveTools.java（246 行）— 安全压缩/解压
- **extractZip** 防御性限制：最多 100 万条目、解压总量 256 GiB（先扫描声明大小，再在写出时二次累计）；路径清洗（`\`→`/`、拒绝绝对路径/`..` 跳转、拒绝重复路径=Zip Slip 同类攻击）；逐条写出 1MB 缓冲，进度 2→10。
- **createZip**：Deflater.BEST_SPEED（等级1），条目名带 `<folder>/` 前缀，保留 mtime，进度 94→100。
- 另有 copyTree / deleteTree / sha256 / hex / safeFolderName（过滤 Windows 非法字符与尾部点空格）。

### 4. WorldDetector.java（222 行）— 存档类型识别
- 深度 ≤10 遍历目录，忽略 `__MACOSX/.git/_conversion`。
- **Java 候选**：目录含 `level.dat` → 基础分 20，5 层内有 `.mca` +50，有 `region/` 或 `dimensions/` +15，再减去深度（浅者优先）。
- **基岩候选**：目录名匹配 `db[_ -]*数字?` 且含 LevelDB 文件（`.ldb/.log/MANIFEST-*/CURRENT`）→ 世界根=其父目录，分 100−深度。
- **加密判定**：优先选择带网易头的基岩候选 —— 读 db 目录内所有文件 + 世界根 MANIFEST-* 的前 4 字节：
  - `80 1D 30 01` → 网易基岩新版（循环 XOR，可解）
  - `90 1D 30 01` → 网易基岩旧版 AES-CFB8（离线不可解，UI 禁用转换并导出报告）
  - 都没有 → 标准基岩版
- Java 类型解析 level.dat（NBT）取 LevelName/Version.Name/DataVersion；文件名含 "netease"（深度2内）则备注网易 Java 标记，否则注明"Java 存档无统一网易签名"。

### 5. NeteaseBedrockDecryptor.java（399 行）— 网易解密（本程序的核心价值）
- LevelDB table footer 魔数（明文末 8 字节）：`57 FB 80 8B 24 75 47 DB`。
- **prepare**：复制世界元数据（跳过 db 目录、level.dat*、根目录 LevelDB 残留）；level.dat 优先根目录、次之 db 目录；收集 db 目录 + 根目录散落的 `.ldb/.log/MANIFEST-*/CURRENT/LOCK`；写回规范化 CURRENT（选 CURRENT 明文指向的或字典序最大 MANIFEST）与空 LOCK。
- **密钥恢复**（8 字节循环 XOR）：
  - CURRENT 已知明文攻击：CURRENT 明文必为 `MANIFEST-xxxxx\n`，密文去头后与其异或得密钥字节序列，取最短周期（≤32）。
  - 每个 `.ldb` footer 攻击：明文长度 = 文件长−4，明文末 8 字节应为 footer 魔数 → 异或出 8 字节密钥（注意环形相位 `(plainOffset+i) % 8`）。
  - 全部候选逐一验证"能解出正确 footer 的加密 LDB 数"，取最优；0 命中即失败；部分命中告警。
- **解密**：每文件去掉 4 字节头后按 `key[(pos)%8]` 循环异或（1MB 缓冲）；未加密文件直接复制。
- **自检**：解密后所有 `.ldb` 末 8 字节必须是明文 footer，且库内无残留加密头，否则中止。

### 6. Backends.java（281 行）— 外部后端调度
- 定位自身：jar 位置 / user.dir / `jpackage.app-path` 的 exe 目录及其 `app/` 子目录。
- **listTargetVersions**：跑 `java -jar chunker-cli.jar -f ?`，正则 `JAVA_(?:26|1)_\d+(?:_\d+){0,2}` 抽取并过滤（26.x ≤2；1.12–1.21），按版本号降序；失败回退内置 60+ 版本清单（26.2 → 1.12）。
- **runJe2be**：`b2j -i <bedrock> -o <java> -n <线程>`，线程数 2–16 取 CPU 数；工作目录与 PATH 前置都指向 native/（找 libc++.dll）；进度 31→64 脉冲式（每行 +1，到顶回卷）；完成必须产出 level.dat。
- **runChunker**：`java -Xms512m -Xmx{内存}G -jar chunker-cli.jar -i <in> -o <out> -f <JAVA_x_y_z>`，内存 = 总内存×70% 夹在 2–12 GiB；进度 64→84 按行内百分比换算；完成必须产出 level.dat。
- **runProcess 公共**：独立线程逐行读 stdout+stderr 合并流写日志；每 250ms 轮询取消标志，取消时 destroy，3 秒不退则 destroyForcibly。

### 7. EntityPreserver.java（181 行）— 实体/POI/玩家保留策略
- 三维度（overworld/the_nether/the_end）× 两类（entities/poi）：
  - 新式路径优先 `dimensions/minecraft/<dim>/<kind>`，旧式回退 `world|DIM-1|DIM1/<kind>`。
- **升级**（源版本≤目标）：删除输出对应目录后整树复制 + 复制 playerdata/advancements/stats；NBT 由目标版本 DataFixer 首次加载时升级。
- **降级**：所有上述文件复制到输出 `_NWC_preserved_source/` 原目录结构下，不静默丢弃，并写入备注。

### 8. JavaWorldValidator.java（161 行）— Anvil 结构验证
- level.dat 必须存在且可解析；至少一个 `.mca`。
- 每个 `.mca`：大小 ≥8192 且为 4096 倍数；位置表 1024 项逐项校验 offset≥2、count>0、不越界；扇区不得重叠（集合判重，含 0/1 头部）；记录长度 length+4 ≤ count×4096；压缩字节低 7 位 ∈ {1 gzip, 2 zlib, 3 raw}，高位 0x80 表示外部 `.mcc`（按 region 坐标推算文件名，必须存在）；解压后必须能走完一棵合法 NBT Compound 树。
- `_NWC_preserved_source` 顶层目录不计入统计。

### 9. Nbt.java（186 行）— 极简 NBT 解析器
- 限制：嵌套 ≤512 层、集合 ≤128MiB、收集模式下 ByteArray ≤1MiB（超出跳过），Int/Long Array 只跳不存。
- `readJavaLevel`：GZIP 解包 → 根 Compound → `Data.LevelName` / `Data.Version.Name` / `Data.DataVersion`。
- `validateRoot`：只验结构不保留数据（跳过模式），防恶意超大 NBT。

### 10. AppLog.java（80 行）— 同步写文件 + UI 回调；Cli.java（64 行）— 无头 E2E 测试入口（analyze/convert 子命令）。

## 四、处理管线总览（进度映射）

```
选 ZIP → [1-12 解压+识别] → 基岩? [13-30 解密/规范化] → [31-64 JE2BE b2j → Java 1.21.10]
       → Java? [64-84 Chunker 跨版本（同版本则保真复制 32-84）] → [补充文件+实体策略]
       → [85-94 Anvil 逐区域验证] → [94-100 打包 ZIP] → 手动保存
```

## 五、安全设计要点（值得在移植中保留）

1. ZIP 沙箱：容量/条目上限、Zip Slip、绝对路径、重复条目全部拒绝。
2. 解密自证：恢复的密钥必须让所有加密 LDB 的 footer 校验通过才算成功；解密后再查残留加密头。
3. 输入只读：全程在临时目录操作，原 ZIP 永不修改。
4. 验证失败不给出成功状态，也不提供下载按钮。
5. 降级不伪造数据：不可转换内容进 `_NWC_preserved_source` 而不是丢弃。

## 六、Tauri 移植方案（本仓库的实现）

| 原实现 | Tauri 移植 |
|---|---|
| Swing 窗口 | WebView 前端（无框架 HTML/CSS/JS，还原全部交互与状态机） |
| ConversionEngine/ArchiveTools/WorldDetector/Decryptor/EntityPreserver/Validator/Nbt | Rust 逐模块移植（行为一一对应，含全部安全限制与错误文案） |
| Chunker（需 JVM） | 仍作为子进程；JVM 优先用打包的 `runtime/`（jlink 镜像），回退系统 java |
| b2j.exe | Windows 沿用原 b2j.exe；macOS 用 je2be-core 的 CMake `b2j` 目标自行构建（上游本身支持 macOS），路径与工作目录逻辑跨平台化（PATH 分隔符、b2j 无 .exe 后缀） |
| jpackage 打包 | Tauri bundler：Windows NSIS 安装器/便携，macOS .app + .dmg |

进度、取消、错误报告、降级确认等行为与原版保持一致，事件通过 Tauri `nwc://progress` / `nwc://log` 推送到前端。
