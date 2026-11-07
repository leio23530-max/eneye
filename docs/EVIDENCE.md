# Eneye v0.2 验收证据

本文件记录 v0.2 阶段 0–5 的可复现证据、构建产物、测试门禁和仍然存在的限制。产品行为契约见 [PRODUCT_CONTRACT.md](PRODUCT_CONTRACT.md)，失败矩阵见 [FAILURE_MATRIX.md](FAILURE_MATRIX.md)。

## 1. 构建与安装

| 项目 | 值 |
| --- | --- |
| 安装包 | `D:\DevTools\Rust\targets\eneye\release\bundle\nsis\Eneye_0.1.0_x64-setup.exe` |
| 安装包大小 | 36.59 MiB |
| 安装包 SHA256 | `04B7AE7562B84841BE227B2A8813887E218632A41FD8AAF24D4F70118448F725` |
| 安装位置 | `D:\Eneye` |
| 已安装 exe | `D:\Eneye\eneye.exe` |
| exe SHA256 | `93B094F1A66AB506E598E082106BCB19C7DA36F255FE659926F899417F79F12A` |
| 构建时间 | 2026-11-07 09:50 左右 |
| 运行进程 | pid 12796 |

安装方式：NSIS 静默安装 `/S /D=D:\Eneye`，随后启动 `D:\Eneye\eneye.exe`。

## 2. 自动化门禁

| 门禁 | 结果 |
| --- | --- |
| `cargo test --lib` | 117 passed，1 ignored，0 failed |
| ignored 测试 | 需要真实 Edit HWND 的交互测试，已由本文件第 3 节的人工/脚本证据覆盖 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo fmt --check` | 通过 |
| `npm run verify` | Prettier、tsc、40 个前端测试（10 个文件）、Vite build 全部通过 |
| `npm run tauri build` | 通过，生成上述 NSIS 安装包 |

## 3. 已安装构建的真实取词证据

### 3.1 测试夹具

使用一个**真正的 Win32 `Edit` 控件**作为夹具，避免 WinForms 的 `WindowsForms10.EDIT.*` 类名绕过 Eneye 的精确 Edit 路径：

- 进程：pid 31552
- 窗口：`Eneye final evidence surface`，hwnd 134336
- 子控件：class `Edit`，hwnd 1248418，DPI 感知物理矩形 `128,151,1068,381`
- 字体：Consolas 20
- 文本：

```text
A)As being a greater threat.
China declared a "war on pollution" and
less linear circular identification nanogenerator
```

每个词的中心由 `EM_POSFROMCHAR(0x00D6)` 计算，而不是用截图目测：

| 目标 | 光标坐标 | 结果 | 截图 |
| --- | --- | --- | --- |
| `on` | 475,201 | 卡片 headword `on`，OALD 介词义 | `F:\DSH\tmp\final-on.png` |
| `war` | 415,201 | 卡片 headword `war`，OALD 名词义 | `F:\DSH\tmp\final-war.png` |
| `being` | 220,169 | 卡片 headword `being`，OALD `be 原形` | `F:\DSH\tmp\final-being.png` |
| `pollution` | 520,201 | 本次未出现卡片；重试时光标被外部自动化移走 | 旧构建证据 `F:\DSH\tmp\native-pollution.png` |

对应的 overlay 窗口实测：

- `on`：hwnd 1377850，visible，rect `487,213,812,748`，光标 `475,201`，卡片在指针右下方。
- `war`：同一 hwnd，rect `427,213,743,748`，光标 `415,201`。
- `being`：同一 hwnd，rect `232,181,542,716`，光标 `220,169`。

这组证据证明：

- 安装版走的是精确 Edit 路径，而不是把整段文本交给 OCR；
- `on`、`war`、`being` 没有前缀污染，也没有把 `war` 拆成 `wa/r`；
- headword 在 360px 卡片内保持一行；
- 卡片出现在指针附近，而不是屏幕左上角。

### 3.2 `pollution` 的说明

旧构建 `2368E4F2…` 在同一夹具上取得过 `pollution` 卡片（`F:\DSH\tmp\native-pollution.png`），headword 一行、OALD 三个名词义。最终构建 `93B094F1…` 的两次 `pollution` 重试中，外部自动化分别把光标改到 `166,234` 和 `1324,638`，因此没有把失败归因于应用，也没有伪造通过结果。识别路径、词典路径和 UI 在两次构建之间没有变化；该样例仍需在无外部输入干扰的环境复测。

### 3.3 单实例

最终构建连续启动两次：

- 第一次启动 pid 12796；
- 第二次执行 `D:\Eneye\eneye.exe` 没有产生第二个进程；
- 设置窗口 hwnd 1837676 被前置，可见矩形 `812,344,1748,1063`。

这验证了 `tauri-plugin-single-instance` 和设置窗聚焦逻辑。

## 4. 诊断页

最终构建的诊断页在 `设置 → 诊断`，实测显示：

- ECDICT：770,611 词条，正常；
- 本地 OCR：Windows 本地 OCR 已启用，正常；
- 系统接口：Windows 10 22H2 / Windows 11 x64，正常；
- 最近一次取词探针：尚未命中单词 · 186ms（点击设置页时触发）。

截图：`F:\DSH\tmp\v02-diagnostics2.png`。

诊断页还提供四层识别 stages、候选排序、失败原因和“这次识别错了”纠正入口；纠正写入 `recognition-corrections.jsonl`，并由 `append_recognition_correction` 的 JSONL 追加测试和空值拒绝测试覆盖。

## 5. 仍未闭环的项

1. **Win11 记事本证据缺失**：当前记事本可见 `Edit` 的文本长度为 0，UIA 只暴露整段 Pane Name。精确 Edit 路径已由 Win32 `Edit` 夹具覆盖，但“记事本里 pollution 任意字母”仍需要在记事本真实控件上复测。
2. **Chrome `being` 未取得最终构建证据**：Chrome QA 页布局变化后，正确坐标约为 `237,367`；测试期间外部自动化把光标移到 `198,369`，卡片未出现。旧构建在 Chrome 上验证过 `pollution`，但 `being` 仍需干净环境复测。
3. **副屏卡片尺寸/DPI 异常**：负坐标副屏上卡片已出现在指针旁，但 overlay 窗口实测 `501x804`，且 `GetDpiForWindow` 返回 144，而两台显示器与系统均为 96。主屏最终构建卡片宽度也出现 310–325px（高度约 535px），与预期的 376×536 不一致。该问题不影响 headword 正确性，但会影响视觉尺寸和多屏一致性，需要后续单独修复。
4. **外部自动化干扰**：本机同时有另一个代理在移动窗口和光标，部分长等待测试不可复现；本文件只保留光标读回与窗口矩形同时自洽的样例。

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
