use crate::parser::{MdSpan, MdSpanKind};

/// 行内解析（移植 MdParser.ParseInlines）。
pub fn parse_inlines(text: &str) -> Vec<MdSpan> {
    let mut spans = Vec::new();
    if text.is_empty() {
        return spans;
    }
    let n = text.len();
    let mut i = 0;
    let mut buf = String::new();

    let flush = |buf: &mut String, spans: &mut Vec<MdSpan>| {
        if !buf.is_empty() {
            spans.push(MdSpan {
                kind: MdSpanKind::Text,
                text: std::mem::take(buf),
                href: String::new(),
            });
        }
    };

    while i < n {
        let c = text.as_bytes()[i];
        if c == b'\n' {
            flush(&mut buf, &mut spans);
            spans.push(MdSpan {
                kind: MdSpanKind::SoftBr,
                text: "\n".into(),
                href: String::new(),
            });
            i += 1;
            continue;
        }
        if c == b'\\' && i + 1 < n && text.as_bytes()[i + 1] == b'\n' {
            flush(&mut buf, &mut spans);
            spans.push(MdSpan {
                kind: MdSpanKind::SoftBr,
                text: "\n".into(),
                href: String::new(),
            });
            i += 2;
            continue;
        }
        if c == b'`' {
            if let Some(j) = text[i + 1..].find('`') {
                let j = i + 1 + j;
                if j > i {
                    flush(&mut buf, &mut spans);
                    spans.push(MdSpan {
                        kind: MdSpanKind::Code,
                        text: text[i + 1..j].to_string(),
                        href: String::new(),
                    });
                    i = j + 1;
                    continue;
                }
            }
        }
        if c == b'!' && i + 1 < n && text.as_bytes()[i + 1] == b'[' {
            if let Some((end, label, href)) = try_link(text, i + 1) {
                flush(&mut buf, &mut spans);
                spans.push(MdSpan {
                    kind: MdSpanKind::Image,
                    text: label,
                    href,
                });
                i = end;
                continue;
            }
        }
        if c == b'[' {
            if let Some((end, label, href)) = try_link(text, i) {
                flush(&mut buf, &mut spans);
                spans.push(MdSpan {
                    kind: MdSpanKind::Link,
                    text: link_label_text(&label),
                    href,
                });
                i = end;
                continue;
            }
        }
        if c == b'=' && i + 1 < n && text.as_bytes()[i + 1] == b'=' {
            if let Some(rel) = text[i + 2..].find("==") {
                let j = i + 2 + rel;
                if j > i + 2 {
                    flush(&mut buf, &mut spans);
                    spans.extend(wrap_style(MdSpanKind::Mark, &text[i + 2..j]));
                    i = j + 2;
                    continue;
                }
            }
        }
        if c == b'~' && i + 1 < n && text.as_bytes()[i + 1] == b'~' {
            if let Some(rel) = text[i + 2..].find("~~") {
                let j = i + 2 + rel;
                if j > i + 2 {
                    flush(&mut buf, &mut spans);
                    spans.extend(wrap_style(MdSpanKind::Strike, &text[i + 2..j]));
                    i = j + 2;
                    continue;
                }
            }
        }
        if c == b'<' && text[i..].len() >= 5 && text[i..].starts_with("<font") {
            if let Some((end, inner, kind, href)) = try_font_tag(text, i) {
                flush(&mut buf, &mut spans);
                spans.push(MdSpan {
                    kind,
                    text: inner,
                    href,
                });
                i = end;
                continue;
            }
        }
        if (c == b'*' || c == b'_')
            && i + 2 < n
            && text.as_bytes()[i + 1] == c
            && text.as_bytes()[i + 2] == c
        {
            let mark = if c == b'*' { "***" } else { "___" };
            if let Some(rel) = text[i + 3..].find(mark) {
                let j = i + 3 + rel;
                if j > i + 3 {
                    flush(&mut buf, &mut spans);
                    spans.extend(wrap_style(MdSpanKind::BoldItalic, &text[i + 3..j]));
                    i = j + 3;
                    continue;
                }
            }
        }
        if (c == b'*' || c == b'_') && i + 1 < n && text.as_bytes()[i + 1] == c {
            let mark = if c == b'*' { "**" } else { "__" };
            if let Some(rel) = text[i + 2..].find(mark) {
                let j = i + 2 + rel;
                if j > i + 2 {
                    flush(&mut buf, &mut spans);
                    spans.extend(wrap_style(MdSpanKind::Bold, &text[i + 2..j]));
                    i = j + 2;
                    continue;
                }
            }
        }
        if c == b'*' || c == b'_' {
            let ch = c as char;
            if let Some(rel) = text[i + 1..].find(ch) {
                let j = i + 1 + rel;
                if j > i + 1 && (j + 1 >= n || text.as_bytes()[j + 1] != c) {
                    flush(&mut buf, &mut spans);
                    spans.extend(wrap_style(MdSpanKind::Italic, &text[i + 1..j]));
                    i = j + 1;
                    continue;
                }
            }
        }
        if c == b'h' && (text[i..].starts_with("http://") || text[i..].starts_with("https://")) {
            let mut j = i;
            while j < n {
                let b = text.as_bytes()[j];
                if b.is_ascii_whitespace() || b == b')' {
                    break;
                }
                j += 1;
            }
            while j > i {
                let prev = text.as_bytes()[j - 1];
                if matches!(prev, b'.' | b',' | b';' | b':') {
                    j -= 1;
                } else {
                    break;
                }
            }
            let url = text[i..j].to_string();
            flush(&mut buf, &mut spans);
            spans.push(MdSpan {
                kind: MdSpanKind::Link,
                text: url.clone(),
                href: url,
            });
            i = j;
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }
    flush(&mut buf, &mut spans);
    spans
}

/// 粗体/斜体等包裹内再解析：`**[mdview](mdview/)**` 展开成链接，而不是把 Markdown 当正文。
fn wrap_style(kind: MdSpanKind, inner: &str) -> Vec<MdSpan> {
    let kids = parse_inlines(inner);
    if kids.is_empty() {
        return Vec::new();
    }
    if kids
        .iter()
        .all(|s| matches!(s.kind, MdSpanKind::Text | MdSpanKind::SoftBr))
    {
        return vec![MdSpan {
            kind,
            text: inner.to_string(),
            href: String::new(),
        }];
    }
    kids.into_iter().map(|s| restyle(kind, s)).collect()
}

fn restyle(wrap: MdSpanKind, s: MdSpan) -> MdSpan {
    match (wrap, s.kind) {
        (_, MdSpanKind::SoftBr | MdSpanKind::Image | MdSpanKind::Code) => s,
        (MdSpanKind::Bold, MdSpanKind::Text | MdSpanKind::Bold) => MdSpan {
            kind: MdSpanKind::Bold,
            ..s
        },
        (MdSpanKind::Bold, MdSpanKind::Italic | MdSpanKind::BoldItalic) => MdSpan {
            kind: MdSpanKind::BoldItalic,
            ..s
        },
        (MdSpanKind::Italic, MdSpanKind::Text | MdSpanKind::Italic) => MdSpan {
            kind: MdSpanKind::Italic,
            ..s
        },
        (MdSpanKind::Italic, MdSpanKind::Bold | MdSpanKind::BoldItalic) => MdSpan {
            kind: MdSpanKind::BoldItalic,
            ..s
        },
        (
            MdSpanKind::BoldItalic,
            MdSpanKind::Text | MdSpanKind::Bold | MdSpanKind::Italic | MdSpanKind::BoldItalic,
        ) => MdSpan {
            kind: MdSpanKind::BoldItalic,
            ..s
        },
        (MdSpanKind::Mark, MdSpanKind::Text) => MdSpan {
            kind: MdSpanKind::Mark,
            ..s
        },
        (MdSpanKind::Strike, MdSpanKind::Text) => MdSpan {
            kind: MdSpanKind::Strike,
            ..s
        },
        _ => s,
    }
}

fn link_label_text(label: &str) -> String {
    let kids = parse_inlines(label);
    if kids.is_empty() {
        return label.to_string();
    }
    if kids.len() == 1 && kids[0].kind == MdSpanKind::Text {
        return kids[0].text.clone();
    }
    kids.into_iter().map(|s| s.text).collect()
}

fn try_link(text: &str, open_bracket: usize) -> Option<(usize, String, String)> {
    if open_bracket >= text.len() || text.as_bytes()[open_bracket] != b'[' {
        return None;
    }
    let close = text[open_bracket + 1..].find(']')?;
    let close = open_bracket + 1 + close;
    if close + 1 >= text.len() || text.as_bytes()[close + 1] != b'(' {
        return None;
    }
    let endp = text[close + 2..].find(')')?;
    let endp = close + 2 + endp;
    let label = text[open_bracket + 1..close].to_string();
    let href = text[close + 2..endp].trim().to_string();
    Some((endp + 1, label, href))
}

fn try_font_tag(text: &str, start: usize) -> Option<(usize, String, MdSpanKind, String)> {
    let rest = &text[start..];
    if !rest.starts_with("<font") {
        return None;
    }
    let gt = rest.find('>')?;
    if gt < 5 {
        return None;
    }
    let attrs = &rest[5..gt];
    let after = &rest[gt + 1..];
    let close = after
        .to_ascii_lowercase()
        .find("</font>")
        .or_else(|| after.to_ascii_lowercase().find("</font >"))?;
    let inner = after[..close].to_string();
    let end = start + gt + 1 + close + "</font>".len();
    let mut bold = false;
    let mut italic = false;
    let mut fg: Option<String> = None;
    let mut bg: Option<String> = None;
    if let Some(c) = attr_value(attrs, "color") {
        fg = normalize_color(&c);
    }
    if let Some(style) = attr_value(attrs, "style") {
        for part in style.split(';') {
            let part = part.trim();
            if let Some((k, v)) = part.split_once(':') {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim();
                match k.as_str() {
                    "font-weight" if v.eq_ignore_ascii_case("bold") || v == "600" || v == "700" => {
                        bold = true;
                    }
                    "font-style" if v.eq_ignore_ascii_case("italic") => italic = true,
                    "color" => fg = normalize_color(v).or(fg),
                    "background" | "background-color" => bg = normalize_color(v),
                    _ => {}
                }
            }
        }
    }
    let kind = match (bold, italic) {
        (true, true) => MdSpanKind::BoldItalic,
        (true, false) => MdSpanKind::Bold,
        (false, true) => MdSpanKind::Italic,
        (false, false) => MdSpanKind::Text,
    };
    let href = format!("{};{}", fg.unwrap_or_default(), bg.unwrap_or_default());
    Some((end, inner, kind, href))
}

fn attr_value(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let key = format!("{name}=");
    let i = lower.find(&key)?;
    let after = attrs[i + key.len()..].trim_start();
    let bytes = after.as_bytes();
    if bytes.first().copied() == Some(b'\'') || bytes.first().copied() == Some(b'"') {
        let q = bytes[0] as char;
        let end = after[1..].find(q)?;
        Some(after[1..1 + end].to_string())
    } else {
        let end = after
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(after.len());
        Some(after[..end].to_string())
    }
}

fn normalize_color(raw: &str) -> Option<String> {
    let s = raw.trim().trim_matches(|c| c == '"' || c == '\'');
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() == 3 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        let b = s.as_bytes();
        Some(format!(
            "#{}{}{}{}{}{}",
            b[0] as char, b[0] as char, b[1] as char, b[1] as char, b[2] as char, b[2] as char
        ))
    } else if s.len() == 6 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{s}"))
    } else {
        None
    }
}
