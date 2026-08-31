# runtime 目录（构建时由脚本填充，勿提交实际文件）

本目录用于放置随包分发的 jlink Java 运行时镜像（供 Chunker 子进程使用）。
不放置时，程序会回退使用系统 `java`（PATH 或 JAVA_HOME）。

生成方式（任选一个 JDK 17+，建议与 Chunker CLI 兼容的版本）：

```
jlink --add-modules java.base,java.desktop,java.logging,java.management,java.naming,java.sql,java.xml,jdk.unsupported,jdk.crypto.ec \
      --strip-native-commands=false --no-header-files --no-man-pages --compress=zip-9 \
      --output src-tauri/runtime
```

更精确的模块列表可用 `jdeps --print-module-deps chunker-cli.jar` 推导。
Windows 可直接复制原便携包的 `runtime/` 目录（jpackage 产物，已含 bin/java.exe）。
