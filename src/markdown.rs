//! Upstream's deliberately small Markdown dialect, expressed as styled spans.
//! AppKit only handles the final attributed-string/RTF serialization.
use anyhow::Result;
use fancy_regex::Regex;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};
use unicode_segmentation::UnicodeSegmentation;
const ESCAPABLE: &str = "\\`*_{}[]()#+-.!>~|";
const PATTERNS: &[(&str, &str)] = &[
    ("heading", r"^[ \t]{0,3}(#{1,6})[ \t]+(.*)$"),
    ("rule", r"^[ \t]*(?:-{3,}|\*{3,}|_{3,})[ \t]*$"),
    ("fence", r"^[ \t]*```"),
    ("bullet", r"^[ \t]{0,3}[-*+][ \t]+(.*)$"),
    ("ordered", r"^[ \t]{0,3}[0-9]{1,9}[.)][ \t]+(.*)$"),
    ("quote", r"^[ \t]{0,3}>[ \t]?(.*)$"),
    ("link", r#"(?<!!)\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#),
    ("triple-star", r"\*\*\*(?=\S)(.+?)(?<=\S)\*\*\*"),
    (
        "triple-under",
        r"(?<![\w_])___(?=\S)(.+?)(?<=\S)___(?![\w_])",
    ),
    ("strong-star", r"\*\*(?=\S)(.+?)(?<=\S)\*\*"),
    ("strong-under", r"(?<![\w_])__(?=\S)(.+?)(?<=\S)__(?![\w_])"),
    ("strike", r"(~~)(?=\S)(.+?)(?<=\S)\1"),
    (
        "emph",
        r"(?<![\w*_])([*_])(?=\S)([^*_]+?)(?<=\S)\1(?![\w*_])",
    ),
];
fn rx(name: &str) -> &'static Regex {
    static CACHE: OnceLock<HashMap<&'static str, Regex>> = OnceLock::new();
    &CACHE.get_or_init(|| {
        PATTERNS
            .iter()
            .map(|(name, p)| (*name, Regex::new(p).expect("constant Markdown regex")))
            .collect()
    })[name]
}
#[derive(Serialize, Default)]
pub struct Span {
    text: String,
    bold: bool,
    italic: bool,
    strike: bool,
}
#[derive(Serialize)]
pub struct Line {
    kind: &'static str,
    spans: Vec<Span>,
}
fn escape_base(s: &str) -> u32 {
    let used = s
        .chars()
        .filter(|c| ('\u{e000}'..='\u{f8ff}').contains(c))
        .map(|c| c as u32)
        .collect::<HashSet<_>>();
    let width = ESCAPABLE.len() as u32;
    (0xe000..=0xf8ff - width)
        .step_by(width as usize)
        .find(|base| !(*base..*base + width).any(|n| used.contains(&n)))
        .unwrap_or(0xe000)
}
fn hidden(s: &str, base: u32) -> Option<char> {
    if s.len() == 1 {
        ESCAPABLE
            .find(s)
            .and_then(|i| char::from_u32(base + i as u32))
    } else {
        None
    }
}
fn hide_all(s: &str, base: u32) -> String {
    s.graphemes(true)
        .map(|c| {
            hidden(c, base)
                .map(|c| c.to_string())
                .unwrap_or_else(|| c.into())
        })
        .collect()
}
fn protect(s: &str, base: u32) -> String {
    let (mut out, mut code) = (String::new(), String::new());
    let (mut escaping, mut in_code) = (false, false);
    for ch in s.graphemes(true) {
        if in_code {
            if ch == "`" {
                out += &hide_all(&format!("`{code}`"), base);
                in_code = false;
                code.clear();
            } else {
                code += ch;
            }
            continue;
        }
        if escaping {
            if let Some(c) = hidden(ch, base) {
                out.push(c);
            } else {
                out.push('\\');
                out += ch;
            }
            escaping = false;
        } else if ch == "\\" {
            escaping = true;
        } else if ch == "`" {
            in_code = true;
            code.clear();
        } else {
            out += ch;
        }
    }
    if in_code {
        out += &hide_all(&format!("`{code}"), base);
    }
    if escaping {
        out.push('\\');
    }
    out
}
fn restore(s: &str, base: u32) -> String {
    s.graphemes(true)
        .map(|c| {
            let mut chars = c.chars();
            let v = chars.next().unwrap() as u32;
            if chars.next().is_none() && v >= base && v < base + ESCAPABLE.len() as u32 {
                (ESCAPABLE.as_bytes()[(v - base) as usize] as char).to_string()
            } else {
                c.into()
            }
        })
        .collect()
}
fn spans(line: &str, force_bold: bool) -> Result<Vec<Span>> {
    let base = escape_base(line);
    let mut s = protect(line, base);
    while let Some(c) = rx("link").captures(&s)? {
        let whole = c.get(0).unwrap();
        let label = c.get(1).unwrap().as_str().trim();
        let url = c.get(2).unwrap().as_str();
        let replacement = if label.is_empty() || label == url {
            url.into()
        } else {
            format!("{label} ({url})")
        };
        let range = whole.range();
        s.replace_range(range, &replacement);
    }
    for (name, template) in [
        ("triple-star", "\x01\x02$1\x02\x01"),
        ("triple-under", "\x01\x02$1\x02\x01"),
        ("strong-star", "\x01$1\x01"),
        ("strong-under", "\x01$1\x01"),
        ("strike", "\x03$2\x03"),
        ("emph", "\x02$2\x02"),
    ] {
        s = rx(name).try_replacen(&s, 0, template)?.into_owned();
    }
    let mut out = vec![];
    let mut current = String::new();
    let (mut bold, mut italic, mut strike) = (false, false, false);
    let flush = |out: &mut Vec<Span>, current: &mut String, bold, italic, strike| {
        if !current.is_empty() {
            out.push(Span {
                text: restore(current, base),
                bold: bold || force_bold,
                italic,
                strike,
            });
            current.clear();
        }
    };
    for ch in s.graphemes(true) {
        match ch {
            "\x01" => {
                flush(&mut out, &mut current, bold, italic, strike);
                bold = !bold;
            }
            "\x02" => {
                flush(&mut out, &mut current, bold, italic, strike);
                italic = !italic;
            }
            "\x03" => {
                flush(&mut out, &mut current, bold, italic, strike);
                strike = !strike;
            }
            _ => current += ch,
        }
    }
    flush(&mut out, &mut current, bold, italic, strike);
    Ok(out)
}
fn normalize(md: &str) -> String {
    md.replace("\r\n", "\n").replace('\r', "\n")
}
pub fn lines(md: &str) -> Result<Vec<Line>> {
    let mut result = vec![];
    let mut in_fence = false;
    for raw in normalize(md).split('\n') {
        if rx("fence").is_match(raw)? {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            result.push(Line {
                kind: "body",
                spans: vec![Span {
                    text: raw.into(),
                    ..Span::default()
                }],
            });
            continue;
        }
        if rx("rule").is_match(raw)? {
            continue;
        }
        let mut found = false;
        for (pattern, kind, group, bold) in [
            ("heading", "body", 2, true),
            ("bullet", "bullet", 1, false),
            ("ordered", "ordered", 1, false),
            ("quote", "quote", 1, false),
        ] {
            if let Some(c) = rx(pattern).captures(raw)? {
                result.push(Line {
                    kind,
                    spans: spans(c.get(group).unwrap().as_str(), bold)?,
                });
                found = true;
                break;
            }
        }
        if !found {
            result.push(Line {
                kind: "body",
                spans: spans(raw, false)?,
            });
        }
    }
    Ok(result)
}
pub fn inline(md: &str) -> Result<String> {
    let mut result = vec![];
    for line in normalize(md).split('\n') {
        result.push(
            spans(line, false)?
                .into_iter()
                .map(|s| s.text)
                .collect::<String>(),
        );
    }
    Ok(result.join(" ").trim().into())
}
