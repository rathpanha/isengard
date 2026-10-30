//! Bun is the app's only JS runtime: the Claude agent adapter, npm-packaged
//! language servers and `bun fmt` all run on it. Resolution is app-local
//! (`…/isengard/lsp/bin`), then `PATH`, then `~/.bun/bin` (a Finder launch has a
//! bare `PATH`). Install is a `recipe_for_command("bun")` download — never global.

use std::fs;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// `bun`, if installed anywhere we look.
pub fn bun_path() -> Option<PathBuf> {
    crate::lsp::resolve_command("bun").or_else(|| {
        let name = if cfg!(windows) { "bun.exe" } else { "bun" };
        let home = dirs::home_dir()?.join(".bun").join("bin").join(name);
        home.is_file().then_some(home)
    })
}

/// Package cache kept inside the app data dir so installs never write to
/// `~/.bun/install/cache`.
pub fn bun_cache_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("isengard")
        .join("runtime")
        .join("bun-cache")
}

/// Run a `#!/usr/bin/env node` script on Bun: `(bun, [script, args…])`. Anything
/// else (native binaries, no Bun) comes back unchanged.
///
/// ponytail: unix only — Windows `.cmd` shims keep running on whatever they call.
pub fn run_on_bun(command: PathBuf, args: Vec<String>) -> (PathBuf, Vec<String>) {
    let script = script_behind(&command);
    match bun_path() {
        Some(bun) if has_node_shebang(&script) => {
            let mut full = vec![script.to_string_lossy().into_owned()];
            full.extend(args);
            (bun, full)
        }
        _ => (command, args),
    }
}

/// Our `lsp/bin` wrappers are `#!/bin/sh\nexec "<target>" "$@"`; follow one hop.
fn script_behind(path: &Path) -> PathBuf {
    let head = read_head(path, 4096);
    let mut lines = head.lines();
    if lines.next() == Some("#!/bin/sh")
        && let Some(line) = lines.next()
        && let Some(rest) = line.strip_prefix("exec \"")
        && let Some(target) = rest.strip_suffix("\" \"$@\"")
    {
        return PathBuf::from(target);
    }
    path.to_path_buf()
}

fn has_node_shebang(path: &Path) -> bool {
    let head = read_head(path, 64);
    head.starts_with("#!") && head.lines().next().is_some_and(|l| l.contains("node"))
}

fn read_head(path: &Path, max: usize) -> String {
    let mut buf = vec![0u8; max];
    let n = fs::File::open(path)
        .and_then(|mut f| f.read(&mut buf))
        .unwrap_or(0);
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// Files `bun fmt` formats; everything else keeps using the language server.
pub fn fmt_supported(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some(
            "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "json" | "jsonc"
                | "css" | "html" | "yaml" | "yml" | "md"
        )
    )
}

/// `bun fmt` on `text` as if it were `path`. `Ok(None)` = Bun is not installed.
/// The project's own config / `.editorconfig` wins; `tab_size` only applies when
/// it has none (pass the configured size, not one detected from the file being
/// fixed). `--no-script` stops a project `fmt` script hijacking it.
///
/// ponytail: blocking with no timeout, like the LSP format request.
pub fn bun_fmt(path: &Path, text: &str, tab_size: usize) -> Result<Option<String>, String> {
    let Some(bun) = bun_path() else {
        return Ok(None);
    };
    let mut cmd = crate::process::command(bun);
    cmd.args(["fmt", "--no-script", "--config-precedence=prefer-file"])
        .arg("--stdin-filepath")
        .arg(path)
        .arg(format!("--tab-width={tab_size}"));
    let mut child = cmd
        .current_dir(path.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run `bun fmt`: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("bun fmt stdin")?;
    let input = text.to_owned();
    // Own thread so a big file cannot deadlock against the stdout pipe.
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    if out.status.success() {
        return Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()));
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err.trim().chars().take(300).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shebang_and_wrapper_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("cli.js");
        fs::write(&script, "#!/usr/bin/env node\nconsole.log(1)\n").unwrap();
        let native = tmp.path().join("native");
        fs::write(&native, [0x7f, b'E', b'L', b'F']).unwrap();
        let wrapper = tmp.path().join("wrap");
        fs::write(&wrapper, format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", script.display())).unwrap();

        assert!(has_node_shebang(&script));
        assert!(!has_node_shebang(&native));
        assert_eq!(script_behind(&wrapper), script);
        assert_eq!(script_behind(&native), native);
    }

    #[test]
    fn fmt_support_by_extension() {
        assert!(fmt_supported(Path::new("a.tsx")));
        assert!(fmt_supported(Path::new("a.json")));
        assert!(!fmt_supported(Path::new("a.rs")));
        assert!(!fmt_supported(Path::new("Makefile")));
    }

    #[test]
    fn bun_fmt_reindents_when_bun_is_installed() {
        if bun_path().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let src = "function f(){\n      return 1\n}\n";
        let ts = tmp.path().join("a.ts");
        assert_eq!(
            bun_fmt(&ts, src, 2).unwrap().unwrap(),
            "function f() {\n  return 1;\n}\n"
        );
        // No project config: the passed width is used.
        assert_eq!(
            bun_fmt(&ts, src, 4).unwrap().unwrap(),
            "function f() {\n    return 1;\n}\n"
        );
        // A project `.prettierrc` beats the passed width.
        fs::write(tmp.path().join(".prettierrc"), r#"{"tabWidth": 3}"#).unwrap();
        assert_eq!(
            bun_fmt(&ts, src, 4).unwrap().unwrap(),
            "function f() {\n   return 1;\n}\n"
        );
    }
}
