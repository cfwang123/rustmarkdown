# Changelog / 更新日志

All notable changes to rustmarkdown are documented here. / 本文档记录 rustmarkdown 的重要变更。
Format based on [Keep a Changelog](https://keepachangelog.com/). / 格式基于 Keep a Changelog。

## unreleased

### English

#### Added

- Help → **Check for Updates...**: queries GitHub Releases, downloads the `.7z` (progress, cancel), then quits, replaces the install directory and restarts — GitHub API queried directly; updater runs as `--apply-update` CLI (wait old process → extract `tmp/update_extract` → overwrite, skipping tmp/target/log/.git). Startup auto-check on the interval (default 7 days, 0 = off; `updateCheckDays` / `lastUpdateCheck` in settings.json, written only after a successful check).
- Preview: double-click a long fenced code block (incl. trailing blanks) toggles expand/collapse; switching source / split / preview keeps the top-of-screen line aligned.

### 中文

#### 新增

- 帮助菜单「检查更新...」：查 GitHub Releases、下载 `.7z`（带进度、可取消）后退出并自动覆盖安装目录、完成后重启（对齐 ScreenKit `AppUpdater`）——GitHub API 直连；更新器为 `--apply-update` 命令行（等旧进程退出 → 解压 `tmp/update_extract` → 覆盖，跳过 tmp/target/log/.git）。启动时按间隔自动检查（默认 7 天、0 关闭；间隔与上次检查时间写 `settings.json`，成功检查后才更新时间）。
- 预览长代码块双击正文（含行尾空白）展开/收起；切换源码 / 双栏 / 预览时保持屏幕顶部那一行对齐。

## v1.0.2 (2026-09-02)

### English

- Source/preview double-click selects by CJK/Latin punctuation, triple-click selects the visual line, selections trim leading/trailing blanks (no more whole-paragraph double-click).
- Find: F3 / Shift+F3 also jump while the find box is focused, changing the keyword jumps to the first hit; Excel searches cell by cell; Excel scrollbar drag no longer selects the last column.
- XLS / XLSX / XLSM read-only preview (virtual grid): sheet tabs, frozen row/column headers, drag-select copy as TSV, Ctrl+wheel zoom, PgUp/PgDn switches sheets, outline lists sheet names.
- PDF preview no longer idles: render visible pages only, coalesce stale rasters, stretch the old bitmap while zooming, drop off-screen textures; fast 1000-page scrollbar drag (page-top binary search, pause raster while dragging); double-click on text pages no longer opens the image overlay (100% ⇄ fit width).
- DOC / DOCX read-only preview rebuilt: `office_oxide` layout model paginated directly (no Markdown round-trip), keeps headings / sizes / bold / colors / alignment / numbering / tables / images; Save As exports `.md`.
- Session restore opens only the active tab's file; other tabs stay placeholders and load on switch (Vim-encrypted ones prompt for the password then).
- Source large selections look the same while dragging and after release (opaque selection fill, full rows); fixed type-flash and ~2 s drag/Ctrl+A stalls (independent Arc per frame, selection paints blue without recoloring glyphs, off-screen rows drop meshes but keep glyphs).
- Copying link text no longer inserts extra spaces; preview bare-text links accept only `http://` / `https://` (no more `k://` / `ftp://` drive misdetect); source bare URLs (drive, UNC, relative with extension) are Ctrl+click links.
- Preview drag-select can start in block gaps and short-line trailing blanks; inline `` `code` `` chips join the selection.
- Settings became a separate Windows dialog (GWLP_HWNDPARENT-owned, always in front, centered on open); Esc cancels input UIs (Vim password, unsaved close, quit, reload; IME off in the password box).
- Vim encryption (zip / blowfish / blowfish2): password prompt on open, save writes back with the original method / salt / seed, password in memory only; File menu shows "Vim password" for encrypted files to re-enter it.
- Tab right-click menu: close / close others / close all (save prompts for dirty tabs; close-others saves only the tabs being closed); hovering another top-level menu item switches to it; recent-file paths drop the `\\?\` prefix.
- Outline scroll fix (highlight syncs only when the document scrolls); Explorer: single-click select / double-click open, double-click folders to expand, toolbar (up/back/forward/refresh, absolute path bar), right-click "set as workspace"; darker folder names.
- Chinese / English switch (View menu / settings, stored in `uiLang`); Settings gained a master "enable logs" toggle (UI stalls written to `ui.log`).
- Source recoloring is incremental per line (fences from the start), paragraph Galleys tolerate 64px wrap jitter; Ctrl+A / back-forward / heading jumps scroll instantly; undoing to the saved text marks the tab clean.
- Inline `` `code` `` chips match the preview fill (incl. backticks); Ctrl+click `[text](#anchor)`, `[text](file.md)` / `file.md#anchor` jumps, `![alt](image)` opens the overlay.
- AccessKit disabled (Windows UIA froze ~1 s on 2000-line sources); `cargo build --release` copies the exe and sibling `*.dll` to `release/`.

### 中文

- 源码/预览双击按中英文标点扩词、三击选视觉行，选区 trim 两端空白（双击不再整段、分区舍入与预览一致）。
- 查找：F3 / Shift+F3 在查找框有焦点时也可跳转，改关键词定位首条；Excel 按单元格搜索并跳转；Excel 拖滚动条不再误选最后一列。
- XLS / XLSX / XLSM 只读预览（虚拟网格）：表页签、行列表头冻结、拖选复制 TSV、Ctrl+滚轮缩放、PgUp/PgDn 换表、大纲为表名。
- PDF 预览不再空转卡顿（只画可见页、合并过时光栅、缩放拉伸旧图、视口外丢纹理，对照 SumatraPDF）；千页拖滚动条快速（页顶二分排可见页、拖条暂停光栅）；文本页双击不再当图片弹层（100% ⇄ 适宽）。
- DOC / DOCX 只读预览重做：`office_oxide` 排版模型直接分页绘制（不再转 Markdown），保留标题/字号/加粗/颜色/对齐/列表编号/表格/图片，另存可导出 `.md`。
- 启动恢复多标签时只真打开当前标签，其它占位、切换再读盘（Vim 加密的切换时再弹密码）。
- 源码大选区拖选/松手外观一致（静止叠画用不透明选区色、整行铺满）；打一字闪白、长文拖选/Ctrl+A 卡约 2 秒等修复（掏空独立 Arc、选区只铺蓝底不改字色、视口外丢网格保字形）。
- 预览复制链接文字不再多出空格；预览裸文本只认 `http://` / `https://` 为网址（`k://`、`ftp://` 不再误判盘符）；源码裸网址/路径（盘符、UNC、带扩展名相对路径）为超链接 Ctrl+点击跳转。
- 预览拖选：块间空隙、短行右侧空白可起选；行内 `` `代码` `` 灰底参与选区。
- 参数设置改独立 Windows 窗口（GWLP_HWNDPARENT 挂主窗口、始终在前、打开居中）；输入界面 Esc 取消（含 Vim 密码、未保存关闭、退出、重载等；密码框关 IME）。
- 支持 Vim 加密（zip / blowfish / blowfish2）：打开弹密码、保存按原方式与 salt/seed 写回，密码仅内存；文件菜单在加密文件打开时显示「Vim密码」可重新输入。
- 标签右键「关闭 / 关闭其它 / 关闭全部」（脏标签保存提示，关闭其它只存待关标签）；主菜单悬停其它标题即切换（对齐 WinForm/WPF）；历史文件路径去 `\\?\` 前缀。
- 大纲侧栏滚动修复（仅正文滚动位置变化才同步高亮）；资源管理器：单击选中/双击打开、子文件夹双击展开收起、工具栏（上一级/后退/前进/刷新/绝对路径栏）、右键设为工作目录；文件夹名深棕。
- 界面中英文切换（查看菜单 / 参数设置，写入 `uiLang`）；参数设置加「启用日志」总开关（UI 卡顿写 `ui.log`）。
- 源码着色按行增量（围栏从开头重做）、段落 Galley 折行 64px 容差；Ctrl+A/后退前进/章节跳转无滚动动画；撤销回打开/保存时正文标签恢复未修改。
- 行内 `` `代码` `` 灰底与预览一致（淡灰含两侧反引号）；`[文字](#锚点)`、`[文字](file.md)` 及 `file.md#锚点` Ctrl+点击跳转，`![alt](图)` 弹层预览。
- 关闭 AccessKit（Windows 无障碍接口导致两千行源码卡约 1 秒）；`cargo build --release` 后把 exe 与同目录 `*.dll` 复制到 `release/`。

## v1.0.1 (2026-08-28)

### English

- Source task boxes `[ ]` / `[x]` get light-gray / light-green fills; monospace prefers Consolas (headings/bold use the same mono bold), line height 1.45, baselines aligned with YaHei.
- Preview CJK/Latin mixed text splits by script and bottom-aligns per line (Latin Ubuntu baseline shifted down); source uses the same baseline.
- Code-fold footer ellipsis changed to `...` (old `⋯` was missing from the font and showed as a hollow square).
- Opening a file while an instance is running forwards to that window as a new tab and brings it forward; reopening the same file restores mode and position (remembered after close, debounced save in `session.json`).
- Image read-only preview: contain-fit, wheel zoom, drag pan, double-click fit ⇄ 100%, `[`/`]` rotate, Ctrl+C / right-click copy, Save As png/jpg/bmp.
- Sidebar tree selects whole rows (no text drag); session remembers the workspace root; build / run on Windows kills a running `rustmarkdown.exe` first.

### 中文

- 源码任务框 `[ ]` / `[x]` 淡灰/淡绿底；等宽字体优先 Consolas（标题/粗体走等宽粗体），源码行距 1.45、编码基线对齐雅黑。
- 预览中英混排按汉字/拉丁脚本拆分 Label 底对齐（拉丁 Ubuntu 下移基线）；源码同基线处理。
- 代码块折叠底栏省略号改 `...`（原先 `⋯` 缺字形显示空心方块）。
- 已有实例时再打开文件转到该窗口新标签并前置；重开同一文件恢复模式与位置（关标签后仍记住，session.json 防抖保存）。
- 图片文件只读预览：居中适应、滚轮缩放、拖拽平移、双击适合/100%、`[` `]` 旋转、Ctrl+C/右键复制、另存 png/jpg/bmp。
- 侧栏目录树点击选中整项（不拖选文字）；会话记住工作区根目录；Windows 下 build / run 自动结束运行中的 `rustmarkdown.exe`。

## 0.1.0 (2026-08-26 ~ 28)

### English

- M1 skeleton (eframe/egui, `wins: Vec<Win>`, main window only); multi-tab (open/close/switch/path de-dupe/drag reorder/middle-click close/unsaved prompt/Ctrl+Shift+T reopen); three-mode shell (source / side-by-side / preview, Ctrl+1/2/3, Ctrl+E); open / save / save-as, drag-and-drop, CLI paths.
- M2: MdParser ported (blocks/inlines/tables/details/img, source line mapping); native preview (heading numbers, lists, task checks, quotes, syntect code, table widths, images, links); 180ms debounced reparse; `--selftest`.
- App icon (window/taskbar/exe resource) with GUI subsystem (no console); `.lnk` support (incl. CJK target decoding fix); settings (tab width 2/3/4/8, heading numbers, max image width).
- Preview image double-click overlay (fit area / wheel zoom / pan / Esc or backdrop close, right-click copy image/file); preview/editor scrollbars flush right with fixed expanded width, faster wheel.
- Preview tables as a continuous grid (header fill, shared borders, top-aligned cells); fenced-code highlight (two-face, common languages and aliases, `text`/`log` uncolored); ` ```mermaid ` in pure Rust (source fallback on failure).
- Top menu bar / line-icon toolbar (hover shows name and shortcut); fold by heading, fenced code over 10 lines starts folded ("show all"); read-only task lists, inline `<font color/style>` styles.
- Markdown source highlight (vim-like: heading level colors, gray markers, links, syntect fences); source wraps by pane width on a pure-white background; real bold (YaHei Bold + mono bold).
- Wheel still scrolls while dragging a selection; image overlay lost its black strip (close button only); fixed quote-bar overdraw, missing preview scrollbar, reversed right-aligned table text (LTR + left padding).
- No-arg launch restores last files and modes (`session.json`), CLI paths open by argument; left outline sidebar (F4, filter highlight, level folding, scroll sync, click jump).
- Side-by-side synced scroll (650ms echo suppression), blue bar on the caret block, cross-pane selection tints; after Ctrl+Z the caret stays at the change.
- Version in the window title; back / forward (outline, anchors, in-doc relative links; cap 50; Alt+←/→).
- DOC / DOCX read-only (office_oxide → Markdown → A4 pages; original never overwritten; Save As exports `.md`); PDF read-only (pdfium raster + text, lazy visible pages, yellow selection, Ctrl+C / right-click copy text, encrypted PDFs report failure).
- Encoding auto-detect (UTF-8 BOM / UTF-16 / GBK) with save-back; external-change watch with auto reload (prompt if unsaved edits); tab right-click "move to new window" / tear-off and merge.
- File menu "recent files" (last 20, de-duped, newest first); folder tree starts collapsed; PDF/Word zoom (Ctrl+wheel / Ctrl++- / Ctrl+0) with status percentages; Word pages centered; PgUp/PgDn pages in preview.

### 中文

- M1 骨架（eframe/egui、`wins: Vec<Win>`、仅主窗口）；多标签（打开/关闭/切换/路径去重/拖动排序/中键关闭/未保存询问/Ctrl+Shift+T 重开）；三模式壳（代码/侧边预览/预览，Ctrl+1/2/3、Ctrl+E）；打开/保存/另存、拖放、命令行传路径。
- M2 移植 MdParser（块/行内/表格/details/img、源行映射）；预览原生渲染（标题编号、列表、任务勾选、引用、code syntect、表格列宽、图片、链接）；编辑 180ms 防抖重解析；`--selftest` 自检。
- 程序图标（窗口/任务栏/exe 资源）与 GUI 子系统（无控制台）；支持 `.lnk`（含中文目标解码修复）；参数设置（Tab 宽度 2/3/4/8、标题编号、图片最大宽）。
- 预览图片双击弹层（适合区域/滚轮缩放/拖拽/Esc 或点背景关闭，右键复制图片/文件）；预览/编辑滚动条贴右缘、固定展开宽、滚轮加速。
- 预览表格连续网格（表头底、共用边框、单元格顶对齐）；围栏代码高亮（two-face、常见语言与 `cs`/`py`/`yml` 等别名，`text`/`log` 不着色）；```mermaid``` 纯 Rust 渲染（失败显示源码）。
- 顶部菜单栏 / 线标图标工具栏（悬停显示功能名与快捷键）；预览按标题折叠、围栏代码超 10 行默认折叠（可「显示全部」）；任务列表只读、行内 `<font color/style>` 样式。
- Markdown 源码语法高亮（vim 风：标题分层色、标记灰、链接、围栏 syntect）；源码按窗格宽度折行、背景纯白；真加粗（雅黑 Bold 与等宽粗体）。
- 编辑区拖选时滚轮仍可滚动；图片弹层去黑条只留右上关闭；引用块竖线、预览滚动条缺失、表格右对齐文字倒序（改用 LTR + 左侧留白）等修复。
- 无参数启动恢复上次文件与模式（session.json），带路径按参数打开；左侧大纲侧栏（F4、筛选高亮、按级折叠、滚动同步、点击跳转）。
- 侧边预览双向同步滚动（650ms 抑制回传）、光标块左侧蓝条、交叉选区淡蓝底；Ctrl+Z 撤销后光标钉在改动处。
- 窗口标题栏显示版本号；后退/前进（大纲、锚点、文内相对链接，上限 50；Alt+←/→）。
- DOC / DOCX 只读预览（`office_oxide` 转 Markdown 后 A4 分页，不可覆盖、另存导出 `.md`）；PDF 只读预览（pdfium 光栅+抽字、可见页懒渲染、黄底选区、Ctrl+C/右键复制文字，加密提示失败）。
- 文本编码自动识别（UTF-8 BOM/UTF-16/GBK）并按原编码回写；外部修改监视自动重载（有未保存修改则询问）；标签右键「移到新窗口」/拖出拆窗、拖入合并。
- 文件菜单「历史文件」（最近 20、去重、新的在前）；文件夹目录树子文件夹默认收起；PDF/Word Ctrl+滚轮/Ctrl++- 缩放、Ctrl+0 复位、状态栏百分比；Word 页面水平居中；预览 PgUp/PgDn 翻页。