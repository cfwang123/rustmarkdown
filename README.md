# rustmarkdown

[中文说明](README.zh.md)

A Windows-first Markdown preview / editor, drawn natively with Rust + egui. **No browser engine.**
Three views: source, side-by-side preview, and preview. Multi-tab; drag-and-drop files, folders, and `.lnk` shortcuts.
Also opens **DOC / DOCX / PDF / images** read-only (aligned with docview).

Follows the DocviewWPF native-render approach (block parse with source line numbers, three view modes).

## Status (v1.0.1)

Implemented:

- Multi-tab: open / close / switch / path de-dupe / drag reorder / middle-click close; long names are clipped
- Menu bar (File / View / Tools / Help; View → Language for Chinese / English) and icon toolbar (hover shows name and shortcut)
- Three modes (source, side-by-side, preview; resizable splitter)
- Open, save, save as; prompt on unsaved close (undoing back to the last saved/opened text clears the dirty flag)
- Drag-and-drop files / folders / `.lnk` (including CJK paths); folders load into the sidebar tree; missing paths fall back to a temp file; errors show a dialog; CLI paths. If an instance is already running, a new file opens as a tab in that window (no second process)
- Markdown parse (GFM subset, source line numbers) and native preview (including nested inlines such as `**[link](url)**`)
- Table column widths aligned with mdview: allocate by content width; short columns stay fixed, long columns share the remainder; no vertical CJK split on short empty columns
- Fenced-code highlight (syntect + two-face: C/C++/C#, JS/TS, Python, Rust, Go, Java, Shell/PowerShell, SQL, JSON/YAML/TOML, HTML/CSS, Dockerfile, and aliases such as `cs` / `py` / `yml` / `ps1`)
- ` ```mermaid ` fences: flowcharts / sequence diagrams in pure Rust (no browser; unsupported syntax falls back to source + error)
- App icon (title bar / taskbar / exe)
- Release / double-click exe: no console window
- Settings (Ctrl+,): UI language (Chinese / English), Markdown tab width, heading auto-number, max image width, master log switch; stored in `%LocalAppData%\rustmarkdown\settings.json`
- Launch with no args restores last files, view mode, and scroll (`session.json`), plus the explorer workspace root; reopening the same file also restores mode and position; CLI paths take precedence (a file argument still restores the last workspace)
- Double-click image overlay (wheel zoom, pan, Esc / click backdrop to close); close button only, no black title strip; right-click copy image / copy as file
- Preview task lists (`- [ ]` / `- [x]`) read-only
- Preview tables as a continuous grid (header fill, shared borders), not per-cell rounded boxes
- Preview/editor scrollbars flush to the window edge, fixed expanded width; faster wheel; wheel still works while selecting text; Ctrl+A, back/forward, and heading jumps snap with no scroll animation
- Source wraps by pane width (break-word; no horizontal scroll on long lines); preview wraps from the start of the line and splits a word only after filling the line; CJK/Latin in preview is split by script and bottom-aligned (Latin Ubuntu baseline shifted down, CJK YaHei); inline `` `code` `` gray chips are vertically centered in the line; source Consolas baseline shifted to match YaHei; blank source lines keep their height in the editor and in preview
- Source editor background solid white `#ffffff`
- Preview bold / bold-italic / heading weight (YaHei Bold); inline `<font>` color and weight
- Markdown source highlight (heading text near-black, `#` markers by level, gray markers, task boxes `[ ]` light-gray fill / `[x]` light-green fill, inline code gray fill like preview, fenced syntect); monospace prefers Consolas (GVIM-like); headings and `**bold**` use Consolas Bold; line height is font size × 1.45; fenced blocks get a full-width gray fill; drag-select reuses layout (find/preview-map tints are overlays, not a relayout); selection mesh is detached from the paragraph cache so unselected lines keep their colors; selection paints a blue fill without recoloring glyphs (avoids cache tint at the start of a range); long selections / drag / Ctrl+A drop off-screen meshes but keep glyphs so caret mapping stays valid; typing recolors only dirty lines (fences from their start) and reuses paragraph Galleys when wrap jitters within 64 px; Ctrl+click a source `[text](#anchor)` or `[text](file.md)` to jump (same as preview links); Ctrl+click `![alt](image)` opens the image overlay; AccessKit is off so Windows UIA does not freeze a 2000-line source view about 3–10 s after open
- Native preview: off-screen unchanged blocks keep cached height (AST fingerprints without source line numbers) so long documents are not fully rebuilt every frame; visible blocks still paint (links / fold / heading numbers)
- Preview fold by heading; fenced code over 10 lines starts folded (not Mermaid); click the gray footer to toggle (`... <CR> collapse` / `expand`, aligned with mdview); code boxes stretch to content width
- Left sidebar (F4): Explorer folder tree (lazy load; single-click selects the row, no text drag; double-click opens a file; darker folder names; toolbar up/back/forward/refresh and an editable absolute path; double-click a folder to expand/collapse; context menu Open / Set as workspace / Show in Explorer / Copy path) + Outline; filter, scroll highlight, click to jump; width and on/off in settings.json
- Ctrl+F find: case-insensitive; F3 / Shift+F3 next/prev; highlight in source and preview
- Encoding auto-detect (UTF-8 / GBK / UTF-16, …); save in original encoding; watch external changes and prompt reload
- Tab drag aligned with docview: reorder in-bar; tearing off the bar immediately creates a new window that follows the cursor; drop on another window’s tab bar to merge; context menu “Open as workspace” (file’s parent folder in the left sidebar) and “Move to new window”. Cannot tear off when there is only one tab
- File menu “Recent files”: last 20 paths, in `%LocalAppData%\rustmarkdown\settings.json`
- Side-by-side synced scroll (wheel / scrollbar; 650 ms suppress after programmatic scroll)
- Side-by-side: blue bar on the block under the caret; status bar shows “N selected” when the editor has a selection
- Status bar: mode / line count / encoding / tab width (no full path); open / switch / save toasts use the file name only
- Cross-pane selection: selecting on the left highlights the matching preview text; dragging on the right highlights matching source
- After Ctrl+Z / Ctrl+Y the caret stays at the change; the viewport does not jump to an older edit; only insert/delete enter the undo history (caret moves do not); moving to another line (including Enter) starts a new undo group; undoing back to the last saved/opened text marks the tab clean
- Preview table right / center align: text is aligned, glyph order matches source (no RTL, so “斜体” does not become “体斜”)
- Title bar shows the version (e.g. `demo.md — rustmarkdown v1.0.1`)
- Back / forward (toolbar, View menu, Alt+← / Alt+→): outline clicks, `#anchors`, and in-doc relative Markdown links (cap 50)
- **DOC / DOCX read-only** (aligned with docview): convert to Markdown and paginate as A4 portrait (gray canvas, white pages, 12 px gap); Ctrl+wheel / Ctrl++- / Ctrl+0 zoom (1.0 = A4 100%); PgUp/PgDn page, arrows scroll; outline jump; original file is not edited; Save As can export `.md`
- **PDF read-only** (aligned with docview continuous pages): pdfium raster per page, stacked vertically; open at 100%; Ctrl+wheel / Ctrl++- / Ctrl+0 zoom; PgUp/PgDn page, arrows scroll; text select is Sumatra-style yellow highlight; Ctrl+C / right-click copy selected text; outline is a page list; double-click zoom, right-click copy image
- **Image file read-only** (aligned with docview ImageViewer): png / jpg / jpeg / gif / bmp / ico / tif / tiff / webp; open contain-fit to the window; wheel zoom (cursor-centered), pan; double-click toggles fit ⇄ 100%; `[` / `]` rotate 90°; Ctrl+C / right-click copy image or as file; Save As png/jpg/bmp; does not overwrite the original
- `node pack.js` builds Release and writes `release/rustmarkdown_x.x.x.7z`

Later: light/dark theme, UI zoom, per-window session restore.

## Requirements

- Windows x64 (other platforms may build; not the primary test target)
- Rust 1.80+ (rustup stable recommended)
- Pack: Node.js, 7-Zip (`7z` on PATH, or the default install location)

## Build and run

```bat
cargo build --release
cargo run -- path\to\file.md
node pack.js
```

Output: `target/release/rustmarkdown.exe`.

Always use `cargo build --release` (not debug). On Windows, build / `cargo run` kills a running `rustmarkdown.exe` before linking so the exe is not locked.

`node pack.js`: kill running `rustmarkdown.exe` → `cargo build --release` → `release/rustmarkdown_x.x.x.7z` (version from `Cargo.toml`; archive contains the exe, `pdfium.dll`, README.md, README.zh.md, CHANGELOG.md). `release/` is gitignored.

`--selftest`: parser / table-width checks (exit 0 = pass). The exe is a GUI subsystem binary; use `cargo test` to see output:

```bat
cargo test
cargo run -- --selftest
```

## Shortcuts

| Key | Action |
|-----|--------|
| Ctrl+N | New |
| Ctrl+O | Open file (Markdown / Word / PDF / image / `.lnk`) |
| Ctrl+Shift+O | Open folder (sidebar workspace) |
| Ctrl+F | Find |
| F3 / Shift+F3 | Next / previous find hit |
| Ctrl+S / Ctrl+Shift+S | Save / Save as |
| Ctrl+W | Close tab |
| Ctrl+Shift+T | Reopen last closed tab |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+1 / Ctrl+2 / Ctrl+3 | Source / side-by-side / preview |
| Ctrl+E | Preview ↔ last edit mode |
| Ctrl+, | Settings |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+C | Copy (selected PDF text, or the whole image on an image tab) |
| Ctrl+wheel, Ctrl++ / Ctrl+-, Ctrl+0 | PDF / Word / image zoom / reset 100% |
| Wheel (image tab) | Zoom (no Ctrl; cursor-centered) |
| [ / ] | Image rotate 90° CCW / CW |
| PgUp / PgDn | Preview page (PDF / Word by page; Markdown by screen) |
| ↑ ↓ ← → | Preview scroll (editor still moves the caret) |
| F4 | Sidebar on/off |
| Alt+← / Alt+→ | Back / forward |

## Layout

```text
src/
  main.rs      entry and CLI
  app.rs       window state, menus, toolbar, shortcuts, drop, jump history
  i18n.rs      Chinese / English UI strings
  nav.rs       back / forward stack
  tabs.rs      tab bar (follow-drag reorder / tear-off / merge)
  workspace.rs folder workspace tree
  doc.rs       document session / tab / mode (Markdown / Word / PDF / image)
  parser/      Markdown parse, table widths, heading numbers
  view/        editor, preview, PDF pages (text select), image preview, find bar, outline, MD source highlight, fence highlight, toolbar icons, fonts
  io/          files and encoding, file watch, Word→MD, PDF/pdfium raster and text, image cache, Mermaid, .lnk, settings
assets/        app icons icon.png / icon.ico
native/pdfium  pdfium.dll (copied next to the exe at build; not committed)
pack.js        Release build and pack to release/rustmarkdown_x.x.x.7z
```
