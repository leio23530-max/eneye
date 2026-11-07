# Eneye v0.2 验收证据

本文件记录 v0.2 阶段 0–5 的可复现证据、构建产物、测试门禁和仍然存在的限制。产品行为契约见 [PRODUCT_CONTRACT.md](PRODUCT_CONTRACT.md)，失败矩阵见 [FAILURE_MATRIX.md](FAILURE_MATRIX.md)。

## 1. 构建与安装

| 项目 | 值 |
| --- | --- |
| 安装包 | `D:\DevTools\Rust\targets\eneye\release\bundle\nsis\Eneye_0.1.0_x64-setup.exe` |
| 安装包大小 | 36.62 MiB |
| 安装包 SHA256 | `35021D731599FAE04F4118F819A4CC67A19C403A784214AFF3DB911BE66CC47F` |
| 安装位置 | `D:\Eneye` |
| 已安装 exe | `D:\Eneye\eneye.exe` |
| exe SHA256 | `3B11B996C5E313B676BA5858637723F25C4BD8BB7CD9667400822A0C5680AE35` |
| 构建时间 | 2026-11-07 10:03 左右 |
| 运行进程 | pid 8640 |

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
- 夹具脚本：`F:\DSH\tmp\eneye-final-edit.ps1`
- 文本：

```text
A)As being a greater threat.
China declared a "war on pollution" and
less linear circular identification nanogenerator
```

每个词的中心由 `EM_POSFROMCHAR(0x00D6)` 计算，而不是用截图目测：

| 目标 | 光标坐标 | 结果 | 截图 |
| --- | --- | --- | --- |
| `on` | 482,201 | 卡片 headword `on`，OALD 介词义 | `F:\DSH\tmp\v02-new-on.png` |
| `war` | 430,201 | 卡片 headword `war`，OALD 名词义 | `F:\DSH\tmp\v02-new-war.png` |
| `being` | 250,172 | 卡片 headword `being`，OALD `be 原形` | `F:\DSH\tmp\v02-new-being.png` |
| `pollution` | 580,205 | 卡片 headword `pollution`，OALD 三个名词义 | `F:\DSH\tmp\v02-new-pollution.png` |

对应的 overlay 窗口实测（显示器 96 DPI）：

- `on`：rect `494,212,870,743`，size `376x531`，光标 `482,200`；
- `war`：rect `442,213,818,744`，size `376x531`，光标 `430,201`；
- `being`：rect `262,184,638,715`，size `376x531`，光标 `250,172`；
- `pollution`：rect `592,217,968,750`，size `376x533`，光标 `580,205`。

这组证据证明：

- 安装版走的是精确 Edit 路径，而不是把整段文本交给 OCR；
- `pollution`、`on`、`war`、`being` 都没有前缀污染，也没有把 `war` 拆成 `wa/r`；
- headword 在 360 CSS px 卡片内保持一行；
- 卡片出现在指针附近，而不是屏幕左上角。

### 3.2 卡片尺寸/DPI 根因与多屏复测

旧构建的卡片宽度在 310–325px 之间浮动，副屏曾出现 501x804。根因有两层：

1. 前端把 `rect.width * devicePixelRatio` 当作窗口尺寸上报，而卡片本身是 flex 子项且默认可收缩；窗口变窄后卡片继续收缩，形成逐次缩小的棘轮。
2. Rust 侧用 `PhysicalSize` 设置窗口，WebView 的 CSS 视口与物理尺寸在 150% DPI 下不一致。

修复后的约定：

- 前端只上报 **CSS 像素** 的宽高；x/y 仍为物理坐标；
- 卡片 `flex: 0 0 auto; width: 100%`，不再随窗口收缩；
- 窗口用 `LogicalSize`（360 + 2×8 gutter），高度按 CSS 高度 clamp 到 120–520；
- 物理卡片尺寸、偏移和放置由 `scale_factor` 换算，透明 gutter 仍为 8 CSS px。

实测：

- 主屏 100%：窗口 `376x531`/`376x533`，内容卡片 360 CSS px；四个 headword 均一行；
- 副屏 150%：窗口 `564x797`，正好是 376×1.5 与 531×1.5；光标 `-2035,397` 时卡片 rect `-2018,415,-1454,1212`，即卡片左上角约等于指针 +20 CSS px（物理 +30，含 gutter -12）。截图 `F:\DSH\tmp\v02-secondary-being.png`，headword `being` 一行。

因此 O-05 的多屏/负坐标放置与 DPI 尺寸已完成装机验证，不再是 3.2 节旧版本所述的未闭环异常。

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
2. **Chrome `being` 未取得最终构建证据**：Chrome QA 页布局变化后，正确坐标约为 `237,367`；测试期间外部自动化把光标移到 `198,369`，卡片未出现。旧构建在 Chrome 上验证过 `pollution`，但 `being` 仍需干净环境复测。原生 Edit 夹具上的 `being` 已通过。
3. **外部自动化干扰**：本机同时有另一个代理在移动窗口和光标，部分长等待测试不可复现；本文件只保留光标读回与窗口矩形同时自洽的样例。

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
