# rustmarkdown

Version **1.0.2**. [中文说明](README.zh.md) · [Changelog](CHANGELOG.md)

A Windows-first Markdown preview / editor rendered natively in Rust + egui — **no browser engine**. Three views: source, side-by-side, preview. Multi-tab, drag-and-drop of files / folders / `.lnk`, plus read-only previews for DOC / DOCX, XLS / XLSX, PDF, and images (aligned with docview).

## Editing and preview

- Three modes: source / side-by-side / preview (Ctrl+1/2/3, Ctrl+E toggle); switching keeps the top-of-screen line aligned.
- Markdown GFM subset with source line mapping; native preview with heading auto-numbering, fold by heading, task lists, continuous-grid tables, inline `<font>` styles; table widths aligned with mdview (short columns pinned).
- Fenced-code highlight (syntect + two-face, common languages and aliases); ` ```mermaid ` charts in pure Rust (no browser; falls back to source on error).
- Editor wraps by pane width, pure-white background; incremental layout (source by line fingerprint, preview by AST block) keeps long files smooth.
- Find (Ctrl+F, F3 / Shift+F3) with source / preview / Excel-cell highlights; encoding auto-detect (UTF-8 / GBK / UTF-16) and save-back; external-change watch with reload prompt.
- Vim-encrypted Markdown (zip / blowfish / blowfish2): password prompt on open, write-back with the original method, password in memory only.
- Ctrl+click links / anchors / local paths; bare `http(s)://` and file paths render as underlined links; double/triple click selects by punctuation, copy drops extra CJK-Latin spaces.
- Undo/redo keeps the caret at the change; undoing back to the saved text marks the tab clean; prompts and inputs close with Esc.

## Documents

- **DOC / DOCX** read-only: `office_oxide` layout modeled and paginated directly (no Markdown round-trip); zoom, PgUp/PgDn, outline jump, Save As exports `.md`.
- **PDF** read-only: pdfium continuous pages, visible-page-only rendering (1000-page scroll OK), Sumatra-style yellow text selection, right-click copy page image.
- **XLS / XLSX** read-only: calamine virtual grid, sheet tabs, frozen headers, drag-select copy as TSV, zoom, PgUp/PgDn switches sheets.
- **Images** read-only: png/jpg/gif/bmp/ico/tif/webp, fit / 100% and cursor-centered zoom, pan, `[`/`]` rotate, copy, Save As; double-click opens the overlay view.

## Tabs, windows and sidebar

- Multi-tab: drag reorder, middle-click close, right-click close others / all, reopen (Ctrl+Shift+T), recent files (last 20); tear-off into a new window and merge (docview-aligned).
- Session restore: files / modes / scroll / workspace root / window size and position (`session.json`); only the active tab reads disk at startup.
- Sidebar (F4): Explorer folder tree (lazy load, path bar, up/back/forward/refresh, set as workspace) + Outline (filter, scroll-sync highlight, jump).
- Back / forward (Alt+← / Alt+→) from outline clicks, anchors and in-doc links; status bar shows mode / lines / encoding / tab width.

## Settings and updates

- Settings dialog (Ctrl+,): UI language (中文 / English), tab width, heading auto-number, max image width, logs, **auto-update interval**.
- **Check for Updates** (Help menu): queries GitHub Releases, downloads the `.7z` with progress, then quits, replaces the install directory, and restarts (aligned with ScreenKit); startup auto-check on the interval (default 7 days, 0 = off); GitHub goes through the local proxy `127.0.0.1:7897` with a direct fallback.
- Window title shows the version; settings and recent files are stored in `%LocalAppData%\rustmarkdown\`.

## Command line

```text
rustmarkdown.exe [path ...] [options]
```

| Option | Meaning |
|--------|---------|
| `path` | Open a file / folder at startup |
| `--selftest` | Parser / table-width checks (exit 0 = pass) |
| `--update-check` | One-shot release check; result written to `tmp/update_apply.log` (debug) |
| `--apply-update <archive>` `--target <dir>` `[--wait-pid <pid>]` `[--restart]` | Updater CLI: wait for `--wait-pid` → extract `.7z` → overwrite `<dir>` → optional restart (used by Check for Updates) |

## Requirements

- Windows x64 (other platforms may build; not the primary test target)
- Rust 1.80+ (rustup stable recommended)
- Packaging and self-update: 7-Zip (`7z` on PATH or default install); `node pack.js` also needs Node.js

## Build and run

```bat
cargo build --release
cargo run -- path\to\file.md
node pack.js
```

Output: `target/release/rustmarkdown.exe`. `node pack.js` kills a running `rustmarkdown.exe`, builds Release, and writes `release/rustmarkdown_x.x.x.7z` (exe + `pdfium.dll` + README.md + README.zh.md + CHANGELOG.md; version from `Cargo.toml`). `release/` is gitignored. The exe is a GUI-subsystem binary; use `cargo test` / `--selftest` for check output.

## Shortcuts

| Key | Action |
|-----|--------|
| Ctrl+N | New |
| Ctrl+O | Open file (Markdown / Word / Excel / PDF / image / `.lnk`) |
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
| Ctrl+C | Copy (selected PDF text, selected spreadsheet cells, or the whole image on an image tab) |
| Ctrl+wheel, Ctrl++ / Ctrl+-, Ctrl+0 | PDF / Word / Excel / image zoom / reset 100% |
| Wheel (image tab) | Zoom (no Ctrl; cursor-centered) |
| [ / ] | Image rotate 90° CCW / CW |
| PgUp / PgDn | Preview page (PDF / Word by page; Excel by sheet; Markdown by screen) |
| ↑ ↓ ← → | Preview scroll (editor still moves the caret) |
| F4 | Sidebar on/off |
| Alt+← / Alt+→ | Back / forward |

## Layout

```text
src/
  main.rs      entry and CLI
  app.rs       window state, menus, toolbar, shortcuts, drop, jump history, update flow
  i18n.rs      Chinese / English UI strings
  nav.rs       back / forward stack
  tabs.rs      tab bar (follow-drag reorder / tear-off / merge)
  workspace.rs folder workspace tree
  doc.rs       document session / tab / mode (Markdown / Word / Excel / PDF / image)
  parser/      Markdown parse, table widths, heading numbers
  view/        editor, preview, Word pages, Excel grid, PDF pages (text select), image preview, find bar, outline, MD source highlight, fence highlight, toolbar icons, fonts
  io/          files and encoding, file watch, Word layout IR, Excel tables, PDF/pdfium raster and text, image cache, Mermaid, .lnk, settings, self-updater
assets/        app icons icon.png / icon.ico
native/pdfium  pdfium.dll (copied next to the exe at build; not committed)
pack.js        Release build and pack to release/rustmarkdown_x.x.x.7z
```