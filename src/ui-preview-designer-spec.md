# 网易 Minecraft 存档转换器 — 日和樱色 UI 设计规格（用于工程落地）

> 配套文件：`src/ui-preview.html`（自包含预览，浏览器直接打开即可查看全部状态）
> 本规格面向工程师：按第 2—7 节的 token 与规范替换 `src/style.css`，选择器保持 `src/index.html` 不变即可 1:1 落地。

---

## 1. 视觉方向

「少女心的游戏启动器」：**日和樱色 × 奶油底 × 丁香紫**。
樱粉（主行为色）负责少女感，奶油白负责工具界面的干净，淡紫丁香只作点缀；所有卡片圆角化、软粉阴影、半透明浅色描边，标题使用本地衬线字体。
**铁律：** 装饰永远不遮挡内容 —— 装饰层在内容 z 轴之下、透明度 ≤0.55、不动用 Emoji、不出现深蓝黑底 + 荧光青这类非本风格的配色；日志区对比度 ≥4.5:1（实测 11.8:1）。

---

## 2. 色板（直接抄进 `:root`）

| 变量名 | Hex | 用途 |
|---|---|---|
| `--primary` | `#C24373` | 主色（樱玫）：主按钮、强调文字、进度条深端 |
| `--primary-hover` | `#AE3560` | 主按钮 hover |
| `--sakura` | `#F2A6C4` | 亮樱粉：装饰花瓣、进度条亮端、拖放遮罩底 |
| `--sakura-mid` | `#E8799F` | 樱花折中：虚线圈、标题樱花徽章 |
| `--sakura-light` | `#FBE3EE` | 最浅樱：焦点环、徽章花芯 |
| `--lavender` | `#B49BE3` | 丁香紫：星光/隐私小花等点缀 |
| `--lavender-soft` | `#EFE8FA` | 淡紫底（备用 chip 底） |
| `--cream` | `#FFFDF9` | 页面/窗口底 |
| `--cream-panel` | `#FFF8F1` | 卡片底、日志标题栏 |
| `--cream-track` | `#F7E6DF` | 进度条轨道（奶油白底） |
| `--success` | `#3F7355` | 成功：✓ 识别结果、完成阶段、保存按钮 |
| `--success-hover` | `#3A6E4B` | 成功按钮 hover |
| `--warning` | `#95581A` | 普通降级警告 |
| `--error` | `#B03A4A` | 错误：识别失败、严重警告、失败弹窗标题 |
| `--ink` | `#4A3A44` | 正文文字（暖墨色） |
| `--muted` | `#7D6476` | 次要文字/说明 |
| `--stage-ink` | `#5C4A55` | 阶段行默认文字 |
| `--line` | `#EFD9E2` | 描边（输入框/日志面板） |
| `--line-soft` | `#F6E7EE` | 卡片描边（半透明白边效果用 `inset 0 1px 0 rgba(255,255,255,0.9)` 叠加） |
| `--log-bg` | `#232848` | 日志区深墨蓝（顶部） |
| `--log-bg-deep` | `#1D2238` | 日志区深墨蓝（底部渐变） |
| `--log-text` | `#F9DCE9` | 日志正文（浅粉字） |
| `--log-err` | `#FFC9D2` | 日志 ERROR 行 |
| `--log-warn` | `#FFDFAE` | 日志 WARN 行 |
| `--log-ok` | `#BFE3C9` | 日志关键完成行（如 25057 chunks converted） |
| `--log-dim` | `#A9A0C8` | 日志时间戳（备用，预览未逐时间戳着色） |

**对比度核验（白/奶油底上）**：`--primary` 白字 4.67:1 ✓ · `--success` 奶油底小字 5.26:1 / 白字 5.53:1 ✓ · `--warning` 奶油底小字 5.41:1 ✓ · `--error` 奶油底小字 5.62:1 ✓ · `--muted` 5.0:1 ✓ · 日志 `--log-text` 在 `--log-bg` 上 11.8:1 ✓

---

## 3. 字体栈（全部本地字体，零外部依赖）

| 用途 | 字体栈 | 说明 |
|---|---|---|
| 标题 / 弹窗标题 / 拖放框 | `"Noto Serif SC", "Source Han Serif SC", "Songti SC", "STZhongsong", "SimSun", serif` | 衬线感；`--font-title` |
| 正文 / 按钮 / 输入 | `"Segoe UI", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif` | `--font-body` |
| 日志 / 等宽 | `Consolas, "Cascadia Mono", "Courier New", monospace` | `--font-mono`，12px / 行高 1.6 |

不引用 Google Fonts / 外部 CDN；中文衬线靠系统已装的 Noto/Songti 命中，兜底 SimSun 仍可用。

---

## 4. 圆角 / 阴影 / 间距（一律 4/8 步长）

| Token | 值 | 用于 |
|---|---|---|
| `--radius-card` | `18px` | 卡片、拖放框、弹窗 |
| `--radius-field` | `12px` | 输入框、下拉框 |
| `--radius-pill` | `999px` | 按钮、进度条、指标 chip |
| 应用窗口 | `22px` | 仅预览外框，真实 UI 由系统窗框替代 |

阴影（粉调，不用灰黑）：

```css
--shadow-card:  0 6px 24px rgba(194,67,115,.10), 0 1px 3px rgba(122,84,105,.08);
--shadow-modal: 0 18px 50px rgba(80,40,66,.28);
--shadow-window:0 24px 60px rgba(150,90,120,.18), 0 4px 14px rgba(150,90,120,.10);
```

间距：`8 / 10 / 12 / 14 / 18 / 26`（卡片内 14×18，行距 10，区块 gap 14，根 padding 20/26/18），全部可整除 2，关键值在 4/8 系列内。

---

## 5. 按钮分级

| 级别 | 类 | 外观 | 状态 |
|---|---|---|---|
| 主操作 | `.primary`（`#start-btn`、`#choose-btn`、弹窗确定） | 渐变 `linear-gradient(135deg,#C64578,#AE3560)` + 白字 + 粉投影 `0 4px 14px rgba(194,67,115,.32)` | hover 加深 `#B93A6A→#9E2F55`；disabled `opacity:.45` |
| 成功 | `.success`（`#save-btn`） | 实色 `#3F7355` + 白字 + 绿投影 | hover `#3A6E4B` |
| 普通 | `.plain`（`#report-btn`、弹窗取消） | 奶油樱底 `#F6E4EC` + 深玫字 `#6E4154` | hover `#EFD3DF` |

公共：pill 圆角、`font-weight:600`、`padding:8px 18px`、`font-size:13.5px`；active 微降 1px；焦点可见环 `outline:2px solid rgba(198,69,120,.55)`（focus-visible）。

---

## 6. 进度条与资源指标

- 轨道：`--cream-track` 奶油白底，高 14px，pill 圆角，内阴影 `inset 0 1px 2px rgba(122,84,105,.07)`。
- 填充：`linear-gradient(90deg,#F7B3CE 0%,#E8739F 60%,#C64578 100%)` + 粉投影。
- **运行中**（`.progress-fill.active`）：叠加 `repeating-linear-gradient(120deg, rgba(255,255,255,.28) 0 10px, transparent 10px 20px)`，`background-size:34px 100%,auto`，动画 `background-position: 34px 0, 0 0` 0.9s 无限循环。
- 阶段文字 `.stage`：13px，默认 `--stage-ink`；`.ok/.error/.warn` 分别用 success/error/warning 色。
- 资源指标：`#metric-*` 为 pill chip（奶油底 + 浅粉描边），等宽数字；`.resource-metrics.running` 时活动点跳 `--success` 呼吸动画，文案 `已运行 mm:ss / CPU nn% / 内存 x.xx GiB`。
- 动画必须尊重 `prefers-reduced-motion: reduce`（预览已实现）。

---

## 7. 日志面板「记忆里的屏幕」

- 标题栏：`--cream-panel` 底 + `--line-soft` 下边线，13px 600。
- 内容区：**深墨蓝渐变** `linear-gradient(180deg,#232848,#1D2238)` + **浅粉字** `#F9DCE9`（对比度 11.8:1）；12px 等宽，行高 1.6，`pre-wrap / break-all`，顶部自动滚动。
- 行着色（预览内用 span 实现，真实 UI 如需同样效果可仿照追加 span 渲染；至少保留深浅两色体系）：
  - ERROR 行：`#FFC9D2` + 600
  - WARN 行：`#FFDFAE`
  - 完成行（如 `25057 chunks converted`）：`#BFE3C9`
  - 其余：`#F9DCE9`
- 日志上限 1500 行（main.js `MAX_LOG_LINES`），滚动到底部。

---

## 8. 装饰元素（克制原则：6 瓣 + 2 云 + 2 星）

全部内联 SVG（无外链、无图片文件）：

| 元素 | 规格 | 位置 |
|---|---|---|
| 樱花瓣 `.petal ×6` | 泪滴形 `M12 1 C16.5 5 18.5 9.5 12 16.5 C5.5 9.5 7.5 5 12 1 Z`，`#F2A6C4/#F8C6DA` 交替，13–19px，opacity ≤0.5，慢速飘落 17–24s 循环（`translate3d + rotate`，`transform-box:fill-box`） | 窗口内容后面，z-index:0，`pointer-events:none` |
| 云朵 `.deco-cloud ×2` | 白色团状 blob path，宽度 250/300px，opacity 0.45–0.55 | 右上角、左下角 |
| 星光 `.sparkle ×2` | 四角星 `M10 1 C10.8 6.2 12.8 8.2 18 9 C…`，`#C8B6F0/#F2A6C4`，22/14px，opacity ≤0.75 | 标题右侧、右下角 |
| 樱花徽章 `.sakura-mark` | 五瓣花（5×petal path rotate72°+ 花芯圆），20px，`#E8799F` | 标题 h1 前、拖放框内 |
| 小花 `.mini-flower` | 同五瓣花 13px，`#B49BE3` | 底部隐私文案前 |

禁止：Emoji、GIF、外链图片、装饰覆盖文本、完全不透明的大面积装饰。

---

## 9. 状态与行为映射（预览 ↔ 真实 main.js）

| 预览状态（演示按钮） | 对应真实流程 | 关键模拟值 |
|---|---|---|
| 空闲态 | 初始 UI | 检测行 idle「尚未选择存档」、目标禁用、进度 0%、按钮全禁用（选择按钮除外） |
| 识别成功 | `analyze()` → `result.supported` 分支 | `✓ 网易基岩版（可解密）· 基岩 LevelDB`、进度 12%、阶段「解析成功」、目标列表填充并启用、`#start-btn` 启用、详情 `世界：… 文件：1284 大小：84.32 MiB` |
| 转换中 | `startConversion()` | 进度 57%、阶段「正在转换到 Java 1.20.4 · 写入区域 r.1.-1.mca（剩余 43%）」、`#start-btn` 变「取消转换」、输入/目标/拖放禁用、指标运行（已运行 00:34 · CPU 62% · 内存 1.28 GiB）、日志滚到 `25057 chunks converted` |
| 失败弹窗 | `startConversion()` catch / `handleAnalysisFailure()` | 阶段 error、进度文字「转换失败」、`#report-btn` 启用、弹窗「转换失败。原始 ZIP 没有被修改。…」仅「确定」 |
| 降级警告 | 选择 1.12–1.16 目标 + `refreshDowngradeWarning()` | severe 警告「⚠ 大跨度降级：…请务必保留原 ZIP。」+ 确认降级弹窗（继续/取消） |
| 拖放遮罩 | 拖拽 enter/over | 全窗樱粉遮罩 + 虚线框「松开以选择存档 ZIP」 |

---

## 10. 选择器映射表（真实 UI → 预览样式锚 → 落地要点）

| 真实选择器 | 预览中的样式锚（可直接照抄） | 落地要点 |
|---|---|---|
| `#input-field` | `#input-field` | 12px 圆角/奶油底 `#FFFDFB`/`--line` 描边/13px muted 字 |
| `#choose-btn` | `button.primary` + `#choose-btn`（类选择器即可） | 渐变主按钮 |
| `#detection-label` | `.detection` / `.detection.idle` · `.detection.ok` · `.detection.error` | ok=`--success`、error=`--error`、idle=`--muted`；ok/error 600 字重 |
| `#details-label` | `.details` | muted 12.5px |
| `#target-box` | `select` | 同输入框描边/圆角，启用后 ink 字色 |
| `#warning-label` | `.warning` / `.warning.severe` | 普通=琥珀，severe=玫红+600；12.5px |
| `#stage-label` | `.stage` / `.stage.ok.error.warn` | 5 色阶段态 |
| `#progress-bar` | `.progress-fill` / `.progress-fill.active` | 樱粉渐变 + 条纹流动 |
| `.progress-track` | `.progress-track` | 奶油底 14px 圆角 |
| `#progress-text` | `.progress-text` | tabular-nums muted 13px |
| `#metric-elapsed` / `#metric-cpu` / `#metric-memory` | `.metric` | pill chip |
| `.resource-metrics.running` | `.resource-metrics.running` | 活动点 success 呼吸 |
| `#log-area` | `#log-area` | 深墨蓝渐变 + 浅粉字 11.8:1，行着色 span 可选项 |
| `#start-btn` | `button.primary` | 转换中变「取消转换」 |
| `#save-btn` | `button.success` | 抹茶绿 |
| `#report-btn` | `button.plain` | 奶油樱底 |
| `#backend-note` | `.privacy` | 12px muted + 前排小花 svg；后端缺失时沿用 error 色应急 |
| `#drop-overlay` | `.drop-overlay` | `rgba(242,166,196,.18)` 半透明全窗 + z-index:40 |
| `.drop-box` | `.drop-box` | 2px 虚橙 `--sakura-mid`、18px 圆角、衬线标题字 |
| `#modal-backdrop` | `.modal-backdrop` | `rgba(74,46,66,.38)` + z-index:50 |
| `#modal-title` | `.modal-title` | 衬线 16.5px 700 |
| `#modal-body` | `.modal-body` | 13.5px / 行高 1.7 / 可选中 |
| `#modal-ok` | `button.primary` | 弹窗确认 |
| `#modal-cancel` | `button.plain` | 无取消时加 `.hidden`（预览演示已实现） |
| `.hidden` | `.hidden` | `display:none !important`（不变） |
| `.root/.header/.subtitle/.card/.config/.row/.actions/.spacer/.privacy/.log-panel/.log-title` | 同名类 | 布局沿用原 grid/flex 结构，仅换装饰层 |

---

## 11. 落地清单（工程侧操作步骤）

1. 将 `ui-preview.html` 的 `:root` 变量块 + 各部分样式规则复制进 `src/style.css`，**删除预览专用块**：`.demo-bar / .demo-btn / .demo-label / .demo-note / .app-window / .backdrop / .petal / .deco-cloud / .sparkle / .sakura-mark / .mini-flower / .log-area span 着色（除非决定做 span 渲染）`。
2. `src/index.html` 结构**无需改动**（id/类名已一致）；如需标题樱花徽章与隐私小花：把预览里对应内联 SVG 就地插入即可（它们是 HTML 元素而非 CSS 背景）。
3. 遮罩定位：预览内 `.drop-overlay`/`.modal-backdrop` 用 `position:absolute`（相对 `.app-window`）；真实 UI 里它们仍是 `position:fixed inset:0`（原 style.css 写法），两种都正确。
4. 拖放与模态框行为、日志上限、阶段/检测文案逻辑全部保持在 `src/main.js` 不变。
5. 无障碍：保留 `role="progressbar" + aria-valuenow`；强烈建议把 `prefers-reduced-motion` 块一并复制。
6. 验证点：按钮四态（默认/hover/disabled/active）对比度；日志区黑白两色在深墨蓝上的对比度 ≥4.5:1；窗口 1000×800 不换行溢出（root padding 20/26/18 布局）。

---

## 12. 窗口尺寸

真实 Tauri 窗口默认 **1000×800**。预览 `.app-window` 复刻该比例（`min(1000px,100%)` × `min(800px, 视口-118px)`），浏览器窗口更大时四周是页面氛围底色，应用本体仍是工具窗口的观感。
