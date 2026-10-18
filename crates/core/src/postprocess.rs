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

#[derive(Debug, PartialEq, Eq)]
pub enum AddOutcome {
    Added,
    Replaced,
}

#[derive(Debug, thiserror::Error)]
pub enum DictionaryError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn check_form(label: &str, form: &str) -> Result<(), DictionaryError> {
    if form.is_empty() {
        return Err(DictionaryError::Invalid(format!("{label} is empty")));
    }
    if form.contains(['\t', '\n', '\r']) {
        return Err(DictionaryError::Invalid(format!(
            "{label} contains a tab or a line break"
        )));
    }
    Ok(())
}

fn read_or_empty(path: &Path) -> std::io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// The key of an entry line, `None` for comments, blank lines and malformed lines.
fn entry_key(line: &str, first: bool) -> Option<&str> {
    let line = if first {
        line.strip_prefix('\u{feff}').unwrap_or(line)
    } else {
        line
    };
    let line = line.trim_end_matches(['\r', '\n']);
    if line.trim().is_empty() || line.starts_with('#') {
        return None;
    }
    line.split_once('\t')
        .map(|(k, _)| k)
        .filter(|k| !k.is_empty())
}

fn line_ending(line: &str) -> &str {
    if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    }
}

/// Sets `wrong` to `right`, replacing the first entry for `wrong` in place and appending otherwise.
/// Comments, blank lines, order, the BOM and the line endings stay as they are.
pub fn add_entry(path: &Path, wrong: &str, right: &str) -> Result<AddOutcome, DictionaryError> {
    check_form("the wrong form", wrong)?;
    check_form("the right form", right)?;
    if wrong.starts_with('#') {
        return Err(DictionaryError::Invalid(
            "the wrong form starts with #, which marks a comment".into(),
        ));
    }
    let text = read_or_empty(path)?;
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = String::with_capacity(text.len() + wrong.len() + right.len() + 2);
    let mut replaced = false;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if entry_key(line, i == 0) == Some(wrong) {
            if replaced {
                continue;
            }
            replaced = true;
            if i == 0 && line.starts_with('\u{feff}') {
                out.push('\u{feff}');
            }
            out.push_str(&format!("{wrong}\t{right}"));
            out.push_str(if line_ending(line).is_empty() {
                eol
            } else {
                line_ending(line)
            });
        } else {
            out.push_str(line);
        }
    }
    if !replaced {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(eol);
        }
        out.push_str(&format!("{wrong}\t{right}{eol}"));
    }
    crate::config::write_atomic(path, &out)?;
    Ok(if replaced {
        AddOutcome::Replaced
    } else {
        AddOutcome::Added
    })
}

/// Removes every entry for `wrong`; returns whether there was one. The file is untouched on a miss.
pub fn remove_entry(path: &Path, wrong: &str) -> Result<bool, DictionaryError> {
    let text = read_or_empty(path)?;
    let mut removed = false;
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if entry_key(line, i == 0) == Some(wrong) {
            removed = true;
            if i == 0 && line.starts_with('\u{feff}') {
                out.push('\u{feff}');
            }
        } else {
            out.push_str(line);
        }
    }
    if removed {
        crate::config::write_atomic(path, &out)?;
    }
    Ok(removed)
}

type Stamp = Option<(std::time::SystemTime, u64)>;

fn stamp(path: &Path) -> Stamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// The reload runs under the daemon's lock at Start, so a file beyond this size is not read there.
const MAX_RELOAD_BYTES: u64 = 1 << 20;

/// Notices changes to the dictionary file by its modification time and length.
pub struct DictionaryWatch {
    path: std::path::PathBuf,
    seen: Stamp,
    /// The stamp of an oversized file already warned about.
    rejected: Stamp,
}

impl DictionaryWatch {
    /// Call before the first load, so a change during that load is not missed.
    pub fn new(path: std::path::PathBuf) -> Self {
        let seen = stamp(&path);
        Self {
            path,
            seen,
            rejected: None,
        }
    }

    /// Warns once per file stamp; returns whether it warned.
    fn warn_oversized(&mut self, now: Stamp) -> bool {
        if self.rejected == now {
            return false;
        }
        self.rejected = now;
        tracing::warn!(
            "{} is larger than 1 MiB; keeping the previous dictionary",
            self.path.display()
        );
        true
    }

    /// A new post-processor when the file differs from the last one seen; a missing file gives an empty one.
    /// A file that cannot be read keeps the old dictionary and is tried again next time.
    pub fn reload(&mut self) -> Option<PostProcessor> {
        let now = stamp(&self.path);
        if now == self.seen {
            return None;
        }
        if let Some((_, len)) = now
            && len > MAX_RELOAD_BYTES
        {
            self.warn_oversized(now);
            return None;
        }
        match load_dictionary(&self.path) {
            Ok(d) => {
                for w in &d.warnings {
                    tracing::warn!("{w}");
                }
                self.seen = now;
                tracing::info!(entries = d.pairs.len(), "dictionary reloaded");
                Some(PostProcessor::new(d.pairs))
            }
            Err(e) => {
                tracing::warn!(
                    "{}: {e}; keeping the previous dictionary",
                    self.path.display()
                );
                None
            }
        }
    }
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

    fn dict_file(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dictionary.tsv");
        std::fs::write(&p, content).unwrap();
        (dir, p)
    }

    #[test]
    fn add_entry_appends_and_replaces_in_place() {
        let (_d, p) = dict_file("# my words\nあ\tい\n\nう\tえ\n");
        assert_eq!(add_entry(&p, "お", "か").unwrap(), AddOutcome::Added);
        assert_eq!(add_entry(&p, "あ", "き").unwrap(), AddOutcome::Replaced);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "# my words\nあ\tき\n\nう\tえ\nお\tか\n"
        );
    }

    #[test]
    fn add_entry_creates_the_file_and_terminates_the_last_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/dictionary.tsv");
        add_entry(&p, "a", "b").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\tb\n");
        std::fs::write(&p, "a\tb").unwrap();
        add_entry(&p, "c", "d").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\tb\nc\td\n");
    }

    #[test]
    fn add_entry_keeps_bom_and_crlf() {
        let (_d, p) = dict_file("\u{feff}a\tb\r\n# c\r\n");
        add_entry(&p, "a", "x").unwrap();
        add_entry(&p, "n", "m").unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "\u{feff}a\tx\r\n# c\r\nn\tm\r\n"
        );
    }

    #[test]
    fn add_entry_refuses_forms_that_cannot_round_trip() {
        let (_d, p) = dict_file("");
        for (w, r) in [
            ("", "b"),
            ("a", ""),
            ("a\tb", "c"),
            ("a", "b\nc"),
            ("a\r", "b"),
            ("#a", "b"),
        ] {
            assert!(add_entry(&p, w, r).is_err(), "{w:?} {r:?}");
        }
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
    }

    #[test]
    fn remove_entry_drops_every_match_and_reports_a_miss() {
        let (_d, p) = dict_file("# c\na\tb\nx\ty\na\tz\n");
        assert!(remove_entry(&p, "a").unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "# c\nx\ty\n");
        assert!(!remove_entry(&p, "a").unwrap());
        let missing = p.with_file_name("none.tsv");
        assert!(!remove_entry(&missing, "a").unwrap());
        assert!(!missing.exists());
    }

    #[test]
    fn add_entry_drops_later_duplicates_of_the_same_wrong_form() {
        let (_d, p) = dict_file("a\tb\n# c\nx\ty\na\tz\na\tw\n");
        assert_eq!(add_entry(&p, "a", "q").unwrap(), AddOutcome::Replaced);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\tq\n# c\nx\ty\n");
    }

    #[test]
    fn add_entry_keeps_a_symlinked_dictionary() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.tsv");
        let link = dir.path().join("dictionary.tsv");
        std::fs::write(&real, "a\tb\n").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        add_entry(&link, "c", "d").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "a\tb\nc\td\n");
    }

    #[test]
    fn watch_reloads_only_when_the_file_changed() {
        let (_d, p) = dict_file("a\tb\n");
        let mut w = DictionaryWatch::new(p.clone());
        assert!(w.reload().is_none());
        std::fs::write(&p, "a\tbb\n").unwrap();
        let pp = w.reload().expect("changed length");
        assert_eq!(pp.apply("a"), "bb");
        assert!(w.reload().is_none());
        std::fs::remove_file(&p).unwrap();
        assert_eq!(w.reload().expect("removed").apply("a"), "a");
        assert!(w.reload().is_none());
        std::fs::write(&p, "a\tc\n").unwrap();
        assert_eq!(w.reload().expect("created").apply("a"), "c");
    }

    #[test]
    fn watch_skips_a_file_over_one_mib_and_checks_again_after_the_next_change() {
        let (_d, p) = dict_file("a\tb\n");
        let mut w = DictionaryWatch::new(p.clone());
        let mut big = String::from("a\tbig\n");
        big.push_str(&"#".repeat((MAX_RELOAD_BYTES as usize) + 1));
        std::fs::write(&p, &big).unwrap();
        assert!(w.reload().is_none());
        assert!(w.reload().is_none());
        std::fs::write(&p, "a\tsmall\n").unwrap();
        assert_eq!(w.reload().expect("shrunk").apply("a"), "small");
    }

    #[test]
    fn watch_warns_about_a_large_file_once_per_stamp() {
        let (_d, p) = dict_file("a\tb\n");
        let mut w = DictionaryWatch::new(p.clone());
        let mut big = String::from("a\tbig\n");
        big.push_str(&"#".repeat((MAX_RELOAD_BYTES as usize) + 1));
        std::fs::write(&p, &big).unwrap();
        let now = stamp(&p);
        assert!(w.warn_oversized(now));
        assert!(!w.warn_oversized(now));
        big.push('#');
        std::fs::write(&p, &big).unwrap();
        assert!(w.warn_oversized(stamp(&p)));
    }
}
