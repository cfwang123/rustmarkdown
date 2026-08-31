# rustmarkdown

[English](README.md)

Windows 优先的 Markdown 预览 / 编辑器，用 Rust + egui 原生绘制，**不依赖浏览器内核**。
三种视图：代码、侧边预览、预览。支持多标签、拖放打开文件 / 文件夹 / `.lnk` 快捷方式。
也可只读打开 **DOC / DOCX / PDF / 图片**（对齐 docview）。

参考 DocviewWPF 的纯 WPF 渲染路线（解析块带源行号、三模式切换）。

## 当前进度（v1.0.1）

已实现：

- 多标签：打开 / 关闭 / 切换 / 按路径去重 / 拖动排序 / 中键关闭；长文件名裁切显示
- 顶部菜单栏（文件 / 查看 / 工具 / 帮助）与图标工具栏（悬停显示功能名与快捷键）
- 三模式切换（代码、侧边预览、预览；侧栏可拖分隔条）
- 打开、保存、另存为；未保存关闭询问（撤销回打开/保存时的正文则视为未修改）
- 拖放文件 / 文件夹 / `.lnk` 快捷方式打开（含中文路径）；文件夹进侧栏目录树；无路径时回退写入临时文件再打开；失败弹错误框；命令行传入路径。已有实例在跑时，再打开文件会转到该窗口用新标签打开（不新开进程）
- Markdown 解析（GFM 子集，块带源行号）与原生预览渲染（含 `**[链接](url)**` 等嵌套行内）
- 表格列宽对齐 mdview：按内容显示宽分配，短列钉死、长列分剩余；无空白短列不竖排拆字
- 围栏代码语法高亮（syntect + two-face：C/C++/C#、JS/TS、Python、Rust、Go、Java、Shell/PowerShell、SQL、JSON/YAML/TOML、HTML/CSS、Dockerfile 等；`cs`/`py`/`yml`/`ps1` 等别名）
- ` ```mermaid ` 围栏：纯 Rust 渲染流程图 / 时序图等（无浏览器内核；不支持的语法回退为源码 + 错误）
- 程序图标（窗口标题栏 / 任务栏 / exe 文件）
- Release / 直接运行 exe 不弹出控制台窗口
- 参数设置（Ctrl+,）：Markdown Tab 宽度、标题自动编号、图片最大显示宽度、日志总开关；保存在 `%LocalAppData%\rustmarkdown\settings.json`
- 无参数启动恢复上次关闭时打开的文件、视图模式与滚动位置（`session.json`），并恢复目录树工作区根目录；之后再次打开同一文件也会恢复上次的模式与位置；命令行传入路径则按参数打开（传入的是文件时仍恢复上次工作区）
- 图片双击弹层预览（滚轮缩放、拖拽平移、Esc / 点背景关闭）；右上仅关闭按钮，无顶栏黑条；右键复制图片 / 复制为文件
- 预览任务列表（`- [ ]` / `- [x]`）只读显示
- 预览表格为连续网格（表头底色、共用边框），不用单元格小框
- 预览/编辑滚动条贴窗口右边缘、固定展开宽度（不随悬停变窄）；滚轮滚动加快；拖选文字时滚轮仍有效
- 代码编辑区按窗格宽度 break-word 折行（长行不再横向滚动）；预览段落换行从行首排，不够放下整词则先填满当前行再拆；预览中英混排按脚本拆段并按行高底对齐（拉丁 Ubuntu 下移基线、汉字雅黑）；行内 `` `代码` `` 灰底在行高内上下居中；源码 Consolas 下移基线对齐雅黑；源码空行在编辑区和预览中都按行保留空隙
- 代码编辑区背景纯白 `#ffffff`
- 预览粗体 / 粗斜体 / 标题加粗（雅黑 Bold）；行内 `<font>` 色与字重
- Markdown 源码语法高亮（标题正文近黑、`#` 标记分层色、标记灰、任务框 `[ ]` 淡灰底 / `[x]` 淡绿底、行内 `` `代码` `` 与预览相同淡灰底、链接、围栏代码 syntect）；源码等宽优先 Consolas（接近 GVIM），标题与 `**粗体**` 用 Consolas Bold；行距为字号 × 1.45；围栏代码块整块灰底（铺满行宽）；拖选复用已排版结果（查找/预览映射底色叠画、不整篇重排；选区网格与段落缓存脱开，未选中行保持原色；选区只铺蓝底、不改字色，避免与段落缓存串色）；打字时只重着色脏行（围栏从开头重做），折行宽在 64px 内仍复用段落 Galley；源码中 `[文字](#锚点)` / `[文字](file.md)` 可 Ctrl+点击跳转（与预览相同）；`![alt](图)` Ctrl+点击打开图片弹层预览；已关闭 AccessKit，避免 Windows 无障碍接口在打开两千行源码数秒后卡约 1 秒
- 预览按段增量：AST 块指纹不含源行号，视口外未变块用缓存高度占位，可见块仍绘制（链接 / 折叠 / 自动序号）
- 预览按标题折叠内容；围栏代码超过 10 行默认折叠（Mermaid 不折），底部灰字行点击切换（`... <CR> collapse` / `expand`，对齐 mdview）；预览代码框拉满内容宽度
- 左侧侧栏（F4）：「资源管理器」文件夹树（懒加载，单击选中整项、不可拖选文字，双击打开文件；文件夹名深棕；工具栏上一级/后退/前进/刷新，路径栏可输入绝对路径；子文件夹双击展开/收起，右键「设为工作目录」）+ 「大纲」章节树；筛选、滚动高亮、点击跳转；宽度与开关写入 settings.json
- Ctrl+F 查找：不区分大小写，F3 / Shift+F3 上下一个；源码与预览高亮
- 文本编码自动检测（UTF-8 / GBK / UTF-16 等），保存按原编码；外部修改监视并提示重载
- 标签拖拽对齐 docview：条内跟手排序；拖离标签栏立刻拆成独立窗口并跟手（不必松手）；拖到其它窗口标签栏立即合并；右键「打开为工作目录」（该文件父目录进左侧栏）、「移到新窗口」仍可用。仅 1 个标签时不可拆窗
- 文件菜单「历史文件」：最近 20 个打开过的文件，写入 `%LocalAppData%\rustmarkdown\settings.json`
- 侧边预览双向同步滚动（滚轮/拖滚动条，程序化滚动后 650ms 只抑制对侧回传）
- 侧边预览：当前光标所在块左侧蓝条；编辑区有选区时状态栏显示「已选择 N 字」
- 状态栏显示模式 / 行数 / 编码 / Tab 宽（不显示完整文件路径）；打开、切换、保存提示只用文件名
- 侧边预览互相对应选区：左边选文字时右边对应文字淡蓝底；右边拖选时左边对应源码淡蓝底
- Ctrl+Z / Ctrl+Y 撤销重做后光标留在改动处，视口不再跳到更早的编辑位置；只记录输入/删除（移动光标不算）；换到其它行（含回车）后下一串输入单独成一步；撤销回打开/保存时的正文后标签变为未修改
- 预览表格右对齐 / 居中：文字靠右或居中，字序与源码一致（不用 RTL 布局，避免「斜体」变成「体斜」）
- 窗口标题栏显示版本号（如 `demo.md — rustmarkdown v1.0.1`）
- 后退 / 前进（工具栏、查看菜单、Alt+← / Alt+→）：大纲点击、`#锚点`、文内相对 Markdown 链接记入历史（上限 50）
- **DOC / DOCX 只读预览**（对齐 docview）：转为 Markdown 后按 A4 竖向分页（灰底白页、页间距 12）；Ctrl+滚轮 / Ctrl++- / Ctrl+0 缩放（1.0 = A4 100%）；PgUp/PgDn 翻页、方向键滚动；大纲可跳转；不可改原文件，可用「另存为」导出 `.md`
- **PDF 只读预览**（对齐 docview 连续页）：pdfium 按页光栅化，竖向一页接一页；打开默认 100%；Ctrl+滚轮 / Ctrl++- / Ctrl+0 缩放；PgUp/PgDn 翻页、方向键滚动；拖选文字为 Sumatra 式黄底高亮，Ctrl+C / 右键复制所选文字；大纲为页列表；双击放大、右键复制图片
- **图片文件只读预览**（对齐 docview ImageViewer）：打开 png / jpg / jpeg / gif / bmp / ico / tif / tiff / webp；打开时按窗口居中适应（contain）；滚轮缩放（光标为中心）、拖拽平移；双击适合窗口 ⇄ 100%；`[` / `]` 旋转 90°；Ctrl+C / 右键复制图片或复制为文件；另存为 png/jpg/bmp；不可覆盖原文件
- `node pack.js` 一键编译并打包到 `release/rustmarkdown_x.x.x.7z`

后续：深浅主题、缩放、按窗口会话恢复。

## 要求

- Windows x64（其它平台可编译，未作为主测试目标）
- Rust 1.80+（建议 rustup stable）
- 打包：Node.js、7-Zip（`7z` 在 PATH 中，或安装到默认目录）

## 编译与运行

```bat
cargo build --release
cargo run -- path\to\file.md
node pack.js
```

输出：`target/release/rustmarkdown.exe`。

改完代码后用 `cargo build --release`（不要 debug）。Windows 下编译 / `cargo run` 会在链接前结束正在运行的 `rustmarkdown.exe`（避免 exe 被占用）。

`node pack.js`：结束正在运行的 `rustmarkdown.exe` → `cargo build --release` → 生成 `release/rustmarkdown_x.x.x.7z`（版本号读 `Cargo.toml`；包内为 exe、`pdfium.dll`、README.md、README.zh.md、CHANGELOG.md）。`release/` 已记入 `.gitignore`，不提交。

`--selftest`：跑解析器 / 表格列宽自检（退出码 0 通过）。exe 为 GUI 子系统，建议用 `cargo test` 看输出：

```bat
cargo test
cargo run -- --selftest
```

## 快捷键

| 键 | 功能 |
|----|------|
| Ctrl+N | 新建 |
| Ctrl+O | 打开文件（Markdown / Word / PDF / 图片 / `.lnk`） |
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
| Ctrl+C | 复制（PDF 有文字选区时复制所选文字；图片标签复制整图） |
| Ctrl+滚轮、Ctrl++ / Ctrl+-、Ctrl+0 | PDF / Word / 图片缩放 / 恢复 100% |
| 滚轮（图片标签） | 缩放（无需 Ctrl；光标为中心） |
| [ / ] | 图片逆时针 / 顺时针旋转 90° |
| PgUp / PgDn | 预览翻页（PDF / Word 按页；Markdown 按一屏） |
| ↑ ↓ ← → | 预览滚动（编辑区仍移动光标） |
| F4 | 目录侧栏开关 |
| Alt+← / Alt+→ | 后退 / 前进 |

## 项目结构

```text
src/
  main.rs     入口与命令行
  app.rs      窗口状态、菜单栏、工具栏、快捷键、拖放、跳转历史
  nav.rs      后退 / 前进栈
  tabs.rs     标签栏（跟手排序 / 离条拆窗 / 拖入合并）
  workspace.rs 文件夹工作区目录树
  doc.rs      文档会话 / 标签 / 模式（Markdown / Word / PDF / 图片）
  parser/     Markdown 解析、表格列宽、标题编号
  view/       编辑器、预览渲染、PDF 连续页（选字）、图片文件预览、查找条、大纲侧栏、MD 源码着色、围栏高亮、工具栏图标、字体
  io/         文件读写与编码检测、文件监视、Word 转 MD、PDF/pdfium 光栅化与抽字、图片缓存、Mermaid 渲染、.lnk 解析、参数设置
assets/       程序图标 icon.png / icon.ico
native/pdfium pdfium.dll（构建时复制到 exe 旁；不提交）
pack.js       编译 Release 并打包到 release/rustmarkdown_x.x.x.7z
```
