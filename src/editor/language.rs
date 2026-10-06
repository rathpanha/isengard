use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    JavaScript,
    TypeScript,
    Tsx,
    Html,
    Css,
    Json,
    PlainText,
}

impl Language {
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match ext.as_str() {
            "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
            "ts" | "mts" | "cts" => Language::TypeScript,
            "tsx" => Language::Tsx,
            "html" | "htm" => Language::Html,
            "css" => Language::Css,
            "json" => Language::Json,
            _ => Language::PlainText,
        }
    }

    /// Language name understood by the GPUI Kit editor's tree-sitter highlighter.
    pub fn highlighter_name(self) -> &'static str {
        match self {
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::Html => "html",
            Language::Css => "css",
            Language::Json => "json",
            Language::PlainText => "text",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Tsx => "TSX",
            Language::Html => "HTML",
            Language::Css => "CSS",
            Language::Json => "JSON",
            Language::PlainText => "Plain Text",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_language_from_extension() {
        assert_eq!(
            Language::from_path(Path::new("a/app.JS")),
            Language::JavaScript
        );
        assert_eq!(Language::from_path(Path::new("x.tsx")), Language::Tsx);
        assert_eq!(Language::from_path(Path::new("index.html")), Language::Html);
        assert_eq!(
            Language::from_path(Path::new("Makefile")),
            Language::PlainText
        );
    }
}
