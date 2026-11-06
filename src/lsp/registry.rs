//! Language → default language-server command mapping.
//!
//! Servers are **not** inside the app binary. Defaults are command names
//! resolved app-local (`…/isengard/lsp/bin`) then `PATH`; users can override
//! or disable per language in `config.json` → `lsp.languages`.

use crate::config::LspSettings;
use crate::editor::language::Language;

/// Built-in default for a language (static).
#[derive(Debug, Clone, Copy)]
pub struct ServerSpec {
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub language_id: &'static str,
}

/// Concrete spawn info after applying [`LspSettings`] overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedServerSpec {
    pub command: String,
    pub args: Vec<String>,
    pub language_id: String,
}

impl ResolvedServerSpec {
    /// Process cache key — same command shares one server (e.g. JS/TS/TSX).
    pub fn process_key(&self) -> &str {
        &self.command
    }
}

const RUST: ServerSpec = ServerSpec {
    command: "rust-analyzer",
    args: &[],
    language_id: "rust",
};
const GO: ServerSpec = ServerSpec {
    command: "gopls",
    args: &[],
    language_id: "go",
};
const PYTHON: ServerSpec = ServerSpec {
    command: "pylsp",
    args: &[],
    language_id: "python",
};
const TS_SERVER: ServerSpec = ServerSpec {
    command: "typescript-language-server",
    args: &["--stdio"],
    language_id: "typescript",
};
const JS_SERVER: ServerSpec = ServerSpec {
    command: "typescript-language-server",
    args: &["--stdio"],
    language_id: "javascript",
};
const TSX_SERVER: ServerSpec = ServerSpec {
    command: "typescript-language-server",
    args: &["--stdio"],
    language_id: "typescriptreact",
};
const CLANGD: ServerSpec = ServerSpec {
    command: "clangd",
    args: &[],
    language_id: "c",
};
const CLANGD_CPP: ServerSpec = ServerSpec {
    command: "clangd",
    args: &[],
    language_id: "cpp",
};
const JSON: ServerSpec = ServerSpec {
    command: "vscode-json-language-server",
    args: &["--stdio"],
    language_id: "json",
};
const CSS: ServerSpec = ServerSpec {
    command: "vscode-css-language-server",
    args: &["--stdio"],
    language_id: "css",
};
const HTML: ServerSpec = ServerSpec {
    command: "vscode-html-language-server",
    args: &["--stdio"],
    language_id: "html",
};
const YAML: ServerSpec = ServerSpec {
    command: "yaml-language-server",
    args: &["--stdio"],
    language_id: "yaml",
};
const TOML: ServerSpec = ServerSpec {
    command: "taplo",
    args: &["lsp", "stdio"],
    language_id: "toml",
};
const BASH: ServerSpec = ServerSpec {
    command: "bash-language-server",
    args: &["start"],
    language_id: "shellscript",
};
const DOCKER: ServerSpec = ServerSpec {
    command: "docker-langserver",
    args: &["--stdio"],
    language_id: "dockerfile",
};
const LUA: ServerSpec = ServerSpec {
    command: "lua-language-server",
    args: &[],
    language_id: "lua",
};
const ZIG: ServerSpec = ServerSpec {
    command: "zls",
    args: &[],
    language_id: "zig",
};
const SWIFT: ServerSpec = ServerSpec {
    command: "sourcekit-lsp",
    args: &[],
    language_id: "swift",
};
const RUBY: ServerSpec = ServerSpec {
    command: "ruby-lsp",
    args: &[],
    language_id: "ruby",
};
const PHP: ServerSpec = ServerSpec {
    command: "intelephense",
    args: &["--stdio"],
    language_id: "php",
};
const KOTLIN: ServerSpec = ServerSpec {
    command: "kotlin-language-server",
    args: &[],
    language_id: "kotlin",
};
const JAVA: ServerSpec = ServerSpec {
    command: "jdtls",
    args: &[],
    language_id: "java",
};
const CSHARP: ServerSpec = ServerSpec {
    command: "csharp-ls",
    args: &[],
    language_id: "csharp",
};
const SCALA: ServerSpec = ServerSpec {
    command: "metals",
    args: &[],
    language_id: "scala",
};
const ELIXIR: ServerSpec = ServerSpec {
    command: "elixir-ls",
    args: &[],
    language_id: "elixir",
};
const SVELTE: ServerSpec = ServerSpec {
    command: "svelteserver",
    args: &["--stdio"],
    language_id: "svelte",
};
const ASTRO: ServerSpec = ServerSpec {
    command: "astro-ls",
    args: &["--stdio"],
    language_id: "astro",
};
const VUE: ServerSpec = ServerSpec {
    command: "vue-language-server",
    args: &["--stdio"],
    language_id: "vue",
};
const ANGULAR: ServerSpec = ServerSpec {
    command: "ngserver",
    args: &["--stdio"],
    language_id: "html",
};
const SCSS: ServerSpec = ServerSpec {
    command: "vscode-css-language-server",
    args: &["--stdio"],
    language_id: "scss",
};
const LESS: ServerSpec = ServerSpec {
    command: "vscode-css-language-server",
    args: &["--stdio"],
    language_id: "less",
};
const PRISMA: ServerSpec = ServerSpec {
    command: "prisma-language-server",
    args: &["--stdio"],
    language_id: "prisma",
};
const XML: ServerSpec = ServerSpec {
    command: "lemminx",
    args: &[],
    language_id: "xml",
};
const DART: ServerSpec = ServerSpec {
    command: "dart",
    args: &["language-server", "--protocol=lsp"],
    language_id: "dart",
};
const HASKELL: ServerSpec = ServerSpec {
    command: "haskell-language-server-wrapper",
    args: &["--lsp"],
    language_id: "haskell",
};
const OCAML: ServerSpec = ServerSpec {
    command: "ocamllsp",
    args: &[],
    language_id: "ocaml",
};
const R_LANG: ServerSpec = ServerSpec {
    command: "R",
    args: &["--slave", "-e", "languageserver::run()"],
    language_id: "r",
};
const PERL: ServerSpec = ServerSpec {
    command: "perlnavigator",
    args: &["--stdio"],
    language_id: "perl",
};
const CLOJURE: ServerSpec = ServerSpec {
    command: "clojure-lsp",
    args: &[],
    language_id: "clojure",
};
const ERLANG: ServerSpec = ServerSpec {
    command: "erlang_ls",
    args: &[],
    language_id: "erlang",
};
const NIM: ServerSpec = ServerSpec {
    command: "nimlangserver",
    args: &[],
    language_id: "nim",
};
const TERRAFORM: ServerSpec = ServerSpec {
    command: "terraform-ls",
    args: &["serve"],
    language_id: "terraform",
};
const NIX: ServerSpec = ServerSpec {
    command: "nil",
    args: &[],
    language_id: "nix",
};
const GLEAM: ServerSpec = ServerSpec {
    command: "gleam",
    args: &["lsp"],
    language_id: "gleam",
};
const OBJC: ServerSpec = ServerSpec {
    command: "clangd",
    args: &[],
    language_id: "objective-c",
};
const LATEX: ServerSpec = ServerSpec {
    command: "texlab",
    args: &[],
    language_id: "latex",
};
const TYPST: ServerSpec = ServerSpec {
    command: "tinymist",
    args: &[],
    language_id: "typst",
};
const ELM: ServerSpec = ServerSpec {
    command: "elm-language-server",
    args: &[],
    language_id: "elm",
};
const PURESCRIPT: ServerSpec = ServerSpec {
    command: "purescript-language-server",
    args: &["--stdio"],
    language_id: "purescript",
};
const SOLIDITY: ServerSpec = ServerSpec {
    command: "nomicfoundation-solidity-language-server",
    args: &["--stdio"],
    language_id: "solidity",
};
const FSHARP: ServerSpec = ServerSpec {
    command: "fsautocomplete",
    args: &[],
    language_id: "fsharp",
};
const MARKDOWN: ServerSpec = ServerSpec {
    command: "marksman",
    args: &["server"],
    language_id: "markdown",
};
const CMAKE: ServerSpec = ServerSpec {
    command: "cmake-language-server",
    args: &[],
    language_id: "cmake",
};
const PROTO: ServerSpec = ServerSpec {
    command: "bufls",
    args: &["serve"],
    language_id: "proto",
};
const GRAPHQL: ServerSpec = ServerSpec {
    command: "graphql-lsp",
    args: &["server", "--method", "stream"],
    language_id: "graphql",
};
const SQL: ServerSpec = ServerSpec {
    command: "sql-language-server",
    args: &["up", "--method", "stdio"],
    language_id: "sql",
};

/// Stable config.json key for `lsp.languages` (not always the highlighter name).
pub fn config_key(language: Language) -> &'static str {
    match language {
        Language::Dockerfile => "dockerfile",
        Language::Svg => "svg",
        Language::PlainText => "plaintext",
        Language::Angular => "angular",
        Language::Scss => "scss",
        Language::Less => "less",
        Language::Prisma => "prisma",
        Language::Xml => "xml",
        Language::Dart => "dart",
        Language::Haskell => "haskell",
        Language::OCaml => "ocaml",
        Language::Julia => "julia",
        Language::R => "r",
        Language::Perl => "perl",
        Language::Clojure => "clojure",
        Language::Erlang => "erlang",
        Language::Nim => "nim",
        Language::Terraform => "terraform",
        Language::Nix => "nix",
        Language::Gleam => "gleam",
        Language::PowerShell => "powershell",
        Language::Groovy => "groovy",
        Language::ObjectiveC => "objective-c",
        Language::Latex => "latex",
        Language::Typst => "typst",
        Language::Elm => "elm",
        Language::PureScript => "purescript",
        Language::Solidity => "solidity",
        Language::Ini => "ini",
        Language::Pug => "pug",
        Language::Handlebars => "handlebars",
        Language::CoffeeScript => "coffeescript",
        Language::FSharp => "fsharp",

        other => other.highlighter_name(),
    }
}

/// Built-in default, if any.
pub fn default_server(language: Language) -> Option<&'static ServerSpec> {
    Some(match language {
        Language::Rust => &RUST,
        Language::Go => &GO,
        Language::Python => &PYTHON,
        Language::TypeScript => &TS_SERVER,
        Language::JavaScript => &JS_SERVER,
        Language::Tsx => &TSX_SERVER,
        Language::C => &CLANGD,
        Language::Cpp => &CLANGD_CPP,
        Language::Json => &JSON,
        Language::Css => &CSS,
        Language::Html => &HTML,
        Language::Yaml => &YAML,
        Language::Toml => &TOML,
        Language::Bash => &BASH,
        Language::Dockerfile => &DOCKER,
        Language::Lua => &LUA,
        Language::Zig => &ZIG,
        Language::Swift => &SWIFT,
        Language::Ruby => &RUBY,
        Language::Php => &PHP,
        Language::Kotlin => &KOTLIN,
        Language::Java => &JAVA,
        Language::CSharp => &CSHARP,
        Language::Scala => &SCALA,
        Language::Elixir => &ELIXIR,
        Language::Svelte => &SVELTE,
        Language::Astro => &ASTRO,
        Language::Vue => &VUE,
        Language::Markdown => &MARKDOWN,
        Language::CMake => &CMAKE,
        Language::Proto => &PROTO,
        Language::GraphQL => &GRAPHQL,
        Language::Sql => &SQL,
        Language::Angular => &ANGULAR,
        Language::Scss => &SCSS,
        Language::Less => &LESS,
        Language::Prisma => &PRISMA,
        Language::Xml => &XML,
        Language::Dart => &DART,
        Language::Haskell => &HASKELL,
        Language::OCaml => &OCAML,
        Language::R => &R_LANG,
        Language::Perl => &PERL,
        Language::Clojure => &CLOJURE,
        Language::Erlang => &ERLANG,
        Language::Nim => &NIM,
        Language::Terraform => &TERRAFORM,
        Language::Nix => &NIX,
        Language::Gleam => &GLEAM,
        Language::ObjectiveC => &OBJC,
        Language::Latex => &LATEX,
        Language::Typst => &TYPST,
        Language::Elm => &ELM,
        Language::PureScript => &PURESCRIPT,
        Language::Solidity => &SOLIDITY,
        Language::FSharp => &FSHARP,
        Language::Diff
        | Language::Make
        | Language::Ejs
        | Language::Erb
        | Language::Svg
        | Language::Julia
        | Language::PowerShell
        | Language::Groovy
        | Language::Ini
        | Language::Pug
        | Language::Handlebars
        | Language::CoffeeScript
        | Language::PlainText => return None,
    })
}

/// Unique default server commands from the built-in language map (sorted).
/// Kept for tests / a future full gallery — status bar uses a short subset.
#[allow(dead_code)]
pub fn catalog_commands() -> Vec<&'static str> {
    use Language::*;
    let languages = [
        Astro, Bash, C, CMake, CSharp, Cpp, Css, Diff, Dockerfile, Ejs, Elixir, Erb, Go, GraphQL,
        Html, Java, JavaScript, Json, Kotlin, Lua, Make, Markdown, Svg, Php, Proto, Python, Ruby,
        Rust, Scala, Sql, Svelte, Swift, Toml, Tsx, TypeScript, Vue, Yaml, Zig, PlainText, Angular,
        Scss, Less, Prisma, Xml, Dart, Haskell, OCaml, R, Perl, Clojure, Erlang, Nim, Terraform,
        Nix, Gleam, ObjectiveC, Latex, Typst, Elm, PureScript, Solidity, FSharp,
    ];
    let mut set = std::collections::BTreeSet::new();
    for language in languages {
        if let Some(spec) = default_server(language) {
            set.insert(spec.command);
        }
    }
    set.into_iter().collect()
}

/// ngserver loads `typescript` and `@angular/language-service` from probe
/// locations; point both at the app-local install.
fn angular_probe_args() -> Vec<String> {
    let dir = super::install::pkg_dir(&super::install::lsp_root(), "angular-language-server")
        .join("node_modules");
    let dir = dir.to_string_lossy().into_owned();
    vec![
        "--tsProbeLocations".into(),
        dir.clone(),
        "--ngProbeLocations".into(),
        dir,
    ]
}

/// Resolve spawn info from defaults + [`LspSettings`].
pub fn resolve(language: Language, settings: &LspSettings) -> Option<ResolvedServerSpec> {
    if !settings.enabled {
        return None;
    }

    let key = config_key(language);
    let override_cfg = settings.languages.get(key);
    if override_cfg.is_some_and(|o| o.enabled == Some(false)) {
        return None;
    }

    let default = default_server(language);
    let command = override_cfg
        .and_then(|o| o.command.clone())
        .filter(|c| !c.trim().is_empty())
        .or_else(|| default.map(|d| d.command.to_string()))?;

    let language_id = default
        .map(|d| d.language_id.to_string())
        .unwrap_or_else(|| key.to_string());

    let args = override_cfg
        .and_then(|o| o.args.clone())
        .unwrap_or_else(|| {
            let mut args: Vec<String> = default
                .map(|d| d.args.iter().map(|s| (*s).to_string()).collect())
                .unwrap_or_default();
            if command == "ngserver" {
                args.extend(angular_probe_args());
            }
            args
        });

    Some(ResolvedServerSpec {
        command,
        args,
        language_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LspLanguageSettings, LspSettings};
    use std::collections::BTreeMap;

    #[test]
    fn rust_defaults_to_rust_analyzer() {
        let spec = resolve(Language::Rust, &LspSettings::default()).unwrap();
        assert_eq!(spec.command, "rust-analyzer");
        assert_eq!(spec.language_id, "rust");
    }

    #[test]
    fn typescript_family_shares_command() {
        let settings = LspSettings::default();
        let ts = resolve(Language::TypeScript, &settings).unwrap();
        let js = resolve(Language::JavaScript, &settings).unwrap();
        let tsx = resolve(Language::Tsx, &settings).unwrap();
        assert_eq!(ts.command, js.command);
        assert_eq!(ts.command, tsx.command);
        assert_eq!(ts.language_id, "typescript");
        assert_eq!(js.language_id, "javascript");
        assert_eq!(tsx.language_id, "typescriptreact");
    }

    #[test]
    fn vue_uses_vue_language_server() {
        let spec = resolve(Language::Vue, &LspSettings::default()).unwrap();
        assert_eq!(spec.command, "vue-language-server");
        assert_eq!(spec.language_id, "vue");
    }

    #[test]
    fn aliased_languages_have_distinct_config_keys() {
        let all = [
            Language::Html,
            Language::Angular,
            Language::Css,
            Language::Scss,
            Language::Less,
            Language::Toml,
            Language::Ini,
            Language::C,
            Language::ObjectiveC,
        ];
        let keys: std::collections::BTreeSet<_> = all.iter().map(|l| config_key(*l)).collect();
        assert_eq!(keys.len(), all.len());
        let angular = resolve(Language::Angular, &LspSettings::default()).unwrap();
        assert_eq!(angular.command, "ngserver");
        assert!(angular.args.contains(&"--ngProbeLocations".to_string()));
    }

    #[test]
    fn master_switch_disables_all() {
        let settings = LspSettings {
            enabled: false,
            ..Default::default()
        };
        assert!(resolve(Language::Rust, &settings).is_none());
        assert!(resolve(Language::Go, &settings).is_none());
    }

    #[test]
    fn per_language_disable() {
        let mut languages = BTreeMap::new();
        languages.insert(
            "python".into(),
            LspLanguageSettings {
                enabled: Some(false),
                ..Default::default()
            },
        );
        let settings = LspSettings {
            languages,
            ..Default::default()
        };
        assert!(resolve(Language::Python, &settings).is_none());
        assert!(resolve(Language::Rust, &settings).is_some());
    }

    #[test]
    fn command_override() {
        let mut languages = BTreeMap::new();
        languages.insert(
            "python".into(),
            LspLanguageSettings {
                command: Some("pyright-langserver".into()),
                args: Some(vec!["--stdio".into()]),
                ..Default::default()
            },
        );
        let settings = LspSettings {
            languages,
            ..Default::default()
        };
        let spec = resolve(Language::Python, &settings).unwrap();
        assert_eq!(spec.command, "pyright-langserver");
        assert_eq!(spec.args, vec!["--stdio"]);
    }

    #[test]
    fn custom_only_language_needs_command() {
        assert!(resolve(Language::PlainText, &LspSettings::default()).is_none());
        let mut languages = BTreeMap::new();
        languages.insert(
            "plaintext".into(),
            LspLanguageSettings {
                command: Some("my-lsp".into()),
                args: Some(vec![]),
                enabled: Some(true),
            },
        );
        let settings = LspSettings {
            languages,
            ..Default::default()
        };
        let spec = resolve(Language::PlainText, &settings).unwrap();
        assert_eq!(spec.command, "my-lsp");
    }

    #[test]
    fn dockerfile_config_key_is_not_bash() {
        assert_eq!(config_key(Language::Dockerfile), "dockerfile");
        assert!(default_server(Language::Dockerfile).is_some());
    }

    #[test]
    fn catalog_dedupes_shared_commands() {
        let cmds = catalog_commands();
        assert!(cmds.contains(&"typescript-language-server"));
        assert!(cmds.contains(&"rust-analyzer"));
        // JS/TS/TSX share one command — only once.
        assert_eq!(
            cmds.iter()
                .filter(|c| **c == "typescript-language-server")
                .count(),
            1
        );
    }
}
