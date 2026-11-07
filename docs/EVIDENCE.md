# Eneye v0.2 验收证据

本文件记录 v0.2 阶段 0–5 的可复现证据、构建产物、测试门禁和仍然存在的限制。产品行为契约见 [PRODUCT_CONTRACT.md](PRODUCT_CONTRACT.md)，失败矩阵见 [FAILURE_MATRIX.md](FAILURE_MATRIX.md)。

## 1. 构建与安装

| 项目 | 值 |
| --- | --- |
| 安装包 | `D:\DevTools\Rust\targets\eneye\release\bundle\nsis\Eneye_0.1.0_x64-setup.exe` |
| 安装包大小 | 38,410,459 bytes（36.63 MiB） |
| 安装包 SHA256 | `4644D166B1801B9F1C53B6E35CF4332D0D861BF7AAD051EB7657CE711BE6791E` |
| 安装位置 | `D:\Eneye` |
| 已安装 exe | `D:\Eneye\eneye.exe` |
| exe SHA256 | `E34DF8B797EA9F61FD8D1E1366A2A250E790B173026250CBBA1CF17A7AD35743` |
| exe 大小 | 26,061,824 bytes |
| 构建时间 | 2026-11-07 13:52 左右 |
| 运行进程 | pid 34932 |

安装方式：NSIS 静默安装 `/S /D=D:\Eneye`，随后启动 `D:\Eneye\eneye.exe`。

## 2. 自动化门禁

| 门禁 | 结果 |
| --- | --- |
| `cargo test --lib` | 118 passed，1 ignored，0 failed |
| ignored 测试 | 需要真实 Edit HWND 的交互测试，已由第 3 节真实记事本证据覆盖 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo fmt --check` | 通过 |
| `npm run verify` | Prettier、tsc、45 个前端测试（10 个文件）、Vite build 全部通过 |
| `npm run tauri build` | 通过，生成上述 NSIS 安装包 |

## 3. P0：真实鼠标取词

### 3.1 Windows 11 记事本

最终安装版连接真实 Windows 11 记事本：

- 进程 pid 5656，主窗口 hwnd 133878，子控件 class 为 `Edit`，hwnd 133874；验收脚本把窗口移到 `200,120`，Edit 左上角为 `208,171`。
- `GetWindowTextLengthW` 返回 0，`GetWindowTextW` 返回空；因此实际走 `WM_GETTEXT` 整段文本，再用 `EM_CHARFROMPOS` / `EM_POSFROMCHAR` 把屏幕坐标映射到字符。
- 文本 120 字符、3 行：

```text
A)As being a greater threat.
China declared a "war on pollution" and
less linear circular identification nanogenerator
```

最终 sweep 结果（脚本 `F:\DSH\tmp\validate-notepad-card.ps1`，每张都是 `1700x1000` 全帧，同时包含记事本文本和卡片；汇总见 `p0-notepad-valid-results.txt`）：

| 目标 | 字符索引（CRLF） | 结果 | 截图 |
| --- | --- | --- | --- |
| `pollution` 第 1–9 个字母 | 55..63 | 9/9 显示一行 headword `pollution` | `F:\DSH\tmp\p0-notepad-valid-pollution-55.png` … `-63.png`；汇总 `F:\DSH\tmp\sheet-pollution.png` |
| `being` 第 1–5 个字母 | 5..9 | 5/5 显示 `being`（OALD `be` 原形） | `F:\DSH\tmp\p0-notepad-valid-being-5.png` … `-9.png`；汇总 `F:\DSH\tmp\sheet-motion.png` |
| `war` w/a/r | 48/49/50 | 3/3 显示 `war` | `F:\DSH\tmp\p0-notepad-valid-war-48.png` … `-50.png`；汇总 `F:\DSH\tmp\sheet-motion.png` |
| `on` o/n | 52/53 | 2/2 显示 `on` | `F:\DSH\tmp\p0-notepad-valid-on-52.png`、`-53.png`；汇总 `F:\DSH\tmp\sheet-motion.png` |
| 空白（As 后空格） | 4 | 不出卡 | `F:\DSH\tmp\p0-notepad-valid-blank-4.png`；汇总 `F:\DSH\tmp\sheet-motion.png` |
| 题号/左括号 `)` | 1 | 不出卡 | `F:\DSH\tmp\p0-notepad-valid-marker-1.png`；汇总 `F:\DSH\tmp\sheet-motion.png` |

每次测试都校验：目标点由 `EM_POSFROMCHAR` 的字符左边界加 6px 得到、光标读回值等于目标点、`WindowFromPoint` 的 pid 为 5656、overlay 在目标点附近。注意记事本使用 CRLF：`war` 的 `w` 是索引 48（索引 47 是前引号），因此旧截图里的 47 只落在引号上。用户点名的四个 `p0-notepad-*.png` 已用全帧重拍并覆盖；旧 `p0-full-*` 是 380x570 裁剪图，只留作历史，不再作为验收证据。

### 3.1.1 最终版 poll / on 复测

最终安装版（E34D）用 `F:\DSH\tmp\poll-fixture.txt`（内容 `poll on`）复测真实 Win32 Edit：poll 344,241 -> OALD `poll` 名词/动词；on 380,241 -> OALD `on` 介词；overlay 分别为 356,253 和 392,253。截图 `F:\DSH\tmp\p0-poll-poll.png`、`p0-poll-on.png`。

### 3.2 Chrome / Edge 普通网页（上一构建 + 最终复测）

以下为 E34D 之前构建的 Chrome 表；最终版复测见 3.2.1。

- Chrome for Testing pid 34444，标题 `Eneye precision QA - Google Chrome for Testing`，页面 `F:\CODEX\2026-11-06\qin\work\eneye-qa.html`。
- 最终 sweep 结果（脚本 `F:\DSH\tmp\p0-chrome-full.ps1`，日志 `F:\DSH\tmp\p0-chrome-results.txt`）：

| 目标 | 测试点 | 结果 | overlay |
| --- | --- | --- | --- |
| `pollution` | 204, 288 | 显示 `pollution`，OALD 三个名词义 | `216,300,592,836` |
| `being` | 330, 377 | 显示 `being`，OALD `be 原形` | `342,389,718,920` |
| `war` | 465, 456 | 显示 `war`，OALD 名词义 | `477,468,853,999` |
| `on` | 543, 456 | 显示 `on`，OALD 介词义 | `555,468,931,999` |

截图：`F:\DSH\tmp\p0-chrome-pollution.png`、`p0-chrome-being.png`、`p0-chrome-war.png`、`p0-chrome-on.png`；汇总图 `F:\DSH\tmp\p0-final-chrome-sheet.png`。

### 3.2.1 最终版 Chrome / Edge 复测

最终安装版（E34D）复测：

| 浏览器 | 目标 | 测试点 | 结果 | 截图 |
| --- | --- | --- | --- | --- |
| Chrome for Testing 156.0.8078.80 | `pollution` | 204,288 | OALD pollution 三个名词义 | `p0-chrome-pollution.png` |
| Chrome | `being` | 330,377 | OALD being（be 原形） | `p0-chrome-being.png` |
| Chrome | `war` | 465,456 | OALD war 名词 + 习语 | `p0-chrome-war.png` |
| Chrome | `on` | 543,456 | OALD on 介词 | `p0-chrome-on.png` |
| Edge 156.0.4314.58 | `pollution` | 204,280 | OALD pollution | `p0-edge-crop-pollution.png` |
| Edge | `being` | 330,363 | OALD being | `p0-edge-crop-being.png` |
| Edge | `war` | 465,432 | OALD war | `p0-edge-crop-war.png` |
| Edge | `on` | 543,432 | OALD on | `p0-edge-crop-on.png` |

Chrome 窗口 80,80,1480,840，overlay 分别在 216,300 / 342,389 / 477,468 / 555,468。Edge 浏览器 chrome 更高，页面首行 y=280；不要把 y=343 当作第二行。Edge 必须用 `WScript.Shell.AppActivate(28744)` 再 `SetForegroundWindow`，否则截图会落到 Chrome/记事本。

### 3.2.2 Chrome `being` 关键修复

Chrome `being` 的关键修复在 `decide_uia_ocr`：当 UIA 已读到完整单词、OCR 只读到视觉前缀且与 UIA 文本不同时，保留 UIA，不再把完整单词清空后交给 OCR。

### 3.3 悬浮窗偏移、翻转与多屏（上一构建）

以下为 E34D 之前构建的实测；最终版复测见 3.3.1。

- `F:\DSH\tmp\p0-offsets.ps1` 使用真实 Win32 `Edit` 夹具（hwnd 134336，Edit 1248418），对 0/0、20/20、64/64、120/120、240/240 五组偏移分别测 LIVE（滑块拖动后不重新悬停）和 FRESH（重新悬停）。
- 结果：LIVE 与 FRESH 的 `dx=dy=0`，卡片边缘严格等于光标 + 设置偏移；截图 `F:\DSH\tmp\p0-offset-live-<x>-<y>.png`、`p0-offset-fresh-<x>-<y>.png`。
- 翻转：夹具放到主屏 `1500,1000`，目标 `1980,1097`，偏移 240/240；overlay `1372,329,1748,865`，卡片 `1380,337`，右侧间隙 240、底部间隙 240、`leftDx=topDy=0`；截图 `F:\DSH\tmp\p0-flip.png`。
- 多屏：副屏工作区 `-2560,-150,0,1540`，`GetDpiForMonitor` = 144（150%）。夹具放在副屏，目标 `-1806,423`；overlay `-1788,442,-1224,1246`，物理尺寸 `564x804`。透明 gutter 在 150% 下为 12px，所以卡片实际左上角为 `-1776,454`，即目标 +30/+30，正好等于 20 CSS px × 1.5，误差 0；截图 `F:\DSH\tmp\p0-multi.png`。

### 3.3.1 最终版偏移、翻转、gutter、拖动复测

最终安装版（E34D，设置窗口 hwnd 3933882）实测：

- **偏移**：`F:\DSH\tmp\p0-offsets-final.ps1`，真实 Win32 Edit 夹具放在 100,100，目标 558,195。LIVE 与 FRESH 均严格相等：0,0 -> 558,195；20,20 -> 578,215；64,64 -> 622,259；120,120 -> 678,315；240,240 -> 798,435，`dx=dy=0`。截图 `p0-offset-live-*.png`、`p0-offset-fresh-*.png`。
- **翻转**：`p0-flip-final.ps1`，夹具 1500,1000，目标 1958,1095，偏移 240/240；卡片 `1350,327,1726,863`，边缘 `1358,335`，与期望完全相等；右侧间隙 600、底部间隙 760；截图 `p0-flip-current.png`。
- **透明 gutter**：`p0-gutter-current3.ps1`，卡片边缘 578,215，点击 gutter 574,255，记事本 caret 从 110,110 变为 102,102；新卡出现在 pointer+20。证明 gutter 不挡下层。日志 `p0-gutter-current2.txt`。
- **拖动保存**：`p0-drag-final2.ps1`，目标 558,195，初始边缘 578,215；拖动 +200,+100 后边缘 778,315；把偏移改为 40/40 后边缘 798,335，正好等于「拖动后锚点 + 20」，不是旧词锚点 598,235。截图 `p0-drag.png`，日志 `p0-offset-trial.txt`。

本轮还修正了一个仅在跨 DPI 屏时出现的真实问题：`position_overlay` 原先使用窗口当前所在屏的 `scale_factor`，窗口尚未移动到副屏时会按 100% 计算偏移，导致 150% 屏上偏移只有 20 物理 px。现在先在目标点调用 `MonitorFromPoint` + `GetDpiForMonitor`，用目标屏 DPI 计算 padding、物理卡片尺寸和物理偏移；单测 `scales_card_and_offset_for_the_target_monitor` 覆盖 1.0/1.5/2.0。

### 3.4 AI 翻译（上一构建）

- mock 服务：`F:\DSH\tmp\eneye-mock.js`，`http://127.0.0.1:18765/v1`，模型 `mock-ok`/`mock-slow`/`mock-broken`/`mock-401`/`mock-429`/`mock-500`。
- 该构建设置 `mock-slow`，在真实记事本中拖选 `China declared a "war on pollution" and`，5.2s 后截图 `F:\DSH\tmp\ai-drag-2.png`：卡片为 `AI TRANSLATION / 正在翻译`，内容 `缓慢 m`，`停止翻译` 可见。
- 立即通过 UIA 调用 `停止翻译`，1.2s 后截图 `F:\DSH\tmp\final-cancel-build2.png`：标题 `已停止翻译`，保留已生成的 `缓慢 mock 译文正在生成`，中性提示 `已停止，保留已经生成的内容。`，提供 `重新翻译`，**没有**红色 `translation cancelled` 错误。
- 该行为由 `applyTranslationFailure` 保证：命令 Promise 的 reject 不会覆盖已经到达的 `cancelled` 事件；`translation-state.test.ts` 覆盖取消、错误、旧 requestId 和已完成状态。
- 本轮验收结束后已清理本机测试配置：删除 mock Base URL、mock 模型、测试 API Key 和测试词典注册，只保留两本 OALD；安装版当前为 AI 未配置状态，不会把 mock 配置留给用户。E34D 只改了模型手输控件，AI 协议与取消逻辑未改，但未重跑本组 E2E。

### 3.5 P1：词典真实设置页流程（上一构建）

以下流程在上一构建的设置页完成。E34D 只改了模型输入控件和拖动锚点，词典流程代码未变，但未逐项重跑：

- **导入**：通过「词典 → 选择词典文件」导入 `F:\DSH\tmp\eneye-fixtures\sample.csv`、MDX `test.mdx`、StarDict `testdict.ifo`、重定向 StarDict `redirect.ifo`；列表显示 CSV/MDX/STARDICT 类型和路径。
- **CSV 卡片**：记事本中悬停 `orbitword`，卡片显示 headword、`/ˈɔːbɪt/`、`SAMPLE`、`noun`、`轨道测试词条; 第二行释义`；截图 `F:\DSH\tmp\dict-orbit.png`。
- **HTML senses**：设置页搜索 `pollution`，OALD 返回 2 条结果，卡片/列表显示 3 个可折叠义项，另有 ECDICT 纯文本 fallback `n. 污染, 玷污 [化] 污染`；截图 `F:\DSH\tmp\settings-search-pollution.png`、展开态 `settings-search-pollution-open2.png`。
- **重定向**：搜索 `makes` 显示 `OALDPE EN-CN 精装版 V2025.02.14（MAKE 原形）` 和 `make` 义项，没有 `@@@LINK`；搜索 `aliaszz` 显示 `REDIRECT FIXTURE（TARGETZZ 原形）`；搜索 `brokenlinkzz` 显示 `重定向目标不存在：doesnotexistzz`；搜索 `cycleazz` 显示 `重定向循环，已停止解析：cyclebzz`，均未裸露控制串。截图 `redirect-broken2.png`、`redirect-cycle.png`。
- **重排/重新定位/移除/重启**：把 Redirect Fixture 上移，重新定位到 `F:\DSH\tmp\eneye-fixtures\relinked\redirect.ifo`，移除 Test Dict；`dictionaries.json` 保存顺序与路径，重启后列表和 `aliaszz` 查询仍正常，`test.mdx` 与原重定向文件仍在磁盘上；截图 `F:\DSH\tmp\dict-after-restart2.png`。
- **AI 错误路径**：`mock-401` → `AI 服务返回 401：API Key 无效或者模型权限...`；`mock-429` → `AI 服务返回 429：请求过于频繁或额度不足...`；`mock-500` → `AI 服务返回 500：服务暂时不可用...`；`mock-broken` → `AI 流读取中断`；截图 `ai-mock-401.png`、`ai-mock-429.png`、`ai-mock-500.png`、`ai-mock-broken.png`。
- **解释 vs 翻译**：`nanogenerator` 为空词典结果时点击「用 AI 解释」，卡片 eyebrow 为 `AI EXPLANATION`，mock 收到的 system prompt 是双语词典编辑说明（词性/核心义/一个短例，≤160 字符），用户文本为 `nanogenerator`；`history.sqlite` 中 `nanogenerator` 记录数为 0。截图 `F:\DSH\tmp\ai-explain-result.png`。
- **历史**：成功翻译完成后 `history` 有 `sentence` 行；解释不写历史；取消的慢速翻译未产生新的完成记录。

### 3.6 模型手输

设置页问答/翻译/图片三行的模型选择已从 `<select>` 改为可输入的 `<input list>` + `<datalist>`：`/models` 为空或不可用时可直接输入模型名，仍保留下拉候选。`SettingsWindow.test.tsx` 新增用例在翻译行输入 `deepseek-chat` 并断言值；`npm run verify` 共 45 个前端测试通过。

## 4. 诊断页

上一构建的诊断页在 `设置 → 诊断`，实测显示（E34D 未重跑）：

- ECDICT：770,611 词条，正常；
- 本地 OCR：Windows 本地 OCR 已启用，正常；
- 系统接口：Windows 10 22H2 / Windows 11 x64，正常；
- 最近一次取词探针：可显示命中/未命中、耗时和四层 stages。

截图：`F:\DSH\tmp\v02-diagnostics2.png`。纠正入口写入 `recognition-corrections.jsonl`，由 `append_recognition_correction` 的 JSONL 追加测试和空值拒绝测试覆盖。

## 5. 仍未闭环的项

1. **真实厂商 AI 未测**：本轮全部 AI E2E 使用本机 loopback mock；真实 DeepSeek/OpenAI/Anthropic Key 未提供，因此不声称真实厂商协议已验收。
2. **MDD 成对资源未测**：MDX 文件精确绑定已验证；同 stem 的 `.mdd` 资源加载只有代码级保护，缺真实 MDD fixture。
3. **200% DPI 未测**：100% 与 150% 已实测；200% 只有单测覆盖缩放公式。
4. **gutter 点击穿透、拖动后设置保存**：最终版已实测通过（3.3.1），矩阵为 `verified`。
5. **最终版只复测了 P0**：Chrome/Edge/poll/偏移/翻转/gutter/拖动已在 E34D 上复测；AI、跨屏 150%、词典设置页仍是上一构建证据，代码未改但未逐项重跑。
6. **模型手输**：设置页已从下拉改为可输入 combobox，单测覆盖手输 `deepseek-chat`；没有单独模拟 `/models` 网络全失败。
7. **外部词典文件不随仓库提交**：OALD 等大文件仅在本机 `D:\SALAD DICTIONGARY`；仓库只有小型 MIT fixture。
8. **自动化方式**：鼠标为 `SetCursorPos` + `mouse_event` 注入，不是人工手部操作；每次测试同时校验光标读回值、目标 pid 和 overlay 可见性，但长等待测试仍可能被本机其他代理干扰。

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
