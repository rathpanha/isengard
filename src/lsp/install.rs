//! One-click install / uninstall recipes for common language-server binaries.
//!
//! Isengard does not bundle servers; when one is missing we can offer to run a
//! well-known package-manager command for the user.

use std::process::Command;

/// How to install / remove a language server via the system shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallRecipe {
    /// Short name shown in dialogs (usually the binary).
    pub name: &'static str,
    /// One-line explanation.
    pub summary: &'static str,
    /// Shell command run with `sh -lc` (Unix) or `cmd /C` (Windows).
    pub shell: &'static str,
    /// Inverse of [`Self::shell`], when we know a safe uninstall command.
    pub remove_shell: Option<&'static str>,
}

/// Known install command for this server binary (basename of `command`).
pub fn recipe_for_command(command: &str) -> Option<InstallRecipe> {
    let name = std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);

    Some(match name {
        "rust-analyzer" => InstallRecipe {
            name: "rust-analyzer",
            summary: "Rust IntelliSense (completion, hover, go-to-definition).",
            shell: "rustup component add rust-analyzer",
            remove_shell: Some("rustup component remove rust-analyzer"),
        },
        "gopls" => InstallRecipe {
            name: "gopls",
            summary: "Go language server.",
            shell: "go install golang.org/x/tools/gopls@latest",
            // go install has no uninstall; forget from Isengard only.
            remove_shell: None,
        },
        "typescript-language-server" => InstallRecipe {
            name: "typescript-language-server",
            summary: "JavaScript / TypeScript language server.",
            shell: "npm install -g typescript typescript-language-server",
            remove_shell: Some("npm uninstall -g typescript-language-server"),
        },
        "pylsp" => InstallRecipe {
            name: "pylsp",
            summary: "Python language server (python-lsp-server).",
            shell: "pip install 'python-lsp-server[all]'",
            remove_shell: Some("pip uninstall -y python-lsp-server"),
        },
        "pyright-langserver" | "pyright" => InstallRecipe {
            name: "pyright",
            summary: "Python language server (Pyright).",
            shell: "npm install -g pyright",
            remove_shell: Some("npm uninstall -g pyright"),
        },
        "vscode-json-language-server"
        | "vscode-css-language-server"
        | "vscode-html-language-server" => InstallRecipe {
            name: "vscode-langservers-extracted",
            summary: "JSON / CSS / HTML language servers.",
            shell: "npm install -g vscode-langservers-extracted",
            remove_shell: Some("npm uninstall -g vscode-langservers-extracted"),
        },
        "yaml-language-server" => InstallRecipe {
            name: "yaml-language-server",
            summary: "YAML language server.",
            shell: "npm install -g yaml-language-server",
            remove_shell: Some("npm uninstall -g yaml-language-server"),
        },
        "bash-language-server" => InstallRecipe {
            name: "bash-language-server",
            summary: "Bash language server.",
            shell: "npm install -g bash-language-server",
            remove_shell: Some("npm uninstall -g bash-language-server"),
        },
        "dockerfile-language-server-nodejs" | "docker-langserver" => InstallRecipe {
            name: "dockerfile-language-server",
            summary: "Dockerfile language server.",
            shell: "npm install -g dockerfile-language-server-nodejs",
            remove_shell: Some("npm uninstall -g dockerfile-language-server-nodejs"),
        },
        "svelteserver" => InstallRecipe {
            name: "svelte-language-server",
            summary: "Svelte language server.",
            shell: "npm install -g svelte-language-server",
            remove_shell: Some("npm uninstall -g svelte-language-server"),
        },
        "astro-ls" => InstallRecipe {
            name: "astro-ls",
            summary: "Astro language server.",
            shell: "npm install -g @astrojs/language-server",
            remove_shell: Some("npm uninstall -g @astrojs/language-server"),
        },
        "taplo" => InstallRecipe {
            name: "taplo",
            summary: "TOML language server.",
            shell: "cargo install taplo-cli --locked --features lsp",
            remove_shell: Some("cargo uninstall taplo-cli"),
        },
        "marksman" => InstallRecipe {
            name: "marksman",
            summary: "Markdown language server.",
            shell: "brew install marksman",
            remove_shell: Some("brew uninstall marksman"),
        },
        "zls" => InstallRecipe {
            name: "zls",
            summary: "Zig language server.",
            shell: "brew install zls",
            remove_shell: Some("brew uninstall zls"),
        },
        "lua-language-server" => InstallRecipe {
            name: "lua-language-server",
            summary: "Lua language server.",
            shell: "brew install lua-language-server",
            remove_shell: Some("brew uninstall lua-language-server"),
        },
        _ => return None,
    })
}

/// Whether the language-server binary looks usable on this machine.
///
/// `rust-analyzer` is special: rustup ships a shim that exists on `PATH` before
/// `rustup component add rust-analyzer`.
pub fn is_server_ready(command: &str) -> bool {
    if !command_on_path(command) {
        return false;
    }
    let name = std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);
    if name != "rust-analyzer" {
        return true;
    }
    Command::new(command)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn command_on_path(command: &str) -> bool {
    let path = std::path::Path::new(command);
    if path.is_absolute() || command.contains('/') || command.contains('\\') {
        return path.is_file();
    }
    let Some(path_os) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_os) {
        if dir.join(command).is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            if dir.join(format!("{command}.exe")).is_file() {
                return true;
            }
        }
    }
    false
}

/// Run a shell recipe (`install` or `remove`).
pub fn run_shell(shell: &str) -> Result<(), String> {
    let output = {
        #[cfg(windows)]
        {
            Command::new("cmd").args(["/C", shell]).output()
        }
        #[cfg(not(windows))]
        {
            Command::new("sh").args(["-lc", shell]).output()
        }
    }
    .map_err(|err| format!("Could not start command: {err}"))?;

    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = stderr.trim();
    let detail = if detail.is_empty() {
        stdout.trim()
    } else {
        detail
    };
    if detail.is_empty() {
        Err(format!("`{shell}` exited with {}", output.status))
    } else {
        Err(detail.chars().take(400).collect())
    }
}

/// Run [`InstallRecipe::shell`].
pub fn run_install(recipe: &InstallRecipe) -> Result<(), String> {
    run_shell(recipe.shell)
}

/// Run [`InstallRecipe::remove_shell`] when present.
pub fn run_uninstall(recipe: &InstallRecipe) -> Result<(), String> {
    let Some(shell) = recipe.remove_shell else {
        return Err("No uninstall command for this server".into());
    };
    run_shell(shell)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_analyzer_has_rustup_recipe() {
        let r = recipe_for_command("rust-analyzer").unwrap();
        assert!(r.shell.contains("rustup"));
        assert!(r.remove_shell.unwrap().contains("remove"));
    }

    #[test]
    fn unknown_command_has_no_recipe() {
        assert!(recipe_for_command("totally-unknown-lsp-xyz").is_none());
    }

    #[test]
    fn basename_from_path() {
        let r = recipe_for_command("/usr/local/bin/gopls").unwrap();
        assert_eq!(r.name, "gopls");
        assert!(r.remove_shell.is_none());
    }
}
