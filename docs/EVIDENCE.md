# Eneye v0.2 验收证据

本文件记录 v0.2 阶段 0–5 的可复现证据、构建产物、测试门禁和仍然存在的限制。产品行为契约见 [PRODUCT_CONTRACT.md](PRODUCT_CONTRACT.md)，失败矩阵见 [FAILURE_MATRIX.md](FAILURE_MATRIX.md)。

## 1. 构建与安装

| 项目 | 值 |
| --- | --- |
| 安装包 | `D:\DevTools\Rust\targets\eneye\release\bundle\nsis\Eneye_0.1.0_x64-setup.exe` |
| 安装包大小 | 38,392,424 bytes（36.61 MiB） |
| 安装包 SHA256 | `91EF0246A6E2AF0B70ABB4416BA21012D5DD8A8B0E80D129C8B136A6ECD8B70C` |
| 安装位置 | `D:\Eneye` |
| 已安装 exe | `D:\Eneye\eneye.exe` |
| exe SHA256 | `0883332A671033B18D9889095E473C1AC5CF15F980F5B3BB055F2F00DFFA94B3` |
| 构建时间 | 2026-11-07 12:50 左右 |
| 运行进程 | pid 27432（测试期间为 31020） |

安装方式：NSIS 静默安装 `/S /D=D:\Eneye`，随后启动 `D:\Eneye\eneye.exe`。

## 2. 自动化门禁

| 门禁 | 结果 |
| --- | --- |
| `cargo test --lib` | 118 passed，1 ignored，0 failed |
| ignored 测试 | 需要真实 Edit HWND 的交互测试，已由第 3 节真实记事本证据覆盖 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo fmt --check` | 通过 |
| `npm run verify` | Prettier、tsc、44 个前端测试（10 个文件）、Vite build 全部通过 |
| `npm run tauri build` | 通过，生成上述 NSIS 安装包 |

## 3. P0：真实鼠标取词

### 3.1 Windows 11 记事本

最终安装版连接真实 Windows 11 记事本：

- 进程 pid 5656，主窗口 hwnd 133878，子控件 class 为 `Edit`，hwnd 133874，物理矩形 `308,231,1292,530`。
- `GetWindowTextLengthW` 返回 0，`GetWindowTextW` 返回空；因此实际走 `WM_GETTEXT` 整段文本，再用 `EM_CHARFROMPOS` / `EM_POSFROMCHAR` 把屏幕坐标映射到字符。
- 文本 120 字符、3 行：

```text
A)As being a greater threat.
China declared a "war on pollution" and
less linear circular identification nanogenerator
```

最终 sweep 结果（脚本 `F:\DSH\tmp\p0-notepad-full.ps1`，完整日志 `F:\DSH\tmp\p0-full-results.txt`）：

| 目标 | 测试点 | 结果 | 截图 |
| --- | --- | --- | --- |
| `pollution` 第 1–9 个字母 | 698..818, 273 | 9/9 都显示一行 headword `pollution` | `F:\DSH\tmp\p0-full-pollution-55.png` … `-63.png` |
| `being` 第 1–5 个字母 | 398..458, 241 | 5/5 都显示 `being`（OALD `be` 原形） | `F:\DSH\tmp\p0-full-being-5.png` … `-9.png` |
| `war` b/w/a/r | 592/608/622, 273 | 3/3 都显示 `war` | `F:\DSH\tmp\p0-full-war-48.png` … `-50.png` |
| `on` o/n | 652/668, 273 | 2/2 都显示 `on` | `F:\DSH\tmp\p0-full-on-52.png`、`-53.png` |
| 空白（空格） | 382, 241 | 不出卡 | `F:\DSH\tmp\p0-full-blank-4.png` |
| 题号/左括号 `)` | 338, 241 | 不出卡 | `F:\DSH\tmp\p0-full-marker-1.png` |

每次测试都校验：光标读回值等于目标点、`WindowFromPoint` 的 pid 为 5656、overlay 可见且位于目标点附近。汇总图：`F:\DSH\tmp\p0-final-build-sheet.png`。

### 3.2 Chrome 普通网页

- Chrome for Testing pid 34444，标题 `Eneye precision QA - Google Chrome for Testing`，页面 `F:\CODEX\2026-11-06\qin\work\eneye-qa.html`。
- 最终 sweep 结果（脚本 `F:\DSH\tmp\p0-chrome-full.ps1`，日志 `F:\DSH\tmp\p0-chrome-results.txt`）：

| 目标 | 测试点 | 结果 | overlay |
| --- | --- | --- | --- |
| `pollution` | 204, 288 | 显示 `pollution`，OALD 三个名词义 | `216,300,592,836` |
| `being` | 330, 377 | 显示 `being`，OALD `be 原形` | `342,389,718,920` |
| `war` | 465, 456 | 显示 `war`，OALD 名词义 | `477,468,853,999` |
| `on` | 543, 456 | 显示 `on`，OALD 介词义 | `555,468,931,999` |

截图：`F:\DSH\tmp\p0-chrome-pollution.png`、`p0-chrome-being.png`、`p0-chrome-war.png`、`p0-chrome-on.png`；汇总图 `F:\DSH\tmp\p0-final-chrome-sheet.png`。

Chrome `being` 的关键修复在 `decide_uia_ocr`：当 UIA 已读到完整单词、OCR 只读到视觉前缀且与 UIA 文本不同时，保留 UIA，不再把完整单词清空后交给 OCR。

### 3.3 悬浮窗偏移、翻转与多屏

最终安装版实测：

- `F:\DSH\tmp\p0-offsets.ps1` 使用真实 Win32 `Edit` 夹具（hwnd 134336，Edit 1248418），对 0/0、20/20、64/64、120/120、240/240 五组偏移分别测 LIVE（滑块拖动后不重新悬停）和 FRESH（重新悬停）。
- 结果：LIVE 与 FRESH 的 `dx=dy=0`，卡片边缘严格等于光标 + 设置偏移；截图 `F:\DSH\tmp\p0-offset-live-<x>-<y>.png`、`p0-offset-fresh-<x>-<y>.png`。
- 翻转：夹具放到主屏 `1500,1000`，目标 `1980,1097`，偏移 240/240；overlay `1372,329,1748,865`，卡片 `1380,337`，右侧间隙 240、底部间隙 240、`leftDx=topDy=0`；截图 `F:\DSH\tmp\p0-flip.png`。
- 多屏：副屏工作区 `-2560,-150,0,1540`，`GetDpiForMonitor` = 144（150%）。夹具放在副屏，目标 `-1806,423`；overlay `-1788,442,-1224,1246`，物理尺寸 `564x804`。透明 gutter 在 150% 下为 12px，所以卡片实际左上角为 `-1776,454`，即目标 +30/+30，正好等于 20 CSS px × 1.5，误差 0；截图 `F:\DSH\tmp\p0-multi.png`。

本轮还修正了一个仅在跨 DPI 屏时出现的真实问题：`position_overlay` 原先使用窗口当前所在屏的 `scale_factor`，窗口尚未移动到副屏时会按 100% 计算偏移，导致 150% 屏上偏移只有 20 物理 px。现在先在目标点调用 `MonitorFromPoint` + `GetDpiForMonitor`，用目标屏 DPI 计算 padding、物理卡片尺寸和物理偏移；单测 `scales_card_and_offset_for_the_target_monitor` 覆盖 1.0/1.5/2.0。

### 3.4 AI 翻译（最终安装版）

- mock 服务：`F:\DSH\tmp\eneye-mock.js`，`http://127.0.0.1:18765/v1`，模型 `mock-ok`/`mock-slow`/`mock-broken`/`mock-401`/`mock-429`/`mock-500`。
- 最终安装版设置 `mock-slow`，在真实记事本中拖选 `China declared a "war on pollution" and`，5.2s 后截图 `F:\DSH\tmp\ai-drag-2.png`：卡片为 `AI TRANSLATION / 正在翻译`，内容 `缓慢 m`，`停止翻译` 可见。
- 立即通过 UIA 调用 `停止翻译`，1.2s 后截图 `F:\DSH\tmp\final-cancel-build2.png`：标题 `已停止翻译`，保留已生成的 `缓慢 mock 译文正在生成`，中性提示 `已停止，保留已经生成的内容。`，提供 `重新翻译`，**没有**红色 `translation cancelled` 错误。
- 该行为由 `applyTranslationFailure` 保证：命令 Promise 的 reject 不会覆盖已经到达的 `cancelled` 事件；`translation-state.test.ts` 覆盖取消、错误、旧 requestId 和已完成状态。
- 本轮验收结束后已清理本机测试配置：删除 mock Base URL、mock 模型、测试 API Key 和测试词典注册，只保留两本 OALD；安装版当前为 AI 未配置状态，不会把 mock 配置留给用户。

### 3.5 P1：词典真实设置页流程

以下流程在安装版设置页完成，代码与最终构建相同（最终构建只改了跨屏定位）：

- **导入**：通过「词典 → 选择词典文件」导入 `F:\DSH\tmp\eneye-fixtures\sample.csv`、MDX `test.mdx`、StarDict `testdict.ifo`、重定向 StarDict `redirect.ifo`；列表显示 CSV/MDX/STARDICT 类型和路径。
- **CSV 卡片**：记事本中悬停 `orbitword`，卡片显示 headword、`/ˈɔːbɪt/`、`SAMPLE`、`noun`、`轨道测试词条; 第二行释义`；截图 `F:\DSH\tmp\dict-orbit.png`。
- **HTML senses**：设置页搜索 `pollution`，OALD 返回 2 条结果，卡片/列表显示 3 个可折叠义项，另有 ECDICT 纯文本 fallback `n. 污染, 玷污 [化] 污染`；截图 `F:\DSH\tmp\settings-search-pollution.png`、展开态 `settings-search-pollution-open2.png`。
- **重定向**：搜索 `makes` 显示 `OALDPE EN-CN 精装版 V2025.02.14（MAKE 原形）` 和 `make` 义项，没有 `@@@LINK`；搜索 `aliaszz` 显示 `REDIRECT FIXTURE（TARGETZZ 原形）`；搜索 `brokenlinkzz` 显示 `重定向目标不存在：doesnotexistzz`；搜索 `cycleazz` 显示 `重定向循环，已停止解析：cyclebzz`，均未裸露控制串。截图 `redirect-broken2.png`、`redirect-cycle.png`。
- **重排/重新定位/移除/重启**：把 Redirect Fixture 上移，重新定位到 `F:\DSH\tmp\eneye-fixtures\relinked\redirect.ifo`，移除 Test Dict；`dictionaries.json` 保存顺序与路径，重启后列表和 `aliaszz` 查询仍正常，`test.mdx` 与原重定向文件仍在磁盘上；截图 `F:\DSH\tmp\dict-after-restart2.png`。
- **AI 错误路径**：`mock-401` → `AI 服务返回 401：API Key 无效或者模型权限...`；`mock-429` → `AI 服务返回 429：请求过于频繁或额度不足...`；`mock-500` → `AI 服务返回 500：服务暂时不可用...`；`mock-broken` → `AI 流读取中断`；截图 `ai-mock-401.png`、`ai-mock-429.png`、`ai-mock-500.png`、`ai-mock-broken.png`。
- **解释 vs 翻译**：`nanogenerator` 为空词典结果时点击「用 AI 解释」，卡片 eyebrow 为 `AI EXPLANATION`，mock 收到的 system prompt 是双语词典编辑说明（词性/核心义/一个短例，≤160 字符），用户文本为 `nanogenerator`；`history.sqlite` 中 `nanogenerator` 记录数为 0。截图 `F:\DSH\tmp\ai-explain-result.png`。
- **历史**：成功翻译完成后 `history` 有 `sentence` 行；解释不写历史；取消的慢速翻译未产生新的完成记录。

## 4. 诊断页

最终构建的诊断页在 `设置 → 诊断`，实测显示：

- ECDICT：770,611 词条，正常；
- 本地 OCR：Windows 本地 OCR 已启用，正常；
- 系统接口：Windows 10 22H2 / Windows 11 x64，正常；
- 最近一次取词探针：可显示命中/未命中、耗时和四层 stages。

截图：`F:\DSH\tmp\v02-diagnostics2.png`。纠正入口写入 `recognition-corrections.jsonl`，由 `append_recognition_correction` 的 JSONL 追加测试和空值拒绝测试覆盖。

## 5. 仍未闭环的项

1. **真实厂商 AI 未测**：本轮全部 AI E2E 使用本机 loopback mock；真实 DeepSeek/OpenAI/Anthropic Key 未提供，因此不声称真实厂商协议已验收。
2. **MDD 成对资源未测**：MDX 文件精确绑定已验证；同 stem 的 `.mdd` 资源加载只有代码级保护，缺真实 MDD fixture。
3. **200% DPI 未测**：100% 与 150% 已实测；200% 只有单测覆盖缩放公式。
4. **透明 gutter 点击穿透、拖动后设置保存**：代码已实现，本轮 P0 未做真实物理点击/拖动复测，矩阵保持 `partial`。
5. **外部词典文件不随仓库提交**：OALD 等大文件仅在本机 `D:\SALAD DICTIONGARY`；仓库只有小型 MIT fixture。
6. **自动化方式**：鼠标为 `SetCursorPos` + `mouse_event` 注入，不是人工手部操作；每次测试同时校验光标读回值、目标 pid 和 overlay 可见性，但长等待测试仍可能被本机其他代理干扰。

## 6. 复现命令

```powershell
cd F:\英语阅读
cargo test --lib
cargo clippy --all-targets -- -D warnings
cargo fmt --check
npm run verify
npm run tauri build
```

安装：

```powershell
& 'D:\DevTools\Rust\targets\eneye\release\bundle\nsis\Eneye_0.1.0_x64-setup.exe' /S /D=D:\Eneye
& 'D:\Eneye\eneye.exe'
```
