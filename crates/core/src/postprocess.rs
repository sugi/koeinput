use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static ARTEFACT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:language\s+[A-Za-z]+\s*)?</?asr_text>|<\|[^|>]*\|>").unwrap());

const FILLERS: [&str; 8] = [
    "えーっと",
    "えーと",
    "えっと",
    "あのー",
    "そのー",
    "うーん",
    "えー",
    "んー",
];

pub struct PostProcessor {
    dict: Vec<(String, String)>,
}

impl PostProcessor {
    pub fn new(mut dict: Vec<(String, String)>) -> Self {
        dict.retain(|(k, _)| !k.is_empty());
        dict.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
        Self { dict }
    }

    pub fn empty() -> Self {
        Self { dict: Vec::new() }
    }

    pub fn apply(&self, text: &str) -> String {
        let t = ARTEFACT.replace_all(text, "");
        let t = self.replace_dictionary(&t);
        let t = remove_fillers(&t);
        remove_japanese_spaces(&t)
    }

    fn replace_dictionary(&self, text: &str) -> String {
        if self.dict.is_empty() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        'outer: while !rest.is_empty() {
            for (k, v) in &self.dict {
                if rest.starts_with(k.as_str()) {
                    out.push_str(v);
                    rest = &rest[k.len()..];
                    continue 'outer;
                }
            }
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
        out
    }
}

fn remove_fillers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'outer: while !rest.is_empty() {
        for f in FILLERS {
            if let Some(after) = rest.strip_prefix(f) {
                rest = after
                    .strip_prefix('、')
                    .or_else(|| after.strip_prefix(','))
                    .or_else(|| after.strip_prefix(' '))
                    .unwrap_or(after);
                continue 'outer;
            }
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn is_japanese(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F | 0x3040..=0x309F | 0x30A0..=0x30FF | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

fn is_space(c: char) -> bool {
    c == ' ' || c == '\u{3000}'
}

// A space is dropped only when the nearest non-space characters on both sides are Japanese,
// so "git worktree を" keeps both of its spaces.
fn remove_japanese_spaces(text: &str) -> String {
    let chars: Vec<char> = text.trim().chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if is_space(c) {
            let prev = chars[..i].iter().rev().find(|c| !is_space(**c));
            let next = chars[i + 1..].iter().find(|c| !is_space(**c));
            if let (Some(p), Some(n)) = (prev, next)
                && is_japanese(*p)
                && is_japanese(*n)
            {
                continue;
            }
        }
        out.push(c);
    }
    out
}

pub fn join_segments(parts: &[String]) -> String {
    let mut out = String::new();
    for p in parts.iter().filter(|p| !p.is_empty()) {
        if let (Some(l), Some(r)) = (out.chars().last(), p.chars().next())
            && l.is_ascii_alphanumeric()
            && r.is_ascii_alphanumeric()
        {
            out.push(' ');
        }
        out.push_str(p);
    }
    out
}

pub struct DictionaryLoad {
    pub pairs: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

pub fn load_dictionary(path: &Path) -> std::io::Result<DictionaryLoad> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DictionaryLoad {
                pairs: Vec::new(),
                warnings: Vec::new(),
            });
        }
        Err(e) => return Err(e),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut pairs = Vec::new();
    let mut warnings = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        match line.split_once('\t') {
            Some((k, v)) if !k.is_empty() => pairs.push((k.to_string(), v.to_string())),
            _ => warnings.push(format!(
                "{}: line {}: expected \"wrong<TAB>right\"",
                path.display(),
                i + 1
            )),
        }
    }
    Ok(DictionaryLoad { pairs, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pp(pairs: &[(&str, &str)]) -> PostProcessor {
        PostProcessor::new(
            pairs
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        )
    }

    #[test]
    fn strips_qwen_and_sensevoice_artefacts() {
        let p = PostProcessor::empty();
        assert_eq!(
            p.apply("language Japanese<asr_text>こんにちは"),
            "こんにちは"
        );
        assert_eq!(
            p.apply("前の文。language Japanese<asr_text>次の文"),
            "前の文。次の文"
        );
        assert_eq!(p.apply("<|ja|><|NEUTRAL|>テスト"), "テスト");
    }

    #[test]
    fn dictionary_longest_match_single_pass() {
        let p = pp(&[
            ("ウィスパー", "Whisper"),
            ("ウィスパーCPP", "whisper.cpp"),
            ("Whisper", "X"),
        ]);
        assert_eq!(p.apply("ウィスパーCPPとウィスパー"), "whisper.cppとWhisper");
    }

    #[test]
    fn removes_unambiguous_fillers_only() {
        let p = PostProcessor::empty();
        assert_eq!(p.apply("えーと、明日はえー晴れ"), "明日は晴れ");
        assert_eq!(p.apply("うーん、そうですね"), "そうですね");
        assert_eq!(p.apply("あの人はまあまあです"), "あの人はまあまあです");
    }

    #[test]
    fn removes_spaces_between_japanese_only() {
        let p = PostProcessor::empty();
        assert_eq!(
            p.apply(" 測る の は 次 の 項目 です。 "),
            "測るのは次の項目です。"
        );
        assert_eq!(p.apply("git worktree を 使う"), "git worktree を使う");
        assert_eq!(p.apply("これは　全角"), "これは全角");
    }

    #[test]
    fn order_dictionary_before_fillers() {
        let p = pp(&[("えーと", "ETO")]);
        assert_eq!(p.apply("えーと"), "ETO");
    }

    #[test]
    fn join_inserts_space_only_between_latin() {
        let parts = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_segments(&parts(&["Rust", "cargo"])), "Rust cargo");
        assert_eq!(join_segments(&parts(&["日本語", "テスト"])), "日本語テスト");
        assert_eq!(join_segments(&parts(&["Rust", "で書く"])), "Rustで書く");
        assert_eq!(join_segments(&parts(&["", "あ", ""])), "あ");
    }

    #[test]
    fn dictionary_tolerates_bom_crlf_comments_and_bad_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("d.tsv");
        std::fs::write(
            &p,
            "\u{feff}# comment\r\nラスト\tRust\r\n\r\nno tab here\r\n\tempty key\r\nギット\tgit\n",
        )
        .unwrap();
        let d = load_dictionary(&p).unwrap();
        assert_eq!(
            d.pairs,
            vec![
                ("ラスト".into(), "Rust".into()),
                ("ギット".into(), "git".into())
            ]
        );
        assert_eq!(d.warnings.len(), 2);
        assert!(d.warnings[0].contains("line 4"), "{:?}", d.warnings);
        assert!(
            load_dictionary(&dir.path().join("missing.tsv"))
                .unwrap()
                .pairs
                .is_empty()
        );
    }
}
