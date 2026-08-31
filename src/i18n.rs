//! 界面中英文。当前语言存在原子量里，各处用 `t()` 取文案。

use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

const ZH: u8 = 0;
const EN: u8 = 1;
static CUR: AtomicU8 = AtomicU8::new(ZH);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Zh,
    En,
}

impl Default for Lang {
    fn default() -> Self {
        Lang::Zh
    }
}

impl Lang {
    fn as_u8(self) -> u8 {
        match self {
            Lang::Zh => ZH,
            Lang::En => EN,
        }
    }
}

pub fn get() -> Lang {
    if CUR.load(Ordering::Relaxed) == EN {
        Lang::En
    } else {
        Lang::Zh
    }
}

pub fn set(lang: Lang) {
    CUR.store(lang.as_u8(), Ordering::Relaxed);
}

pub fn t() -> &'static Tr {
    table(get())
}

pub fn table(lang: Lang) -> &'static Tr {
    match lang {
        Lang::Zh => &TR_ZH,
        Lang::En => &TR_EN,
    }
}

fn r1(tpl: &str, a: impl std::fmt::Display) -> String {
    tpl.replace("{0}", &a.to_string())
}

fn r2(tpl: &str, a: impl std::fmt::Display, b: impl std::fmt::Display) -> String {
    tpl.replace("{0}", &a.to_string())
        .replace("{1}", &b.to_string())
}

fn r3(
    tpl: &str,
    a: impl std::fmt::Display,
    b: impl std::fmt::Display,
    c: impl std::fmt::Display,
) -> String {
    tpl.replace("{0}", &a.to_string())
        .replace("{1}", &b.to_string())
        .replace("{2}", &c.to_string())
}

macro_rules! tr_def {
    ($($name:ident: $zh:expr, $en:expr;)*) => {
        #[allow(dead_code)]
        pub struct Tr {
            $(pub $name: &'static str,)*
        }
        static TR_ZH: Tr = Tr { $($name: $zh,)* };
        static TR_EN: Tr = Tr { $($name: $en,)* };
    };
}

tr_def! {
    lang_zh: "中文", "中文";
    lang_en: "English", "English";
    menu_file: "文件", "File";
    menu_view: "查看", "View";
    menu_tools: "工具", "Tools";
    menu_help: "帮助", "Help";
    menu_language: "语言", "Language";
    new: "新建", "New";
    open_ellipsis: "打开...", "Open...";
    open_folder_ellipsis: "打开文件夹...", "Open Folder...";
    recent_files: "历史文件", "Recent Files";
    recent_empty: "（空）", "(empty)";
    clear_recent: "清除历史", "Clear Recent";
    save: "保存", "Save";
    save_as: "另存为...", "Save As...";
    close: "关闭", "Close";
    close_all: "关闭全部", "Close All";
    close_others: "关闭其它", "Close Others";
    close_others_confirm: "有未保存的更改。关闭其它标签？", "There are unsaved changes. Close other tabs?";
    closed_others: "已关闭其它标签", "Closed other tabs";
    vim_password: "Vim密码", "Vim Password";
    vim_password_prompt: "这是 Vim 加密文件，请输入密码：", "This file is Vim-encrypted. Enter its password:";
    vim_unsupported_method: "不支持的 Vim 加密方式（支持 zip / blowfish / blowfish2）", "Unsupported Vim encryption method (supported: zip / blowfish / blowfish2)";
    vim_not_encrypted: "不是 Vim 加密文件", "Not a Vim-encrypted file";
    vim_password_ok: "已用新密码重新解密", "Re-decrypted with the new password";
    reopen_tab: "重新打开关闭的标签", "Reopen Closed Tab";
    copy_file_path: "复制文件路径", "Copy File Path";
    reveal_in_explorer: "在资源管理器中显示", "Show in Explorer";
    exit: "退出", "Exit";
    back: "后退", "Back";
    forward: "前进", "Forward";
    find: "查找", "Find";
    mode_code: "代码", "Source";
    mode_side: "侧边预览", "Split Preview";
    mode_preview: "预览", "Preview";
    toggle_preview_edit: "预览 / 编辑切换", "Toggle Preview / Edit";
    sidebar: "目录侧栏", "Sidebar";
    settings_ellipsis: "参数设置...", "Settings...";
    about_app: "关于 rustmarkdown", "About rustmarkdown";
    tip_new: "新建 (Ctrl+N)", "New (Ctrl+N)";
    tip_open: "打开 (Ctrl+O)", "Open (Ctrl+O)";
    tip_save: "保存 (Ctrl+S)", "Save (Ctrl+S)";
    tip_save_as: "另存为 (Ctrl+Shift+S)", "Save As (Ctrl+Shift+S)";
    tip_back: "后退 (Alt+←)", "Back (Alt+←)";
    tip_forward: "前进 (Alt+→)", "Forward (Alt+→)";
    tip_code: "代码 (Ctrl+1)", "Source (Ctrl+1)";
    tip_side: "侧边预览 (Ctrl+2)", "Split Preview (Ctrl+2)";
    tip_preview: "预览 (Ctrl+3)", "Preview (Ctrl+3)";
    tip_sidebar: "目录侧栏 (F4)", "Sidebar (F4)";
    tip_settings: "参数设置 (Ctrl+,)", "Settings (Ctrl+,)";
    ready: "就绪", "Ready";
    untitled: "未命名", "Untitled";
    untitled_md: "未命名.md", "Untitled.md";
    unsaved: "未保存", "Unsaved";
    explorer: "资源管理器", "Explorer";
    outline: "大纲", "Outline";
    no_folder: "未打开文件夹", "No folder opened";
    open_folder_btn: "打开文件夹…", "Open Folder…";
    parent_folder: "上一级", "Up";
    refresh: "刷新", "Refresh";
    abs_path: "绝对路径", "Absolute path";
    set_as_workspace: "设为工作目录", "Set as Workspace";
    open: "打开", "Open";
    copy_path: "复制路径", "Copy Path";
    open_as_workspace: "打开为工作目录", "Open as Workspace";
    move_to_new_window: "移到新窗口", "Move to New Window";
    chapter_list: "章节列表", "Headings";
    filter_chapters: "筛选章节…", "Filter headings…";
    no_headings: "当前文档无章节", "No headings in this document";
    empty_doc: "(空文档)", "(empty document)";
    expand: "展开", "Expand";
    collapse: "折叠", "Collapse";
    collapse_code: "收起", "Collapse";
    copy: "复制", "Copy";
    dblclick_preview_copy: "双击预览 · 右键复制", "Double-click to preview · right-click to copy";
    rendering_chart: "正在渲染图表…", "Rendering chart…";
    image: "图片", "Image";
    find_hint: "文本", "Text";
    no_match: "无匹配", "No matches";
    find_prev: "上一个", "Previous";
    find_next: "下一个", "Next";
    copy_image: "复制图片", "Copy Image";
    copy_as_file: "复制为文件", "Copy as File";
    copy_text: "复制文字", "Copy Text";
    zoom_out: "缩小", "Zoom out";
    zoom_in: "放大", "Zoom in";
    fit: "适合", "Fit";
    fit_area: "适合区域", "Fit to view";
    original_size: "原始大小", "Actual size";
    opening_pdf: "正在打开 PDF…", "Opening PDF…";
    opening_image: "正在打开图片…", "Opening image…";
    pdf_drag_hint: "拖选文字 · 双击预览 · 右键复制", "Drag to select · double-click preview · right-click copy";
    readonly: "只读", "read-only";
    ok: "确定", "OK";
    cancel: "取消", "Cancel";
    settings: "参数设置", "Settings";
    language: "界面语言", "Language";
    language_help: "立即切换菜单、工具栏和对话框的显示语言。确定后写入 settings.json。", "Switches menus, toolbar, and dialogs. Saved to settings.json when you click OK.";
    md_tab: "Markdown Tab", "Markdown Tab";
    tab_width_help: "Tab 宽度（字符）。列表缩进层级与预览缩进按此列宽计算。源码仍是单个 Tab，不会变成多个空格。默认 3。", "Tab width in characters. List indent and preview indent use this width. Source still stores a single Tab, not spaces. Default 3.";
    width_chars: "宽度（字符）", "Width (characters)";
    heading_auto_number: "标题自动编号", "Auto-number headings";
    heading_auto_help: "启用后在预览中为标题加 1 / 1.1 / 1.1.1 编号（不修改源文件）。默认开启。", "Adds 1 / 1.1 / 1.1.1 numbers in preview only (source unchanged). On by default.";
    img_max_width: "图片最大显示宽度", "Max image width";
    img_max_help: "限制预览里图片的显示宽度。0 表示不限制，按预览区宽度。HTML 上写的 width 更小时仍用较小值。", "Caps preview image width. 0 means no cap (follow the pane). A smaller HTML width still wins.";
    width: "宽度", "Width";
    img_unlimited: "不限制（随预览宽度）", "No limit (follow pane width)";
    enable_logs: "启用日志", "Enable logs";
    logs_help: "总开关：写入 UI 卡顿等到 %LocalAppData%\\rustmarkdown\\ui.log。默认关闭。", "Writes UI stalls to %LocalAppData%\\rustmarkdown\\ui.log. Off by default.";
    notes: "说明", "Notes";
    settings_notes: "设置保存在本机 %LocalAppData%\\rustmarkdown\\settings.json。\n标题编号仅影响预览显示，不修改源文件。", "Settings are stored in %LocalAppData%\\rustmarkdown\\settings.json.\nHeading numbers affect preview only and do not change the source file.";
    file_changed: "文件已更改", "File changed";
    reload_discard: "重载（丢弃修改）", "Reload (discard edits)";
    keep_edits: "保留编辑", "Keep edits";
    error: "错误", "Error";
    unsaved_changes: "未保存的更改", "Unsaved changes";
    dont_save: "不保存", "Don't Save";
    save_all: "保存全部", "Save All";
    close_without_save: "不保存关闭", "Close without Saving";
    quit_without_save: "不保存退出", "Quit without Saving";
    about: "关于 rustmarkdown", "About rustmarkdown";
    about_line1: "Windows 优先的原生 Markdown 预览 / 编辑器。", "A Windows-first native Markdown preview / editor.";
    about_line2: "不依赖浏览器内核。", "No browser engine.";
    quit: "退出", "Quit";
    filter_docs: "文档", "Documents";
    filter_images: "图片", "Images";
    filter_shortcut: "快捷方式", "Shortcuts";
    filter_all: "所有文件", "All files";
    welcome_hint: "拖放 Markdown / Word / PDF / 图片、文件夹或快捷方式（.lnk）到此窗口，或使用「文件」菜单 / 工具栏。", "Drop Markdown / Word / PDF / images, a folder, or a shortcut (.lnk) here, or use the File menu / toolbar.";
    welcome_keys: "Ctrl+O 打开   Ctrl+F 查找   Ctrl+Shift+O 文件夹   Ctrl+1/2/3 模式   F4 侧栏", "Ctrl+O Open   Ctrl+F Find   Ctrl+Shift+O Folder   Ctrl+1/2/3 Mode   F4 Sidebar";
    drop_to_open: "松开以打开文件", "Drop to open";
    code_fold_collapse: "... 收起", "... collapse";
    no_ext: "(无扩展名)", "(no extension)";
    restored: "已恢复上次打开的 {0} 个文件", "Restored {0} files from last session";
    path_missing: "路径不存在：{0}", "Path not found: {0}";
    not_folder: "不是文件夹：{0}", "Not a folder: {0}";
    opened_folder: "已打开文件夹 {0}", "Opened folder {0}";
    untitled_no_parent: "未保存的标签没有父目录", "Unsaved tab has no parent folder";
    no_parent: "无法取得父目录", "Cannot get parent folder";
    switched_to: "已切换到 {0}", "Switched to {0}";
    unsupported_type: "不支持的文件类型：{0}", "Unsupported file type: {0}";
    opened: "已打开 {0}", "Opened {0}";
    file_missing_named: "文件不存在：{0}", "File not found: {0}";
    new_untitled: "新建未命名文档", "New untitled document";
    no_reopen: "没有可重开的标签", "No closed tab to reopen";
    reopened_tab: "已重开标签", "Reopened tab";
    closed_tab: "已关闭标签", "Closed tab";
    closed_all: "已关闭全部标签", "Closed all tabs";
    doc_not_saved: "当前文档尚未保存到文件", "This document is not saved to a file yet";
    copied_path: "已复制文件路径", "Copied file path";
    copy_path_fail: "复制路径失败：{0}", "Failed to copy path: {0}";
    file_missing: "文件不存在", "File not found";
    cannot_open_explorer: "无法打开资源管理器", "Cannot open Explorer";
    cannot_open_dir: "无法打开所在目录", "Cannot open containing folder";
    cleared_recent: "已清除历史文件", "Cleared recent files";
    find_status: "查找 {0}/{1}", "Find {0}/{1}";
    pdf_readonly_save: "PDF 为只读预览，无法保存", "PDF is read-only and cannot be saved";
    image_readonly_save: "图片为只读预览，请用「另存为」导出", "Image is read-only; use Save As to export";
    word_readonly_save: "Word 为只读预览，请用「另存为」导出 Markdown", "Word is read-only; use Save As to export Markdown";
    saved: "已保存 {0}", "Saved {0}";
    opened_n_files: "已在当前窗口打开 {0} 个文件", "Opened {0} files in this window";
    drop_no_content: "无法读取拖入的文件（没有路径也没有内容）", "Cannot read dropped file (no path or content)";
    tmp_dir_fail: "无法创建临时目录：{0}", "Cannot create temp folder: {0}";
    tmp_write_fail: "无法写入临时文件：{0}", "Cannot write temp file: {0}";
    settings_saved: "已保存参数设置", "Settings saved";
    moved_window: "已移到新窗口", "Moved to a new window";
    merged_window: "已合并到窗口", "Merged into window";
    cannot_open: "无法打开：{0}", "Cannot open: {0}";
    copied_path_ok: "已复制路径", "Copied path";
    bad_folder_path: "路径无效或不是文件夹", "Invalid path or not a folder";
    copied_n_chars: "已复制 {0} 字", "Copied {0} characters";
    image_not_ready: "图片尚未加载完成", "Image is still loading";
    image_saved: "图片已另存 {0}", "Image saved as {0}";
    copied_image: "已复制图片", "Copied image";
    copy_image_fail: "复制图片失败：{0}", "Failed to copy image: {0}";
    copied_as_file: "已复制为文件：{0}", "Copied as file: {0}";
    copy_as_file_fail: "复制为文件失败：{0}", "Failed to copy as file: {0}";
    cannot_load_image: "无法加载图片：{0}", "Cannot load image: {0}";
    loading_image: "正在加载图片…", "Loading image…";
    cannot_open_link: "无法打开链接：{0}", "Cannot open link: {0}";
    anchor_missing: "未找到锚点：#{0}", "Anchor not found: #{0}";
    cannot_restore: "无法回到该位置：{0}", "Cannot restore that location: {0}";
    cannot_restore_closed: "无法回到该位置（标签已关闭）", "Cannot restore that location (tab closed)";
    pdf_status: "PDF  第 {0}/{1} 页  {2}%", "PDF  p. {0}/{1}  {2}%";
    word_status: "{0}  第 {1}/{2} 页  只读  {3}%", "{0}  p. {1}/{2}  read-only  {3}%";
    image_readonly_status: "{0}  只读", "{0}  read-only";
    n_lines: "{0} 行", "{0} lines";
    n_selected: "已选择 {0} 字", "{0} selected";
    file_changed_dirty: "「{0}」在磁盘上已更改，且当前有未保存修改。", "\"{0}\" has changed on disk and has unsaved edits.";
    unsaved_named: "「{0}」有未保存的更改。", "\"{0}\" has unsaved changes.";
    close_all_confirm: "有未保存的更改。确定关闭全部标签？", "There are unsaved changes. Close all tabs?";
    quit_confirm: "有未保存的更改。确定退出？", "There are unsaved changes. Quit?";
    version: "版本 {0}", "Version {0}";
    reloaded: "已从磁盘重载 {0}", "Reloaded {0} from disk";
    mermaid_fail: "图表渲染失败：{0}", "Chart render failed: {0}";
    pdf_page_fail: "第 {0} 页无法渲染", "Page {0} failed to render";
    pdf_page_loading: "第 {0} 页…", "Page {0}…";
    page_n: "第 {0} 页", "Page {0}";
    cannot_open_image: "无法打开图片：\n{0}", "Cannot open image:\n{0}";
    close_esc: "关闭 (Esc) · {0}", "Close (Esc) · {0}";
    code_fold_more: "... 还有 {0} 行 · 展开", "... {0} more · expand";
    read_fail: "读取失败：{0}", "Read failed: {0}";
    read_fail_path: "读取失败：{0} ({1})", "Read failed: {0} ({1})";
    write_tmp_fail: "写入临时文件失败：{0}", "Failed to write temp file: {0}";
    cannot_overwrite: "无法覆盖原文件：{0}", "Cannot overwrite original file: {0}";
    save_fail: "保存失败：{0}", "Save failed: {0}";
    no_image_data: "没有可用的图片数据", "No image data available";
    cannot_encode_image: "无法编码图片", "Cannot encode image";
    word_parse_start: "无法启动 Word 解析：{0}", "Cannot start Word parser: {0}";
    word_parse_crash: "解析 Word 时崩溃（可能栈溢出或文档损坏）", "Word parser crashed (stack overflow or damaged file)";
    word_format: "无法识别 Word 格式：{0}", "Unrecognized Word format: {0}";
    word_empty: "Word 文档是空文件", "Word document is empty";
    word_open: "无法打开 Word 文档：{0}", "Cannot open Word document: {0}";
    word_cache: "无法创建预览缓存：{0}", "Cannot create preview cache: {0}";
    word_read: "无法读取 Word 文档：{0} ({1})", "Cannot read Word document: {0} ({1})";
    word_read_fail: "读取 Word 文档失败：{0}", "Failed to read Word document: {0}";
    pdfium_load: "无法加载 pdfium.dll（{0}）：{1}", "Cannot load pdfium.dll ({0}): {1}";
    pdfium_sym: "pdfium 缺少符号 {0}：{1}", "pdfium missing symbol {0}: {1}";
    pdfium_missing: "找不到 pdfium.dll（应与 exe 同目录，或 native/pdfium/）", "pdfium.dll not found (place it next to the exe, or in native/pdfium/)";
    pdf_empty: "PDF 为空", "PDF is empty";
    pdf_open_fail: "无法打开 PDF（格式错误或已加密）", "Cannot open PDF (invalid or encrypted)";
    pdf_no_pages: "PDF 没有页面", "PDF has no pages";
    pdf_page_range: "页码超出范围", "Page out of range";
    pdf_page_load: "无法加载页面", "Cannot load page";
    pdf_bitmap: "无法创建位图", "Cannot create bitmap";
    pdf_read: "无法读取 PDF：{0} ({1})", "Cannot read PDF: {0} ({1})";
    file_empty: "文件为空", "File is empty";
    decode_fail: "无法解码：{0}", "Cannot decode: {0}";
    unknown_arg: "未知参数：{0}", "Unknown argument: {0}";
}

pub fn restored(n: impl std::fmt::Display) -> String {
    r1(t().restored, n)
}
pub fn path_missing(p: impl std::fmt::Display) -> String {
    r1(t().path_missing, p)
}
pub fn not_folder(p: impl std::fmt::Display) -> String {
    r1(t().not_folder, p)
}
pub fn opened_folder(name: &str) -> String {
    r1(t().opened_folder, name)
}
pub fn switched_to(name: &str) -> String {
    r1(t().switched_to, name)
}
pub fn unsupported_type(p: impl std::fmt::Display) -> String {
    r1(t().unsupported_type, p)
}
pub fn opened(name: &str) -> String {
    r1(t().opened, name)
}
pub fn file_missing_named(name: &str) -> String {
    r1(t().file_missing_named, name)
}
pub fn copy_path_fail(e: impl std::fmt::Display) -> String {
    r1(t().copy_path_fail, e)
}
pub fn find_status(cur: usize, n: usize) -> String {
    r2(t().find_status, cur, n)
}
pub fn saved(name: &str) -> String {
    r1(t().saved, name)
}
pub fn opened_n_files(n: impl std::fmt::Display) -> String {
    r1(t().opened_n_files, n)
}
pub fn tmp_dir_fail(e: impl std::fmt::Display) -> String {
    r1(t().tmp_dir_fail, e)
}
pub fn tmp_write_fail(e: impl std::fmt::Display) -> String {
    r1(t().tmp_write_fail, e)
}
pub fn cannot_open(name: impl std::fmt::Display) -> String {
    r1(t().cannot_open, name)
}
pub fn copied_n_chars(n: usize) -> String {
    r1(t().copied_n_chars, n)
}
pub fn image_saved(name: &str) -> String {
    r1(t().image_saved, name)
}
pub fn copy_image_fail(e: impl std::fmt::Display) -> String {
    r1(t().copy_image_fail, e)
}
pub fn copied_as_file(p: impl std::fmt::Display) -> String {
    r1(t().copied_as_file, p)
}
pub fn copy_as_file_fail(e: impl std::fmt::Display) -> String {
    r1(t().copy_as_file_fail, e)
}
pub fn cannot_load_image(href: impl std::fmt::Display) -> String {
    r1(t().cannot_load_image, href)
}
pub fn cannot_open_link(href: &str) -> String {
    r1(t().cannot_open_link, href)
}
pub fn anchor_missing(frag: &str) -> String {
    r1(t().anchor_missing, frag)
}
pub fn cannot_restore(e: impl std::fmt::Display) -> String {
    r1(t().cannot_restore, e)
}
pub fn pdf_status(cur: impl std::fmt::Display, n: impl std::fmt::Display, pct: impl std::fmt::Display) -> String {
    r3(t().pdf_status, cur, n, pct)
}
pub fn word_status(fmt: &str, page: usize, n: usize, pct: i32) -> String {
    t().word_status
        .replace("{0}", fmt)
        .replace("{1}", &page.to_string())
        .replace("{2}", &n.to_string())
        .replace("{3}", &pct.to_string())
}
pub fn image_readonly_status(s: &str) -> String {
    r1(t().image_readonly_status, s)
}
pub fn n_lines(n: usize) -> String {
    r1(t().n_lines, n)
}
pub fn n_selected(n: usize) -> String {
    r1(t().n_selected, n)
}
pub fn file_changed_dirty(name: &str) -> String {
    r1(t().file_changed_dirty, name)
}
pub fn unsaved_named(name: &str) -> String {
    r1(t().unsaved_named, name)
}
pub fn version(v: &str) -> String {
    r1(t().version, v)
}
pub fn reloaded(name: &str) -> String {
    r1(t().reloaded, name)
}
pub fn mermaid_fail(err: impl std::fmt::Display) -> String {
    r1(t().mermaid_fail, err)
}
pub fn pdf_page_fail(n: usize) -> String {
    r1(t().pdf_page_fail, n)
}
pub fn pdf_page_loading(n: usize) -> String {
    r1(t().pdf_page_loading, n)
}
pub fn page_n(n: usize) -> String {
    r1(t().page_n, n)
}
pub fn cannot_open_image(err: &str) -> String {
    r1(t().cannot_open_image, err)
}
pub fn close_esc(title: &str) -> String {
    r1(t().close_esc, title)
}
pub fn code_fold_more(n: usize) -> String {
    r1(t().code_fold_more, n)
}
pub fn read_fail(e: impl std::fmt::Display) -> String {
    r1(t().read_fail, e)
}
pub fn read_fail_path(path: impl std::fmt::Display, e: impl std::fmt::Display) -> String {
    r2(t().read_fail_path, path, e)
}
pub fn write_tmp_fail(e: impl std::fmt::Display) -> String {
    r1(t().write_tmp_fail, e)
}
pub fn cannot_overwrite(e: impl std::fmt::Display) -> String {
    r1(t().cannot_overwrite, e)
}
pub fn save_fail(e: impl std::fmt::Display) -> String {
    r1(t().save_fail, e)
}
pub fn word_parse_start(e: impl std::fmt::Display) -> String {
    r1(t().word_parse_start, e)
}
pub fn word_format(ext: &str) -> String {
    r1(t().word_format, ext)
}
pub fn word_open(e: impl std::fmt::Display) -> String {
    r1(t().word_open, e)
}
pub fn word_cache(e: impl std::fmt::Display) -> String {
    r1(t().word_cache, e)
}
pub fn word_read(path: impl std::fmt::Display, e: impl std::fmt::Display) -> String {
    r2(t().word_read, path, e)
}
pub fn word_read_fail(e: impl std::fmt::Display) -> String {
    r1(t().word_read_fail, e)
}
pub fn pdfium_load(path: impl std::fmt::Display, e: impl std::fmt::Display) -> String {
    r2(t().pdfium_load, path, e)
}
pub fn pdfium_sym(name: &str, e: impl std::fmt::Display) -> String {
    r2(t().pdfium_sym, name, e)
}
pub fn pdf_read(path: impl std::fmt::Display, e: impl std::fmt::Display) -> String {
    r2(t().pdf_read, path, e)
}
pub fn decode_fail(e: impl std::fmt::Display) -> String {
    r1(t().decode_fail, e)
}
pub fn unknown_arg(arg: &str) -> String {
    r1(t().unknown_arg, arg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_lang() {
        assert_eq!(serde_json::to_string(&Lang::Zh).unwrap(), "\"zh\"");
        assert_eq!(serde_json::to_string(&Lang::En).unwrap(), "\"en\"");
        let t: Lang = serde_json::from_str("\"en\"").unwrap();
        assert_eq!(t, Lang::En);
    }

    #[test]
    fn tables() {
        assert_eq!(table(Lang::Zh).save, "保存");
        assert_eq!(table(Lang::En).save, "Save");
        assert_eq!(table(Lang::Zh).opened.replace("{0}", "a.md"), "已打开 a.md");
        assert_eq!(table(Lang::En).opened.replace("{0}", "a.md"), "Opened a.md");
    }
}
