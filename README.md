# Eneye

Eneye 是一个 Windows 跨应用划词翻译器。它读取当前屏幕中可见的文字，因此可用于浏览器、PDF 阅读器、Office、记事本和图片查看器，而不依赖特定文件格式。

## 当前功能

- 鼠标稳定悬停约 240ms 自动查询英文单词，延迟可在 100–500ms 间调整。
- 优先使用 Windows UI Automation；无法直接读取时使用局部屏幕 OCR。
- 悬浮卡片可拖动；移开后保留时间可调到 0ms（立即收起）。
- 拖选至少两个拉丁词或四个连续 CJK 字符后，自动调用 AI 翻译。
- 支持 OpenAI Chat Completions 和 Anthropic Messages，可配置 DeepSeek、OpenAI、Anthropic 或兼容地址。
- API Key 保存在 Windows 凭据管理器，不写入设置文件。
- 内置完整 ECDICT；支持导入 CSV、StarDict 和 MDX/MDD，外部词典引用原文件。
- 本地生词本：内置生词本、好词本、坏词本，可继续新建；在悬浮卡片里一键收藏，也可搜索和移除单词。
- 查询历史仍保留在本地（最多 10,000 条），AI 译文与查询记录不会被生词本操作删除。
- 支持系统明暗主题、可选本机字体和减少动态效果。

## 快捷键

- `Ctrl+Alt+E`：打开 Eneye 设置。
- `Ctrl+Alt+T`：翻译当前应用中的选中文字，供不支持鼠标选区检测的场景使用。

## 开发环境

- Windows 10 22H2 或 Windows 11 x64
- Node.js 24+
- Rust stable MSVC、rustfmt、clippy
- Visual Studio C++ Build Tools 与 Windows SDK
- Python 3.10+，仅用于生成词典数据库

当前机器的工具链安装位置：

```text
D:\DevTools\Rust\cargo
D:\DevTools\Rust\rustup
D:\DevTools\Rust\targets\eneye
D:\DevTools\npm-cache
D:\DevTools\data\eneye\ecdict.csv
```

## 开发

```powershell
npm install
python scripts/build_ecdict.py
npm run tauri dev
```

## 验证

```powershell
npm run verify
cd src-tauri
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

## 构建

```powershell
npm run tauri build
```

NSIS 安装包使用当前用户安装模式，无需管理员权限。ECDICT SQLite 是构建资源，不提交到 Git；脚本会从固定的上游提交校验并生成它。

## OCR 可选功能

直接文本和多数浏览器/Office 场景无需额外安装。对于扫描 PDF、图片或不支持 UI Automation 的应用，需要在 Windows 语言设置中启用英文 OCR 可选功能。Eneye 的诊断页会显示当前状态并提供设置入口。

## 数据与隐私

- 单词查询和 OCR 在本机完成。
- AI 只接收用户当前拖选并触发的文字，不发送截图或周围段落。
- 不包含遥测、云同步或自动更新。
- 用户导入词典的授权由用户自行确认。

## 已知限制

- 不索引磁盘文件，只识别当前屏幕可见文字。
- DRM 黑屏区域、管理员权限窗口中未授权的内容无法读取。
- 不支持旧版 MDX v1.2、Salsa20 加密头及部分 Compact 样式词典。
- 个人 MVP 安装包未进行代码签名，Windows 可能显示 SmartScreen 提示。
