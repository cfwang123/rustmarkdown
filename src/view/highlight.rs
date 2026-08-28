use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use egui::{Align, Color32, FontId, Stroke, TextFormat};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

const MAX_CHARS: usize = 250_000;
const MAX_LINES: usize = 12_000;
const CACHE_CAP: usize = 64;
const CODE_FG: Color32 = Color32::from_rgb(0x1F, 0x29, 0x37);

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    ThemeSet::load_defaults()
        .themes
        .get("InspiredGitHub")
        .cloned()
        .unwrap_or_else(|| ThemeSet::load_defaults().themes["base16-ocean.light"].clone())
});

struct JobCache {
    map: HashMap<(u64, String), egui::text::LayoutJob>,
    lru: VecDeque<(u64, String)>,
}

static JOB_CACHE: LazyLock<Mutex<JobCache>> = LazyLock::new(|| {
    Mutex::new(JobCache {
        map: HashMap::new(),
        lru: VecDeque::new(),
    })
});

/// 预加载语法集，避免第一块代码着色时卡一下。
pub fn warmup() {
    let _ = &*SYNTAXES;
    let _ = &*THEME;
}

/// 围栏代码 syntect 着色（含常见语言别名；two-face 扩展语法）。
pub fn code_job(code: &str, lang: &str) -> egui::text::LayoutJob {
    let key_lang = normalize_lang(lang);
    let h = hash_code(code);
    {
        let mut cache = JOB_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(job) = cache.map.get(&(h, key_lang.clone())).cloned() {
            touch_lru(&mut cache, &(h, key_lang.clone()));
            return job;
        }
    }
    let job = build_job(code, &key_lang);
    {
        let mut cache = JOB_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let k = (h, key_lang);
        if !cache.map.contains_key(&k) {
            if cache.map.len() >= CACHE_CAP {
                if let Some(old) = cache.lru.pop_front() {
                    cache.map.remove(&old);
                }
            }
            cache.lru.push_back(k.clone());
            cache.map.insert(k, job.clone());
        }
    }
    job
}

fn touch_lru(cache: &mut JobCache, key: &(u64, String)) {
    if let Some(i) = cache.lru.iter().position(|k| k == key) {
        if let Some(k) = cache.lru.remove(i) {
            cache.lru.push_back(k);
        }
    }
}

fn hash_code(code: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}

fn build_job(code: &str, lang_key: &str) -> egui::text::LayoutJob {
    let font = FontId::monospace(12.5);
    let line_n = code.bytes().filter(|&b| b == b'\n').count() + 1;
    if code.len() > MAX_CHARS || line_n > MAX_LINES || is_plain(lang_key) {
        return plain_job(code, font);
    }
    let ps = &*SYNTAXES;
    let syntax = resolve_syntax(ps, lang_key, code);
    if syntax.name.eq_ignore_ascii_case("Plain Text") {
        return plain_job(code, font);
    }
    highlight_job(code, ps, syntax, font)
}

fn highlight_job(
    code: &str,
    ps: &SyntaxSet,
    syntax: &SyntaxReference,
    font: FontId,
) -> egui::text::LayoutJob {
    let mut h = HighlightLines::new(syntax, &THEME);
    let mut job = egui::text::LayoutJob::default();
    for line in LinesWithEndings::from(code) {
        match h.highlight_line(line, ps) {
            Ok(ranges) => {
                for (style, text) in ranges {
                    let fg = style.foreground;
                    let color = Color32::from_rgb(fg.r, fg.g, fg.b);
                    job.append(
                        text,
                        0.0,
                        TextFormat {
                            font_id: font.clone(),
                            color,
                            italics: style.font_style.contains(FontStyle::ITALIC),
                            underline: if style.font_style.contains(FontStyle::UNDERLINE) {
                                Stroke::new(1.0_f32, color)
                            } else {
                                Stroke::NONE
                            },
                            ..Default::default()
                        },
                    );
                }
            }
            Err(_) => {
                job.append(
                    line,
                    0.0,
                    TextFormat {
                        font_id: font.clone(),
                        color: CODE_FG,
                        ..Default::default()
                    },
                );
            }
        }
    }
    job.wrap.max_width = f32::INFINITY;
    job
}

/// 围栏内逐行 syntect（跨行保持 HighlightLines 状态）。
pub struct LineHl {
    h: HighlightLines<'static>,
}

impl LineHl {
    pub fn try_new(lang: &str) -> Option<Self> {
        let key = normalize_lang(lang);
        if is_plain(&key) {
            return None;
        }
        let ps: &'static SyntaxSet = &SYNTAXES;
        let theme: &'static Theme = &THEME;
        let syntax = resolve_syntax(ps, &key, "");
        if syntax.name.eq_ignore_ascii_case("Plain Text") {
            return None;
        }
        Some(Self {
            h: HighlightLines::new(syntax, theme),
        })
    }

    pub fn append_line(
        &mut self,
        job: &mut egui::text::LayoutJob,
        line: &str,
        font: &FontId,
        bg: Color32,
    ) {
        if line.is_empty() {
            return;
        }
        let ps = &*SYNTAXES;
        match self.h.highlight_line(line, ps) {
            Ok(ranges) => {
                for (style, text) in ranges {
                    let fg = style.foreground;
                    let color = Color32::from_rgb(fg.r, fg.g, fg.b);
                    job.append(
                        text,
                        0.0,
                        TextFormat {
                            font_id: font.clone(),
                            color,
                            background: bg,
                            italics: style.font_style.contains(FontStyle::ITALIC),
                            valign: Align::Center,
                            underline: if style.font_style.contains(FontStyle::UNDERLINE) {
                                Stroke::new(1.0_f32, color)
                            } else {
                                Stroke::NONE
                            },
                            ..Default::default()
                        },
                    );
                }
            }
            Err(_) => {
                job.append(
                    line,
                    0.0,
                    TextFormat {
                        font_id: font.clone(),
                        color: CODE_FG,
                        background: bg,
                        valign: Align::Center,
                        ..Default::default()
                    },
                );
            }
        }
    }
}

fn plain_job(code: &str, font: FontId) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        code,
        0.0,
        TextFormat {
            font_id: font,
            color: CODE_FG,
            ..Default::default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    job
}

fn resolve_syntax<'a>(ps: &'a SyntaxSet, lang_key: &str, code: &str) -> &'a SyntaxReference {
    if lang_key.is_empty() {
        return first_line_syntax(ps, code).unwrap_or_else(|| ps.find_syntax_plain_text());
    }
    let mapped = alias_token(lang_key);
    try_syntax(ps, mapped)
        .or_else(|| {
            if mapped != lang_key {
                try_syntax(ps, lang_key)
            } else {
                None
            }
        })
        .unwrap_or_else(|| ps.find_syntax_plain_text())
}

fn first_line_syntax<'a>(ps: &'a SyntaxSet, code: &str) -> Option<&'a SyntaxReference> {
    let line = code.lines().next().unwrap_or("");
    if line.is_empty() {
        return None;
    }
    ps.find_syntax_by_first_line(line)
}

fn try_syntax<'a>(ps: &'a SyntaxSet, tok: &str) -> Option<&'a SyntaxReference> {
    if tok.is_empty() {
        return None;
    }
    ps.find_syntax_by_token(tok)
        .or_else(|| ps.find_syntax_by_extension(tok))
        .or_else(|| ps.find_syntax_by_name(tok))
}

fn is_plain(lang: &str) -> bool {
    matches!(
        lang,
        "text" | "txt" | "log" | "plaintext" | "plain" | "none" | "output"
    )
}

fn normalize_lang(raw: &str) -> String {
    let t = raw.trim().trim_matches('`');
    let t = t
        .strip_prefix("language-")
        .or_else(|| t.strip_prefix("lang-"))
        .unwrap_or(t);
    let t = t
        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .next()
        .unwrap_or("");
    t.trim_start_matches('.').to_ascii_lowercase()
}

fn alias_token(lang: &str) -> &str {
    match lang {
        "c#" | "csharp" | "cs" => "cs",
        "js" | "javascript" | "node" | "mjs" | "cjs" => "js",
        "ts" | "typescript" => "ts",
        "tsx" | "typescriptreact" => "tsx",
        "jsx" => "jsx",
        "py" | "python" | "pyw" => "py",
        "sh" | "bash" | "shell" | "zsh" | "console" => "sh",
        "ps1" | "powershell" | "pwsh" => "ps1",
        "bat" | "cmd" | "batch" => "bat",
        "yml" | "yaml" => "yaml",
        "rs" | "rust" => "rs",
        "kt" | "kotlin" => "kt",
        "go" | "golang" => "go",
        "c++" | "cpp" | "cxx" | "cc" | "hpp" | "hxx" => "cpp",
        "htm" | "html" => "html",
        "scss" | "sass" => "scss",
        "less" => "less",
        "md" | "markdown" => "md",
        "dockerfile" | "docker" => "Dockerfile",
        "ini" | "cfg" | "conf" => "ini",
        "proto" | "protobuf" => "proto",
        "rb" | "ruby" => "rb",
        "pl" | "perl" => "pl",
        "objc" | "obj-c" | "objective-c" | "objectivec" => "m",
        "fs" | "fsharp" | "f#" => "fs",
        "make" | "makefile" => "Makefile",
        "diff" | "patch" => "diff",
        "graphql" | "gql" => "graphql",
        "tf" | "terraform" | "hcl" => "tf",
        "jsonc" => "json",
        _ => lang,
    }
}

#[cfg(test)]
pub(crate) fn syntax_name_for(lang: &str) -> String {
    let key = normalize_lang(lang);
    if is_plain(&key) {
        return "Plain Text".to_string();
    }
    resolve_syntax(&SYNTAXES, &key, "").name.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csharp_not_css() {
        let name = syntax_name_for("cs");
        assert_eq!(name, "C#", "cs should be C#, got {name}");
        assert_eq!(syntax_name_for("csharp"), "C#");
        assert_eq!(syntax_name_for("c#"), "C#");
        assert_eq!(syntax_name_for("css"), "CSS");
    }

    #[test]
    fn common_langs_not_plain() {
        let langs = [
            "js",
            "javascript",
            "ts",
            "py",
            "python",
            "rs",
            "go",
            "java",
            "c",
            "cpp",
            "sql",
            "json",
            "html",
            "css",
            "yaml",
            "yml",
            "sh",
            "bash",
            "toml",
            "kt",
            "ps1",
            "dockerfile",
            "xml",
            "lua",
            "php",
            "rb",
            "swift",
            "dart",
            "vue",
        ];
        for lang in langs {
            let name = syntax_name_for(lang);
            assert_ne!(name, "Plain Text", "lang={lang} unresolved");
        }
    }

    #[test]
    fn plaintext_stays_plain() {
        assert_eq!(syntax_name_for("text"), "Plain Text");
        assert_eq!(syntax_name_for("txt"), "Plain Text");
        assert_eq!(syntax_name_for("log"), "Plain Text");
    }

    #[test]
    fn language_prefix_stripped() {
        assert_eq!(syntax_name_for("language-python"), syntax_name_for("py"));
    }

    #[test]
    fn rust_uses_multiple_colors() {
        let job = code_job("fn main() {\n    let x = 1;\n}\n", "rust");
        let colors: std::collections::HashSet<_> =
            job.sections.iter().map(|s| s.format.color).collect();
        assert!(
            colors.len() >= 2,
            "expected keyword coloring, got {} colors",
            colors.len()
        );
    }
}
