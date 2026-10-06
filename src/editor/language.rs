use std::path::Path;

/// Languages with tree-sitter highlighters from GPUI Kit's `tree-sitter-languages`
/// feature (plus JSON from the base `tree-sitter` feature). `PlainText` is the
/// fallback when nothing matches. Injection-only grammars (e.g. markdown_inline,
/// jsdoc) are enabled in the crate but not opened as top-level file languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Astro,
    Bash,
    C,
    CMake,
    CSharp,
    Cpp,
    Css,
    Diff,
    Ejs,
    Elixir,
    Erb,
    Go,
    GraphQL,
    Html,
    Java,
    JavaScript,
    Json,
    Kotlin,
    Lua,
    Make,
    Markdown,
    Php,
    Proto,
    Python,
    Ruby,
    Rust,
    Scala,
    Sql,
    Svelte,
    Swift,
    Toml,
    Tsx,
    TypeScript,
    Yaml,
    Zig,
    PlainText,
}

impl Language {
    pub fn from_path(path: &Path) -> Self {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match file_name.as_str() {
            "makefile" | "gnumakefile" => return Language::Make,
            "cmakelists.txt" => return Language::CMake,
            _ => {}
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();

        // Compound suffixes that `Path::extension` only sees partially.
        if file_name.ends_with(".html.erb") || file_name.ends_with(".js.erb") {
            return Language::Erb;
        }

        match ext.as_str() {
            "astro" => Language::Astro,
            "sh" | "bash" | "zsh" | "ksh" => Language::Bash,
            "c" | "h" => Language::C,
            "cmake" => Language::CMake,
            "cs" => Language::CSharp,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "h++" | "c++" => Language::Cpp,
            "css" | "scss" => Language::Css,
            "diff" | "patch" => Language::Diff,
            "ejs" => Language::Ejs,
            "ex" | "exs" => Language::Elixir,
            "erb" => Language::Erb,
            "go" => Language::Go,
            "graphql" | "gql" => Language::GraphQL,
            "html" | "htm" => Language::Html,
            "java" => Language::Java,
            "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
            "json" | "jsonc" | "json5" => Language::Json,
            "kt" | "kts" | "ktm" => Language::Kotlin,
            "lua" => Language::Lua,
            "mk" | "mak" => Language::Make,
            "md" | "markdown" | "mdx" => Language::Markdown,
            "php" | "php3" | "php4" | "php5" | "phtml" => Language::Php,
            "proto" => Language::Proto,
            "py" | "pyi" | "pyw" => Language::Python,
            "rb" | "rake" | "gemspec" => Language::Ruby,
            "rs" => Language::Rust,
            "scala" | "sc" => Language::Scala,
            "sql" => Language::Sql,
            "svelte" => Language::Svelte,
            "swift" => Language::Swift,
            "toml" => Language::Toml,
            "tsx" => Language::Tsx,
            "ts" | "mts" | "cts" => Language::TypeScript,
            "yml" | "yaml" => Language::Yaml,
            "zig" | "zon" => Language::Zig,
            _ => Language::PlainText,
        }
    }

    /// Language name understood by the GPUI Kit editor's tree-sitter highlighter.
    pub fn highlighter_name(self) -> &'static str {
        match self {
            Language::Astro => "astro",
            Language::Bash => "bash",
            Language::C => "c",
            Language::CMake => "cmake",
            Language::CSharp => "csharp",
            Language::Cpp => "cpp",
            Language::Css => "css",
            Language::Diff => "diff",
            Language::Ejs => "ejs",
            Language::Elixir => "elixir",
            Language::Erb => "erb",
            Language::Go => "go",
            Language::GraphQL => "graphql",
            Language::Html => "html",
            Language::Java => "java",
            Language::JavaScript => "javascript",
            Language::Json => "json",
            Language::Kotlin => "kotlin",
            Language::Lua => "lua",
            Language::Make => "make",
            Language::Markdown => "markdown",
            Language::Php => "php",
            Language::Proto => "proto",
            Language::Python => "python",
            Language::Ruby => "ruby",
            Language::Rust => "rust",
            Language::Scala => "scala",
            Language::Sql => "sql",
            Language::Svelte => "svelte",
            Language::Swift => "swift",
            Language::Toml => "toml",
            Language::Tsx => "tsx",
            Language::TypeScript => "typescript",
            Language::Yaml => "yaml",
            Language::Zig => "zig",
            Language::PlainText => "text",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Language::Astro => "Astro",
            Language::Bash => "Bash",
            Language::C => "C",
            Language::CMake => "CMake",
            Language::CSharp => "C#",
            Language::Cpp => "C++",
            Language::Css => "CSS",
            Language::Diff => "Diff",
            Language::Ejs => "EJS",
            Language::Elixir => "Elixir",
            Language::Erb => "ERB",
            Language::Go => "Go",
            Language::GraphQL => "GraphQL",
            Language::Html => "HTML",
            Language::Java => "Java",
            Language::JavaScript => "JavaScript",
            Language::Json => "JSON",
            Language::Kotlin => "Kotlin",
            Language::Lua => "Lua",
            Language::Make => "Make",
            Language::Markdown => "Markdown",
            Language::Php => "PHP",
            Language::Proto => "Protocol Buffers",
            Language::Python => "Python",
            Language::Ruby => "Ruby",
            Language::Rust => "Rust",
            Language::Scala => "Scala",
            Language::Sql => "SQL",
            Language::Svelte => "Svelte",
            Language::Swift => "Swift",
            Language::Toml => "TOML",
            Language::Tsx => "TSX",
            Language::TypeScript => "TypeScript",
            Language::Yaml => "YAML",
            Language::Zig => "Zig",
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
        assert_eq!(Language::from_path(Path::new("lib.rs")), Language::Rust);
        assert_eq!(Language::from_path(Path::new("main.go")), Language::Go);
        assert_eq!(Language::from_path(Path::new("app.py")), Language::Python);
        assert_eq!(Language::from_path(Path::new("docker-compose.yml")), Language::Yaml);
        assert_eq!(Language::from_path(Path::new("Cargo.toml")), Language::Toml);
        assert_eq!(Language::from_path(Path::new("README.md")), Language::Markdown);
        assert_eq!(Language::from_path(Path::new("Makefile")), Language::Make);
        assert_eq!(Language::from_path(Path::new("CMakeLists.txt")), Language::CMake);
        assert_eq!(Language::from_path(Path::new("notes.xyz")), Language::PlainText);
    }
}
