use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use once_cell::sync::Lazy;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use super::language::Language;

/// Capture names we recognize. tree-sitter matches these by prefix, so e.g.
/// `function.method` in a query resolves to `function` unless listed itself.
pub const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "escape",
    "function",
    "keyword",
    "number",
    "operator",
    "property",
    "punctuation",
    "string",
    "string.special.key",
    "tag",
    "type",
    "variable",
    "variable.builtin",
    "variable.parameter",
    "namespace",
    // CSS at-rules
    "media",
    "import",
    "charset",
    "keyframes",
    "supports",
];

/// The upstream JSON query captures keys as both `@string.special.key` and `@string`,
/// and `@string` wins. This variant only marks non-key strings as `@string`.
const JSON_HIGHLIGHTS_QUERY: &str = r#"
(pair key: (string) @string.special.key)
(pair value: (string) @string)
(array (string) @string)
(document (string) @string)
(number) @number
[(null) (true) (false)] @constant.builtin
(escape_sequence) @escape
(comment) @comment
"#;

/// A contiguous byte range of the source and the innermost highlight applied to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    pub range: Range<usize>,
    /// Index into [`HIGHLIGHT_NAMES`]; `None` for unhighlighted text.
    pub highlight: Option<usize>,
}

static CONFIGS: Lazy<HashMap<Language, HighlightConfiguration>> = Lazy::new(|| {
    let ts_highlights = format!(
        "{}\n{}",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        tree_sitter_javascript::HIGHLIGHT_QUERY
    );
    let ts_locals = format!(
        "{}\n{}",
        tree_sitter_typescript::LOCALS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY
    );
    let tsx_highlights = format!("{ts_highlights}\n{}", tree_sitter_javascript::JSX_HIGHLIGHT_QUERY);
    let js_highlights = format!(
        "{}\n{}",
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
        tree_sitter_javascript::HIGHLIGHT_QUERY
    );

    let specs: [(Language, tree_sitter::Language, &str, &str, &str); 6] = [
        (
            Language::JavaScript,
            tree_sitter_javascript::LANGUAGE.into(),
            &js_highlights,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        ),
        (
            Language::TypeScript,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            &ts_highlights,
            tree_sitter_javascript::INJECTIONS_QUERY,
            &ts_locals,
        ),
        (
            Language::Tsx,
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            &tsx_highlights,
            tree_sitter_javascript::INJECTIONS_QUERY,
            &ts_locals,
        ),
        (
            Language::Html,
            tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY,
            "",
        ),
        (
            Language::Css,
            tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        (
            Language::Json,
            tree_sitter_json::LANGUAGE.into(),
            JSON_HIGHLIGHTS_QUERY,
            "",
            "",
        ),
    ];

    let mut configs = HashMap::new();
    for (lang, ts_lang, highlights, injections, locals) in specs {
        match HighlightConfiguration::new(ts_lang, lang.display_name(), highlights, injections, locals) {
            Ok(mut config) => {
                config.configure(HIGHLIGHT_NAMES);
                configs.insert(lang, config);
            }
            Err(err) => log::error!("highlight config for {lang:?} failed: {err}"),
        }
    }
    configs
});

thread_local! {
    static HIGHLIGHTER: RefCell<Highlighter> = RefCell::new(Highlighter::new());
}

/// Highlights `source`, returning non-overlapping spans that cover the whole text.
/// Falls back to a single unhighlighted span for plain text or on error.
pub fn highlight(source: &str, lang: Language) -> Vec<HighlightSpan> {
    let plain = || {
        vec![HighlightSpan {
            range: 0..source.len(),
            highlight: None,
        }]
    };
    let Some(config) = CONFIGS.get(&lang) else {
        return plain();
    };

    HIGHLIGHTER.with_borrow_mut(|highlighter| {
        let events = match highlighter.highlight(config, source.as_bytes(), None, |name| {
            Language::from_name(name).and_then(|l| CONFIGS.get(&l))
        }) {
            Ok(events) => events,
            Err(err) => {
                log::warn!("highlighting failed: {err}");
                return plain();
            }
        };

        let mut spans = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut covered = 0;
        for event in events {
            match event {
                Ok(HighlightEvent::HighlightStart(h)) => stack.push(h.0),
                Ok(HighlightEvent::HighlightEnd) => {
                    stack.pop();
                }
                Ok(HighlightEvent::Source { start, end }) => {
                    if start < end {
                        push_span(&mut spans, start..end, stack.last().copied());
                        covered = end;
                    }
                }
                Err(err) => {
                    log::warn!("highlighting aborted: {err}");
                    break;
                }
            }
        }
        if covered < source.len() {
            push_span(&mut spans, covered..source.len(), None);
        }
        spans
    })
}

/// Appends a span, merging with the previous one when the highlight is identical.
fn push_span(spans: &mut Vec<HighlightSpan>, range: Range<usize>, highlight: Option<usize>) {
    if let Some(last) = spans.last_mut() {
        if last.highlight == highlight && last.range.end == range.start {
            last.range.end = range.end;
            return;
        }
    }
    spans.push(HighlightSpan { range, highlight });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captures(source: &str, lang: Language) -> Vec<(&str, Option<&'static str>)> {
        highlight(source, lang)
            .into_iter()
            .map(|s| (&source[s.range], s.highlight.map(|h| HIGHLIGHT_NAMES[h])))
            .collect()
    }

    fn assert_covers(source: &str, lang: Language) {
        let spans = highlight(source, lang);
        let mut pos = 0;
        for s in &spans {
            assert_eq!(s.range.start, pos, "spans must be contiguous");
            pos = s.range.end;
        }
        assert_eq!(pos, source.len());
    }

    #[test]
    fn all_configs_compile() {
        for lang in [
            Language::JavaScript,
            Language::TypeScript,
            Language::Tsx,
            Language::Html,
            Language::Css,
            Language::Json,
        ] {
            assert!(CONFIGS.contains_key(&lang), "{lang:?} config missing");
        }
    }

    #[test]
    fn javascript() {
        let src = "const x = 'hi'; // note\nfunction foo() { return 42; }\n";
        let caps = captures(src, Language::JavaScript);
        assert!(caps.contains(&("const", Some("keyword"))));
        assert!(caps.contains(&("'hi'", Some("string"))));
        assert!(caps.contains(&("// note", Some("comment"))));
        assert!(caps.contains(&("foo", Some("function"))));
        assert!(caps.contains(&("42", Some("number"))));
        assert_covers(src, Language::JavaScript);
    }

    #[test]
    fn typescript() {
        let src = "interface A { b: string }\nlet n: number = 1;\n";
        let caps = captures(src, Language::TypeScript);
        assert!(caps.contains(&("interface", Some("keyword"))));
        assert!(caps.contains(&("A", Some("type"))));
        assert_covers(src, Language::TypeScript);
    }

    #[test]
    fn html_with_injected_script() {
        let src = "<div class=\"a\"><script>const y = 1;</script></div>";
        let caps = captures(src, Language::Html);
        assert!(caps.contains(&("div", Some("tag"))));
        assert!(caps.contains(&("class", Some("attribute"))));
        assert!(caps.contains(&("const", Some("keyword"))), "{caps:?}");
        assert_covers(src, Language::Html);
    }

    #[test]
    fn json_and_css() {
        let caps = captures("{\"k\": [1, true, \"v\"], \"o\": \"s\"}", Language::Json);
        assert!(caps.contains(&("\"k\"", Some("string.special.key"))), "{caps:?}");
        assert!(caps.contains(&("\"v\"", Some("string"))));
        assert!(caps.contains(&("\"s\"", Some("string"))));
        assert!(caps.contains(&("1", Some("number"))));
        assert!(caps.contains(&("true", Some("constant.builtin"))));

        let caps = captures("@media screen { a { color: red; } }", Language::Css);
        assert!(caps.contains(&("@media", Some("keyword"))), "{caps:?}");
        assert!(caps.contains(&("color", Some("property"))));
    }

    #[test]
    fn plain_text_and_unicode() {
        assert_covers("héllo wörld", Language::PlainText);
        assert_covers("const s = 'héllo 🌍';", Language::JavaScript);
        assert_covers("", Language::JavaScript);
    }
}
