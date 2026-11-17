//! Small text helpers: front matter, page-to-text, text-to-HTML.
//!
//! The model never writes markup. Its plain text goes through [`to_html`], which escapes first,
//! then applies typography, so a model answer can never introduce a tag.

use crate::error::{Error, Result};
use crate::util::{const_regex, RE_FOOTNOTE};
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Split `---` front matter from the body. Text without front matter returns an empty first part.
///
/// ```
/// let (fm, body) = post_kit::text::split_front_matter("---\ntitle: \"A\"\n---\nBody");
/// assert_eq!(fm, "title: \"A\"");
/// assert_eq!(body, "Body");
/// ```
#[must_use]
pub fn split_front_matter(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return ("", text);
    };
    let Some(end) = rest.find("\n---") else {
        return ("", text);
    };
    let fm = &rest[..end];
    let after = &rest[end + 4..];
    (fm, after.strip_prefix('\n').unwrap_or(after))
}

/// The value of a simple `key: value` line in front matter, without surrounding quotes.
///
/// ```
/// assert_eq!(post_kit::text::fm_value("title: \"Alpha\"\nstub: true", "title").as_deref(), Some("Alpha"));
/// assert_eq!(post_kit::text::fm_value("stub: true", "title"), None);
/// ```
#[must_use]
pub fn fm_value(fm: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    let line = fm.lines().find_map(|l| l.strip_prefix(&prefix))?;
    let v = line.trim();
    if v.is_empty() {
        return None;
    }
    let b = v.as_bytes();
    if v.len() >= 2 && b[0] == b[v.len() - 1] && (b[0] == b'"' || b[0] == b'\'') {
        return Some(v[1..v.len() - 1].to_string());
    }
    Some(v.to_string())
}

/// Extra page-to-text rules from a profile: a regex and its replacement (`$1` or `\1` for a group).
#[derive(Debug, Clone, Default)]
pub struct PlainRules(Vec<(Regex, String)>);

impl PlainRules {
    /// Compile `[pattern, replacement]` pairs. Patterns are dot-matches-newline.
    ///
    /// # Errors
    /// Returns an error if a pattern is not a valid regular expression.
    pub fn new(pairs: &[(String, String)]) -> Result<Self> {
        let group = const_regex(r"\\(\d)");
        let mut out = Vec::new();
        for (pat, repl) in pairs {
            let re = Regex::new(&format!("(?s){pat}"))
                .map_err(|e| Error::Profile(format!("bad plain rule '{pat}': {e}")))?;
            out.push((re, group.replace_all(repl, "$${$1}").into_owned()));
        }
        Ok(Self(out))
    }
}

static RE_LIQUID: LazyLock<Regex> = LazyLock::new(|| const_regex(r"(?s)\{%.*?%\}|\{\{.*?\}\}"));
static RE_BLOCK_END: LazyLock<Regex> =
    LazyLock::new(|| const_regex(r"</p>|</aside>|</li>|</h\d>|<br\s*/?>"));
static RE_TAG: LazyLock<Regex> = LazyLock::new(|| const_regex(r"<[^>]+>"));
static RE_SPACES: LazyLock<Regex> = LazyLock::new(|| const_regex(r"[ \t]+"));
static RE_BLANKS: LazyLock<Regex> = LazyLock::new(|| const_regex(r"\n\s*\n+"));

/// HTML/Liquid page body to readable text for the model.
///
/// ```
/// use post_kit::text::{to_plain, PlainRules};
/// let t = to_plain("<p>One &amp; two.</p><p>Three.</p>", &PlainRules::default());
/// assert_eq!(t, "One & two.\nThree.");
/// ```
#[must_use]
pub fn to_plain(body: &str, rules: &PlainRules) -> String {
    let mut t = RE_LIQUID.replace_all(body, " ").into_owned();
    for (re, repl) in &rules.0 {
        t = re.replace_all(&t, repl.as_str()).into_owned();
    }
    t = RE_BLOCK_END.replace_all(&t, "\n").into_owned();
    t = RE_TAG.replace_all(&t, "").into_owned();
    t = unescape(&t);
    t = RE_SPACES.replace_all(&t, " ").into_owned();
    t = RE_BLANKS.replace_all(&t, "\n\n").into_owned();
    t.trim().to_string()
}

/// The HTML 4 named entities (from the HTML specification), as code points.
const ENTITIES: [(&str, u32); 252] = [
    ("AElig", 198),
    ("Aacute", 193),
    ("Acirc", 194),
    ("Agrave", 192),
    ("Alpha", 913),
    ("Aring", 197),
    ("Atilde", 195),
    ("Auml", 196),
    ("Beta", 914),
    ("Ccedil", 199),
    ("Chi", 935),
    ("Dagger", 8225),
    ("Delta", 916),
    ("ETH", 208),
    ("Eacute", 201),
    ("Ecirc", 202),
    ("Egrave", 200),
    ("Epsilon", 917),
    ("Eta", 919),
    ("Euml", 203),
    ("Gamma", 915),
    ("Iacute", 205),
    ("Icirc", 206),
    ("Igrave", 204),
    ("Iota", 921),
    ("Iuml", 207),
    ("Kappa", 922),
    ("Lambda", 923),
    ("Mu", 924),
    ("Ntilde", 209),
    ("Nu", 925),
    ("OElig", 338),
    ("Oacute", 211),
    ("Ocirc", 212),
    ("Ograve", 210),
    ("Omega", 937),
    ("Omicron", 927),
    ("Oslash", 216),
    ("Otilde", 213),
    ("Ouml", 214),
    ("Phi", 934),
    ("Pi", 928),
    ("Prime", 8243),
    ("Psi", 936),
    ("Rho", 929),
    ("Scaron", 352),
    ("Sigma", 931),
    ("THORN", 222),
    ("Tau", 932),
    ("Theta", 920),
    ("Uacute", 218),
    ("Ucirc", 219),
    ("Ugrave", 217),
    ("Upsilon", 933),
    ("Uuml", 220),
    ("Xi", 926),
    ("Yacute", 221),
    ("Yuml", 376),
    ("Zeta", 918),
    ("aacute", 225),
    ("acirc", 226),
    ("acute", 180),
    ("aelig", 230),
    ("agrave", 224),
    ("alefsym", 8501),
    ("alpha", 945),
    ("amp", 38),
    ("and", 8743),
    ("ang", 8736),
    ("aring", 229),
    ("asymp", 8776),
    ("atilde", 227),
    ("auml", 228),
    ("bdquo", 8222),
    ("beta", 946),
    ("brvbar", 166),
    ("bull", 8226),
    ("cap", 8745),
    ("ccedil", 231),
    ("cedil", 184),
    ("cent", 162),
    ("chi", 967),
    ("circ", 710),
    ("clubs", 9827),
    ("cong", 8773),
    ("copy", 169),
    ("crarr", 8629),
    ("cup", 8746),
    ("curren", 164),
    ("dArr", 8659),
    ("dagger", 8224),
    ("darr", 8595),
    ("deg", 176),
    ("delta", 948),
    ("diams", 9830),
    ("divide", 247),
    ("eacute", 233),
    ("ecirc", 234),
    ("egrave", 232),
    ("empty", 8709),
    ("emsp", 8195),
    ("ensp", 8194),
    ("epsilon", 949),
    ("equiv", 8801),
    ("eta", 951),
    ("eth", 240),
    ("euml", 235),
    ("euro", 8364),
    ("exist", 8707),
    ("fnof", 402),
    ("forall", 8704),
    ("frac12", 189),
    ("frac14", 188),
    ("frac34", 190),
    ("frasl", 8260),
    ("gamma", 947),
    ("ge", 8805),
    ("gt", 62),
    ("hArr", 8660),
    ("harr", 8596),
    ("hearts", 9829),
    ("hellip", 8230),
    ("iacute", 237),
    ("icirc", 238),
    ("iexcl", 161),
    ("igrave", 236),
    ("image", 8465),
    ("infin", 8734),
    ("int", 8747),
    ("iota", 953),
    ("iquest", 191),
    ("isin", 8712),
    ("iuml", 239),
    ("kappa", 954),
    ("lArr", 8656),
    ("lambda", 955),
    ("lang", 9001),
    ("laquo", 171),
    ("larr", 8592),
    ("lceil", 8968),
    ("ldquo", 8220),
    ("le", 8804),
    ("lfloor", 8970),
    ("lowast", 8727),
    ("loz", 9674),
    ("lrm", 8206),
    ("lsaquo", 8249),
    ("lsquo", 8216),
    ("lt", 60),
    ("macr", 175),
    ("mdash", 8212),
    ("micro", 181),
    ("middot", 183),
    ("minus", 8722),
    ("mu", 956),
    ("nabla", 8711),
    ("nbsp", 160),
    ("ndash", 8211),
    ("ne", 8800),
    ("ni", 8715),
    ("not", 172),
    ("notin", 8713),
    ("nsub", 8836),
    ("ntilde", 241),
    ("nu", 957),
    ("oacute", 243),
    ("ocirc", 244),
    ("oelig", 339),
    ("ograve", 242),
    ("oline", 8254),
    ("omega", 969),
    ("omicron", 959),
    ("oplus", 8853),
    ("or", 8744),
    ("ordf", 170),
    ("ordm", 186),
    ("oslash", 248),
    ("otilde", 245),
    ("otimes", 8855),
    ("ouml", 246),
    ("para", 182),
    ("part", 8706),
    ("permil", 8240),
    ("perp", 8869),
    ("phi", 966),
    ("pi", 960),
    ("piv", 982),
    ("plusmn", 177),
    ("pound", 163),
    ("prime", 8242),
    ("prod", 8719),
    ("prop", 8733),
    ("psi", 968),
    ("quot", 34),
    ("rArr", 8658),
    ("radic", 8730),
    ("rang", 9002),
    ("raquo", 187),
    ("rarr", 8594),
    ("rceil", 8969),
    ("rdquo", 8221),
    ("real", 8476),
    ("reg", 174),
    ("rfloor", 8971),
    ("rho", 961),
    ("rlm", 8207),
    ("rsaquo", 8250),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("scaron", 353),
    ("sdot", 8901),
    ("sect", 167),
    ("shy", 173),
    ("sigma", 963),
    ("sigmaf", 962),
    ("sim", 8764),
    ("spades", 9824),
    ("sub", 8834),
    ("sube", 8838),
    ("sum", 8721),
    ("sup", 8835),
    ("sup1", 185),
    ("sup2", 178),
    ("sup3", 179),
    ("supe", 8839),
    ("szlig", 223),
    ("tau", 964),
    ("there4", 8756),
    ("theta", 952),
    ("thetasym", 977),
    ("thinsp", 8201),
    ("thorn", 254),
    ("tilde", 732),
    ("times", 215),
    ("trade", 8482),
    ("uArr", 8657),
    ("uacute", 250),
    ("uarr", 8593),
    ("ucirc", 251),
    ("ugrave", 249),
    ("uml", 168),
    ("upsih", 978),
    ("upsilon", 965),
    ("uuml", 252),
    ("weierp", 8472),
    ("xi", 958),
    ("yacute", 253),
    ("yen", 165),
    ("yuml", 255),
    ("zeta", 950),
    ("zwj", 8205),
    ("zwnj", 8204),
];

/// Decode the common named entities and numeric references.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let decoded = tail.find(';').filter(|&j| j <= 10).and_then(|j| {
            let name = &tail[..j];
            let ch = if let Some(num) = name.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse().ok(),
                };
                code.and_then(char::from_u32).map(String::from)
            } else {
                ENTITIES
                    .iter()
                    .find(|(n, _)| *n == name)
                    .and_then(|(_, c)| char::from_u32(*c))
                    .map(String::from)
            };
            ch.map(|c| (c, j))
        });
        match decoded {
            Some((c, j)) => {
                out.push_str(&c);
                rest = &tail[j + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Wrap exact-case whole-word occurrences of `word` in a span of class `class`.
fn wrap_token(s: &str, word: &str, class: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (i, _) in s.match_indices(word) {
        if i < last {
            continue;
        }
        let before = s[..i].chars().next_back();
        let after = s[i + word.len()..].chars().next();
        if before.is_some_and(|c| is_word(c) || c == '>')
            || after.is_some_and(|c| is_word(c) || c == '<')
        {
            continue;
        }
        out.push_str(&s[last..i]);
        out.push_str(&format!("<span class=\"{class}\">{word}</span>"));
        last = i + word.len();
    }
    out.push_str(&s[last..]);
    out
}

/// `*italic*` to `<em>`: the opener must be followed, and the closer preceded, by a non-space.
fn italics(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '*' && chars.get(i + 1).is_some_and(|c| !c.is_whitespace()) {
            let close = (i + 2..chars.len())
                .take_while(|&j| chars[j] != '\n')
                .find(|&j| chars[j] == '*' && !chars[j - 1].is_whitespace());
            if let Some(j) = close {
                out.push_str("<em>");
                out.extend(&chars[i + 1..j]);
                out.push_str("</em>");
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Runs of two or more `█` become the redaction markup.
fn redact(s: &str, html: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = 0usize;
    for c in s.chars() {
        if c == '█' {
            run += 1;
            continue;
        }
        flush_run(&mut out, &mut run, html);
        out.push(c);
    }
    flush_run(&mut out, &mut run, html);
    out
}

fn flush_run(out: &mut String, run: &mut usize, html: &str) {
    match *run {
        0 => {}
        1 => out.push('█'),
        _ => out.push_str(html),
    }
    *run = 0;
}

/// Plain text from the model to safe HTML.
///
/// Order matters: escape first, then typography `tokens` (exact-case whole words), then `*italics*`,
/// then the redaction block, then footnote marks (`fn_map` maps the model's local `[^n]` to the
/// page's running number).
///
/// ```
/// use post_kit::text::to_html;
/// let h = to_html("A <b> & ELITA, *quietly*.[^1]", &[("ELITA".into(), "tok".into())], Some(&[(1, 26)].into()), None);
/// assert!(h.contains("&lt;b&gt; &amp;"));
/// assert!(h.contains("<span class=\"tok\">ELITA</span>"));
/// assert!(h.contains("<em>quietly</em>"));
/// assert!(h.contains("title=\"See note 26.\""));
/// ```
#[must_use]
pub fn to_html(
    text: &str,
    tokens: &[(String, String)],
    fn_map: Option<&BTreeMap<u32, u32>>,
    redaction_html: Option<&str>,
) -> String {
    let mut t = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    for (word, class) in tokens {
        t = wrap_token(&t, word, class);
    }
    t = italics(&t);
    if let Some(html) = redaction_html {
        t = redact(&t, html);
    }
    if let Some(map) = fn_map {
        t = RE_FOOTNOTE
            .replace_all(&t, |c: &regex::Captures<'_>| {
                let local: u32 = c[1].parse().unwrap_or(0);
                match map.get(&local) {
                    Some(n) => format!("<span class=\"fn-ref\" title=\"See note {n}.\">{n}</span>"),
                    None => c[0].to_string(),
                }
            })
            .into_owned();
    }
    t
}

/// All of a model answer's text, joined by newlines (for prompts and the review).
#[must_use]
pub fn flatten(obj: &Map<String, Value>) -> String {
    let mut parts = Vec::new();
    for v in obj.values() {
        match v {
            Value::Array(items) => parts.extend(items.iter().map(plain)),
            other => parts.push(plain(other)),
        }
    }
    parts.join("\n")
}

fn plain(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_string)
}

/// A YAML-safe scalar. JSON strings, booleans and numbers are valid YAML, and so is a flat list.
///
/// ```
/// use post_kit::text::yaml_scalar;
/// assert_eq!(yaml_scalar("a \"b\""), r#""a \"b\"""#);
/// assert_eq!(yaml_scalar(&["x", "y"]), r#"["x", "y"]"#);
/// assert_eq!(yaml_scalar(&true), "true");
/// ```
#[must_use]
pub fn yaml_scalar<T: serde::Serialize + ?Sized>(v: &T) -> String {
    match serde_json::to_value(v) {
        // A flow list with ", " between items, which reads better than the compact form.
        Ok(Value::Array(items)) => format!(
            "[{}]",
            items.iter().map(yaml_scalar).collect::<Vec<_>>().join(", ")
        ),
        Ok(other) => serde_json::to_string(&other).unwrap_or_else(|_| "null".to_string()),
        Err(_) => "null".to_string(),
    }
}

/// A lowercase ASCII slug of at most `limit` characters.
///
/// ```
/// assert_eq!(post_kit::text::slug("Why the Glue Joint Failed?!", 60), "why-the-glue-joint-failed");
/// ```
#[must_use]
pub fn slug(s: &str, limit: usize) -> String {
    let ascii: String = unicode_normalization::UnicodeNormalization::nfkd(s)
        .filter(char::is_ascii)
        .collect::<String>()
        .to_lowercase();
    let mut out = String::new();
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let cut: String = out.trim_end_matches('-').chars().take(limit).collect();
    let cut = cut.trim_end_matches('-').to_string();
    if cut.is_empty() {
        "post".to_string()
    } else {
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_escaped_and_typeset() {
        let tokens = vec![
            ("ELITA".to_string(), "tok".to_string()),
            ("THE BROTHERHOOD".to_string(), "b".to_string()),
            ("BROTHERHOOD".to_string(), "b".to_string()),
        ];
        let map: BTreeMap<u32, u32> = [(1, 26)].into();
        let h = to_html("A <script>x</script> & ELITA, *quietly*, ████████.[^1] THE BROTHERHOOD and the BROTHERHOOD", &tokens, Some(&map), Some("<r/>"));
        assert!(!h.contains("<script>"));
        assert!(h.contains("&lt;script&gt;"));
        assert!(h.contains("<span class=\"tok\">ELITA</span>"));
        assert!(h.contains("<em>quietly</em>"));
        assert!(h.contains("<r/>"));
        assert!(h.contains("title=\"See note 26.\""));
        assert!(h.contains("<span class=\"b\">THE BROTHERHOOD</span>"));
        assert!(h.contains("the <span class=\"b\">BROTHERHOOD</span>"));
        assert!(
            !h.contains("<span class=\"b\"><span"),
            "no double wrapping: {h}"
        );
    }

    #[test]
    fn tokens_do_not_match_inside_words() {
        let t = vec![("ELITA".to_string(), "tok".to_string())];
        assert!(!to_html("ELITAS and xELITA", &t, None, None).contains("tok"));
    }

    #[test]
    fn italics_follow_the_markdown_flanking_rule() {
        assert_eq!(italics("a *b* c"), "a <em>b</em> c");
        assert_eq!(italics("2 * 3 * 4"), "2 * 3 * 4");
        assert_eq!(italics("*one* and *two*"), "<em>one</em> and <em>two</em>");
        assert_eq!(italics("no close *here"), "no close *here");
    }

    #[test]
    fn a_single_block_character_is_left_alone() {
        assert_eq!(redact("a █ b ██ c", "R"), "a █ b R c");
    }

    #[test]
    fn plain_text_extraction_with_rules() {
        let rules = PlainRules::new(&[(
            "<aside class=\"x\">(.*?)</aside>".into(),
            "[margin: \\1]".into(),
        )])
        .unwrap();
        let t = to_plain(
            "{% if a %}<p>Hi &mdash; there</p><aside class=\"x\">note</aside>{{ b }}",
            &rules,
        );
        assert_eq!(t, "Hi \u{2014} there\n[margin: note]");
    }

    #[test]
    fn entities() {
        assert_eq!(
            unescape("a &amp; b &#169; &#xA9; &bogus; &"),
            "a & b \u{a9} \u{a9} &bogus; &"
        );
        assert_eq!(
            unescape("tr&aelig;nslators &eacute; &hellip; &frac12;"),
            "tr\u{e6}nslators \u{e9} \u{2026} \u{bd}"
        );
    }

    #[test]
    fn front_matter() {
        assert_eq!(split_front_matter("no fm"), ("", "no fm"));
        let (fm, body) = split_front_matter("---\na: 1\n---\n\nBody");
        assert_eq!((fm, body), ("a: 1", "\nBody"));
        assert_eq!(fm_value("t: 'x y'", "t").as_deref(), Some("x y"));
    }

    #[test]
    fn slug_edge_cases() {
        assert_eq!(slug("???", 60), "post");
        assert_eq!(slug("Café au lait", 60), "cafe-au-lait");
        assert_eq!(slug("a b c d e f", 5), "a-b-c");
    }

    #[test]
    fn flatten_joins_strings_and_arrays() {
        let v: Value = serde_json::json!({"a": "x", "b": ["y", "z"]});
        assert_eq!(flatten(v.as_object().unwrap()), "x\ny\nz");
    }
}
