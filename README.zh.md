# rustmarkdown

Version **1.0.2**. [English](README.md) · [Changelog](CHANGELOG.md)

Windows 优先的 Markdown 预览 / 编辑器，Rust + egui 原生绘制，**不依赖浏览器内核**。三种视图：代码、侧边预览、预览。多标签，拖放打开文件 / 文件夹 / `.lnk`，也可只读打开 **DOC / DOCX、XLS·XLSX、PDF、图片**（对齐 docview）。

## 编辑与预览

- 三模式切换：代码 / 侧边预览 / 预览（Ctrl+1/2/3、Ctrl+E）；切换时保持屏幕顶部那一行对齐。
- Markdown GFM 子集解析（块带源行号），原生预览渲染：标题自动编号、按标题折叠、任务列表、连续网格表格、行内 `<font>` 样式；表格列宽对齐 mdview（短列钉死）。
- 围栏代码语法高亮（syntect + two-face，常见语言与别名）；` ```mermaid ` 纯 Rust 渲染图表（无浏览器，失败回退源码）。
- 编辑区按窗格宽度折行、纯白背景；增量排版（源码按行指纹、预览按块指纹）长文不卡；源码大选区只画视口。
- 查找（Ctrl+F、F3 / Shift+F3）：源码 / 预览高亮，Excel 按单元格搜索跳转；编码自动识别（UTF-8 / GBK / UTF-16 等）并按原编码保存；外部修改监视并提示重载。
- Vim 加密 Markdown（zip / blowfish / blowfish2）：打开弹密码、按原方式写回，密码只在内存。
- `[文字](#锚点)` / `[文字](file.md)` Ctrl+点击跳转；裸 `http(s)://` 网址与文件路径显示为下划线超链接；双击/三击按中英文标点扩选，复制挤掉汉字与拉丁之间多余空格。
- 撤销/重做光标钉在改动处，撤销回保存态标签变干净；输入界面 Esc 取消。

## 文档

- **DOC / DOCX 只读**：`office_oxide` 排版模型直接分页绘制（不再转 Markdown）；缩放、PgUp/PgDn、大纲跳转，另存可导出 `.md`。
- **PDF 只读**：pdfium 连续页、只画可见页（千页拖滚动条流畅）、Sumatra 式黄底文字选区、右键复制页图。
- **XLS / XLSX 只读**：calamine 虚拟网格、工作表页签、表头冻结、拖选复制 TSV、缩放、PgUp/PgDn 换表。
- **图片只读**：png/jpg/gif/bmp/ico/tif/webp，适合/100% 与光标居中缩放、拖拽平移、`[`/`]` 旋转、复制、另存；双击弹层预览。

## 标签、窗口与侧栏

- 多标签：拖动排序、中键关闭、右键关闭其它/全部、Ctrl+Shift+T 重开、历史文件（最近 20）；拖出拆窗、拖入合并（对齐 docview）。
- 会话恢复：上次文件 / 模式 / 滚动 / 工作区根目录 / 窗口宽高位置（`session.json`）；启动只真打开当前标签，其它切换时再读盘。
- 左侧侧栏（F4）：资源管理器目录树（懒加载、路径栏、上一级/后退/前进/刷新、设为工作目录）+ 大纲（筛选、滚动同步高亮、点击跳转）。
- 后退 / 前进（Alt+← / Alt+→）：大纲点击、锚点、文内链接；状态栏显示模式 / 行数 / 编码 / Tab 宽。

## 参数设置与更新

- 参数设置（Ctrl+,）：界面语言（中文 / English）、Tab 宽度、标题自动编号、图片最大宽度、日志开关、**检查更新间隔**。
- **检查更新**（帮助菜单）：查 GitHub Releases、下载 `.7z`（带进度）后退出并自动覆盖安装目录、完成后重启（对齐 ScreenKit）；启动时按间隔自动检查（默认 7 天、0 关闭）；GitHub 走本机代理 `127.0.0.1:7897`、失败直连。
- 窗口标题栏显示版本号；设置与历史文件存 `%LocalAppData%\rustmarkdown\`。

## 命令行

```text
rustmarkdown.exe [路径 ...] [选项]
```

| 选项 | 说明 |
|------|------|
| `路径` | 启动时打开文件 / 文件夹 |
| `--selftest` | 解析器 / 表格列宽自检（退出码 0 通过） |
| `--update-check` | 只查一次最新版本，结果写 `tmp/update_apply.log`（调试用） |
| `--apply-update <包>`、`--target <目录>`、`[--wait-pid <进程>]`、`[--restart]` | 更新器命令行：等 `--wait-pid` 退出 → 解压 `.7z` → 覆盖 `<目录>` → 可选重启（检查更新流程使用） |

## 要求

- Windows x64（其它平台可编译，未作为主测试目标）
- Rust 1.80+（建议 rustup stable）
- 打包与自更新：7-Zip（`7z` 在 PATH 中或安装到默认目录）；`node pack.js` 还需要 Node.js

## 编译与运行

```bat
cargo build --release
cargo run -- path\to\file.md
node pack.js
```

输出：`target/release/rustmarkdown.exe`。`node pack.js` 结束正在运行的 `rustmarkdown.exe` → Release 编译 → 生成 `release/rustmarkdown_x.x.x.7z`（exe、`pdfium.dll`、README.md、README.zh.md、CHANGELOG.md；版本号读 `Cargo.toml`）；`release/` 已 gitignore。exe 为 GUI 子系统，检查输出用 `cargo test` / `--selftest`。

## 快捷键

| 键 | 功能 |
|----|------|
| Ctrl+N | 新建 |
| Ctrl+O | 打开文件（Markdown / Word / Excel / PDF / 图片 / `.lnk`） |
| Ctrl+Shift+O | 打开文件夹（侧栏目录树工作区） |
| Ctrl+F | 查找 |
| F3 / Shift+F3 | 下一个 / 上一个查找命中 |
| Ctrl+S / Ctrl+Shift+S | 保存 / 另存为 |
| Ctrl+W | 关闭标签 |
| Ctrl+Shift+T | 重开最近关闭的标签 |
| Ctrl+Tab / Ctrl+Shift+Tab | 下一个 / 上一个标签 |
| Ctrl+1 / Ctrl+2 / Ctrl+3 | 代码 / 侧边预览 / 预览 |
| Ctrl+E | 预览 ↔ 上次编辑模式 |
| Ctrl+, | 参数设置 |
| Ctrl+Z / Ctrl+Y | 撤销 / 重做 |
| Ctrl+C | 复制（PDF 有文字选区时复制所选文字；表格复制所选单元格；图片标签复制整图） |
| Ctrl+滚轮、Ctrl++ / Ctrl+-、Ctrl+0 | PDF / Word / Excel / 图片缩放 / 恢复 100% |
| 滚轮（图片标签） | 缩放（无需 Ctrl；光标为中心） |
| [ / ] | 图片逆时针 / 顺时针旋转 90° |
| PgUp / PgDn | 预览翻页（PDF / Word 按页；Excel 换工作表；Markdown 按一屏） |
| ↑ ↓ ← → | 预览滚动（编辑区仍移动光标） |
| F4 | 目录侧栏开关 |
| Alt+← / Alt+→ | 后退 / 前进 |

## 项目结构

```text
src/
  main.rs     入口与命令行
  app.rs      窗口状态、菜单栏、工具栏、快捷键、拖放、跳转历史、更新流程
  i18n.rs     界面中英文文案
  nav.rs      后退 / 前进栈
  tabs.rs     标签栏（跟手排序 / 离条拆窗 / 拖入合并）
  workspace.rs 文件夹工作区目录树
  doc.rs      文档会话 / 标签 / 模式（Markdown / Word / Excel / PDF / 图片）
  parser/     Markdown 解析、表格列宽、标题编号
  view/       编辑器、预览渲染、Word 分页、Excel 网格、PDF 连续页（选字）、图片文件预览、查找条、大纲侧栏、MD 源码着色、围栏高亮、工具栏图标、字体
  io/         文件读写与编码检测、文件监视、Word 排版 IR、Excel 读表、PDF/pdfium 光栅化与抽字、图片缓存、Mermaid 渲染、.lnk 解析、参数设置、自更新
assets/       程序图标 icon.png / icon.ico
native/pdfium pdfium.dll（构建时复制到 exe 旁；不提交）
pack.js       编译 Release 并打包到 release/rustmarkdown_x.x.x.7z
```