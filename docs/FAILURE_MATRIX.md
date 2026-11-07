# Eneye v0.2 失败矩阵与测试矩阵

状态：`open` 未修复；`partial` 有代码但未验收；`verified` 有实际运行证据。  
本文件随 Phase 更新；任何 `verified` 都必须能指向测试或真实鼠标证据。

## 1. 取词与识别

| ID | 场景 | 期望 | 当前状态 | 证据 / 根因 | 目标 Phase |
| --- | --- | --- | --- | --- | --- |
| C-01 | 记事本 `pollution` 任意字母 | `pollution` | open | Win11 记事本文本控件是 `ControlType.Pane`、class `Edit`，只有整段 Name，不支持 TextPattern；Win32 整段文本含换行被 `word_from_text_at_x` 拒绝；当前落到 OCR | 1 |
| C-02 | 记事本 `A)As being` | `being` 或 `As` | open | 同上；OCR token 可能把 `A)As` 合并 | 1 |
| C-03 | 记事本 `war on pollution` | 不拆成 `wa/r`、`0/n` | open | 同上；短词/数字混淆 | 1 |
| C-04 | 记事本空白、题号、下划线 | 不出卡 | partial | 现有视觉完整性只检查右侧墨迹，不检查左侧/整行 | 1 |
| C-05 | 独立 `poll`、`on` | 正常显示 | partial | 依赖 OCR 与词典证据，缺真实窗口回归 | 1 |
| C-06 | Chrome/Edge 普通网页 | 正确 headword | partial | 现有截图验证过 pollution，但未形成可重复 E2E | 1 |
| C-07 | PDF 文本层 / 扫描 PDF | 正确 headword / 不伪造 | open | 无真实 fixture 与 DPI 矩阵 | 1 |
| C-08 | Calibre / Office / WordPad | 正确 headword | open | 无真实窗口证据 | 1 |
| C-09 | 100% / 150% / 200% DPI | 词框和点击一致 | open | 现有固定 440×610 物理假设；OCR crop 坐标风险 | 1 / 4 |
| C-10 | 负坐标副屏 | 正确显示器、正确词框 | open | 现有测试无负坐标真实窗口 | 1 / 4 |
| C-11 | UIA/MSAA/Win32 冲突 | 有统一候选和解释 | open | `probe_accessible` 首个命中即返回，无统一候选列表 | 1 |
| C-12 | OCR 正常版 / 去干扰版 | 两组结果互不覆盖 | open | 当前仅一套 OCR 结果 | 1 |
| C-13 | 卡片/托盘/设置窗自取词 | 忽略 | verified | 已有自窗口/overlay 判定与测试 | - |
| C-14 | 同一词轻微抖动 | 不重复查询 | partial | 有稳定半径，缺真实鼠标抖动回归 | 1 |

## 2. 词典

| ID | 场景 | 期望 | 当前状态 | 证据 / 根因 | 目标 Phase |
| --- | --- | --- | --- | --- | --- |
| D-01 | CSV/TSV 导入 | 查询有结果、纯文本 fallback | partial | 已有 CSV 导入/查询与空词典拒绝单测；设置窗真实导入仍待 Phase 5 复测 | 2 |
| D-02 | MDX/MDD 成套识别 | 自动关联 MDD | partial | 改为精确 MDX 文件绑定；MDD 只接受同 stem 文件，缺真实 MDD fixture | 2 |
| D-03 | StarDict 成套识别 | ifo/idx/dict/dz/syn | verified | 精确 stem、压缩件与同义词测试通过 | 2 |
| D-04 | `@@@LINK` 可解析 | 显示目标词义 | verified | 已有 redirect 测试与 OALD 实测 | 2 回归 |
| D-05 | `@@@LINK` 目标缺失 | 明确提示，不裸露控制串 | verified | 缺失目标、循环、过深均有单测与中文提示 | 2 |
| D-06 | 重复 redirect | 去重 | partial | visited 去重与循环测试通过；真实 OALD 待 Phase 5 回归 | 2 |
| D-07 | HTML senses 折叠 | 每义项独立折叠 | verified | OALD 实测 via OverlayCard | 2 回归 |
| D-08 | 无法结构化 HTML | “完整释义”fallback | partial | 需补测试 | 2 |
| D-09 | 文件移动 | 重新定位 | verified | 新增 relink/reload 命令、设置窗按钮与保留 id/priority 的测试 | 2 |
| D-10 | 导入成功但查询空白 | 明确原因 | verified | 重复路径与零词条拒绝、错误上屏、relink 后清空 error | 2 |

## 3. AI

| ID | 场景 | 期望 | 当前状态 | 证据 / 根因 | 目标 Phase |
| --- | --- | --- | --- | --- | --- |
| A-01 | 无 Key / 无模型 | UI 明确“未配置” | verified | 设置页明确提示未选择翻译模型；命令返回可操作中文错误 | - |
| A-02 | 测试连接 + 模型列表 | 明确成功/失败 | partial | 有 `list_ai_models`，无 UI E2E | 3 |
| A-03 | `/models` 不可用 | 可手输模型 | partial | 后端允许，UI 需验收 | 3 |
| A-04 | 拖选 → 流式翻译 | 逐字增长、完成 | partial | OpenAI 兼容 mock HTTP 流式 E2E；前端 reducer 测试；缺真实选择拖选自动化 | 3 |
| A-05 | 取消 | 正常终态，无错误/重试 | verified | cancelled 中性终态、保留已生成内容、无 alert；reducer/UI 测试 | - |
| A-06 | 旧请求覆盖新卡 | 丢弃旧事件 | verified | applyTranslationEvent 明确丢弃旧 requestId；单测覆盖 | - |
| A-07 | 202/204/非流式 2xx | 明确文案 | verified | 202/204 明确提示；JSON completion 支持多种结构并有 mock 测试 | - |
| A-08 | 401/403/404/429/5xx | 明确文案 | verified | 补充 3xx/408/504/超时/连接失败中文；映射测试 | - |
| A-09 | 超时/断流/空响应 | 可恢复 | partial | 无 mock E2E；chunk 错误可能英文 | 3 |
| A-10 | 翻译 vs 解释 | 不同 prompt/命令 | verified | TranslationKind 区分 prompt；解释不写历史；前端标题/动作独立 | - |
| A-11 | Key 安全 | 只进 Credential Manager | verified | keyring + redact_secret 测试 | - |
| A-12 | 历史记录 | 成功才写，取消/失败不写 | partial | 后端成功才写；缺 E2E | 3 |

## 4. 悬浮窗

| ID | 场景 | 期望 | 当前状态 | 证据 / 根因 | 目标 Phase |
| --- | --- | --- | --- | --- | --- |
| O-01 | 距离 0–240 | 实测误差 ≤4px | partial | 双轴 0–240 已实现，place_card 单测 20/240 精确；待装机实测 150%/200% | 4 |
| O-02 | 右侧默认 | 距离等于设置 | verified | 窗口=卡片+8px gutter，卡片不再有 112px 左缩进；单测 1000,300+20 -> 1020,320 | 4 |
| O-03 | 左/上翻转 | 同逻辑距离 | verified | 按真实卡片尺寸翻转；单测右边界 1800 -> 1460、下边界 900 -> 480 | 4 |
| O-04 | 右/下边缘 clamp | 不遮词、不越界 | verified | 统一 12px 屏边 clamp；单测角点 5,5 -> 25,25 | 4 |
| O-05 | 多屏/负坐标 | 正确 monitor/scale | partial | 符号坐标单测 -1500 -> -1480、-350 翻转 -690；待双屏/缩放实测 | 4 |
| O-06 | 滑块实时移动 | 不重新悬停 | partial | worker 20ms 轮询双轴 offset 并用新卡片尺寸重排；待装机实测 | 4 |
| O-07 | 设置页预览 | 实时反映距离 | verified | 预览读取双轴 CSS 变量（0.35 比例）；SettingsWindow 测试覆盖 120/80 | 4 |
| O-08 | 透明 gutter 点击 | 不挡下层、不算卡片内 | partial | 卡片 bounds 与 8px gutter 分离，worker 切换 ignore_cursor_events；待物理点击验证 | 4 |
| O-09 | 卡片拖动后设置保存 | 不跳回词锚点 | partial | 拖动期间按新位置重算 anchor，保存不再用旧锚点；单测 anchor 往返；待装机验证 | 4 |

## 5. 诊断与交付

| ID | 场景 | 期望 | 当前状态 | 证据 / 根因 | 目标 Phase |
| --- | --- | --- | --- | --- | --- |
| G-01 | 每层耗时/失败原因 | UIA/MSAA/Win32/OCR 全展示 | partial | report/stages 有，OCR crop/候选不足 | 5 |
| G-02 | 用户纠正 | 本地保存正确词 | open | 无提交入口 | 5 |
| G-03 | 真实鼠标证据库 | 每个失败样例有 fixture | open | 目前多为临时截图 | 0–5 |
| G-04 | 安装版验收 | D:\Eneye 实际运行 | open | 必须在最终 Phase 重装验证 | 5 |
| G-05 | 全量门禁 | verify/test/clippy/fmt/build | verified（基线） | 当前 84 Rust + 30 frontend 通过 | 每 Phase 重跑 |

## 6. 测试矩阵

### 6.1 自动测试层级

| 层级 | 位置 | 覆盖 | 运行方式 |
| --- | --- | --- | --- |
| 纯函数单测 | Rust `src/**` | 归一化、完整性、评分、placement、redirect、stream parser | `cargo test` |
| 截图 fixture | `src-tauri/tests/fixtures/` | OCR 真实渲染、词框、候选 | `cargo test` |
| 真实窗口 E2E | `src-tauri/tests/` 或 ignored | Notepad/Chrome/PDF 实际窗口 | 手动/夜间 |
| 前端组件 | `src/**/*.test.tsx` | 事件链、设置、折叠、预览 | `npm test` |
| Mock AI | 本地 loopback server | 流式/错误/取消/重试 | `npm run test:e2e:mock` |
| 安装版真实鼠标 | 外部脚本 + 截图 | 最终验收 | 手动 |

### 6.2 必测环境

- Windows 11 记事本（纯文本、未保存、换行）。
- Chrome/Edge 普通网页 + 引号/括号/下划线。
- Calibre 阅读器。
- PDF 文本层 + 扫描 PDF/PNG。
- Office/WordPad。
- 100/150/200% DPI。
- 主屏 + 负坐标副屏 + 右/下边缘。

### 6.3 验收数字

- curated 回归集 top-1 ≥ 98%，虚构 headword = 0。
- OCR 首卡 p95 ≤ 1.2s，最差 ≤ 2.5s；不满足时隐藏而非返回短词。
- 悬浮窗距离误差 ≤ 4 physical px（0/20/64/120/240）。
- 所有 AI 错误场景都有明确文案；取消不是错误。
- 每个 Phase 结束 `npm run verify` + Rust tests + clippy + fmt 通过，并独立 commit。
