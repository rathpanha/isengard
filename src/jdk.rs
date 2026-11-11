//! JDK discovery for Java projects.
//!
//! Isengard never downloads or manages a JDK: it finds the ones the user
//! already has installed (reading each home's `release` file — no process is
//! spawned) and lets them pick one per workspace, like IntelliJ's Project SDK.
use std::path::{Path, PathBuf};

/// One installed JDK (has `bin/javac`, so it can compile and run Maven/Gradle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jdk {
    /// Directory holding `bin/` and `release` (what `JAVA_HOME` points at).
    pub home: PathBuf,
    /// `JAVA_VERSION` from `release`, e.g. `21.0.2` or `1.8.0_392`.
    pub version: String,
    /// Feature release: `21`, or `8` for `1.8.*`.
    pub major: u32,
    /// `IMPLEMENTOR` from `release` (may be empty).
    pub vendor: String,
}

impl Jdk {
    /// Short status-bar text, e.g. `JDK 21`.
    pub fn short_label(&self) -> String {
        format!("JDK {}", self.major)
    }

    /// Picker row text, e.g. `Eclipse Adoptium 21.0.2`.
    pub fn label(&self) -> String {
        if self.vendor.is_empty() {
            format!("JDK {}", self.version)
        } else {
            format!("{} {}", self.vendor, self.version)
        }
    }
}

/// Feature release from a `JAVA_VERSION` string (`1.8.0_392` → 8, `21.0.2` → 21).
pub fn major_of(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// `KEY="value"` lookup in a JDK `release` file.
fn release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// `<home>/bin/java[.exe]`.
pub fn java_exe(home: &Path) -> PathBuf {
    home.join("bin").join(exe("java"))
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// `Some` when `home` (or, on macOS bundles, `home/Contents/Home`) is a JDK.
pub fn inspect(home: &Path) -> Option<Jdk> {
    for dir in [home.to_path_buf(), home.join("Contents").join("Home")] {
        if !dir.join("bin").join(exe("javac")).is_file()
            || !dir.join("bin").join(exe("java")).is_file()
        {
            continue;
        }
        let release = std::fs::read_to_string(dir.join("release")).ok()?;
        let version = release_value(&release, "JAVA_VERSION")?;
        let major = major_of(&version)?;
        return Some(Jdk {
            home: dir,
            major,
            version,
            vendor: release_value(&release, "IMPLEMENTOR").unwrap_or_default(),
        });
    }
    None
}

/// Folders whose *children* are JDK homes.
fn container_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let home = dirs::home_dir();
    if let Some(h) = &home {
        dirs.push(h.join(".jdks"));
        dirs.push(h.join(".sdkman/candidates/java"));
        dirs.push(h.join(".asdf/installs/java"));
        dirs.push(h.join("Library/Java/JavaVirtualMachines"));
    }
    #[cfg(target_os = "linux")]
    dirs.extend(["/usr/lib/jvm", "/usr/java", "/opt/jdk", "/opt/java"].map(PathBuf::from));
    #[cfg(target_os = "macos")]
    dirs.extend(["/Library/Java/JavaVirtualMachines", "/opt/homebrew/opt"].map(PathBuf::from));
    #[cfg(windows)]
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = std::env::var_os(var).map(PathBuf::from) {
            for vendor in [
                "Java",
                "Eclipse Adoptium",
                "Microsoft",
                "Zulu",
                "Amazon Corretto",
            ] {
                dirs.push(pf.join(vendor));
            }
        }
    }
    dirs
}

/// Home implied by the first `java` on `PATH` (symlinks resolved).
fn path_java_home() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(exe("java")))
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent()?.parent().map(Path::to_path_buf))
}

/// Every JDK we can find: `JAVA_HOME`, `PATH`, well-known install folders and
/// the user's `extra` homes. Deduplicated by real path, newest first.
pub fn scan(extra: &[PathBuf]) -> Vec<Jdk> {
    let mut homes: Vec<PathBuf> = Vec::new();
    homes.extend(std::env::var_os("JAVA_HOME").map(PathBuf::from));
    homes.extend(path_java_home());
    for container in container_dirs() {
        if let Ok(entries) = std::fs::read_dir(container) {
            homes.extend(entries.flatten().map(|e| e.path()));
        }
    }
    homes.extend(extra.iter().cloned());
    collect(homes)
}

fn collect(homes: Vec<PathBuf>) -> Vec<Jdk> {
    let mut found: Vec<Jdk> = Vec::new();
    for home in homes {
        let Some(mut jdk) = inspect(&home) else {
            continue;
        };
        jdk.home = jdk.home.canonicalize().unwrap_or(jdk.home);
        if !found.iter().any(|j| j.home == jdk.home) {
            found.push(jdk);
        }
    }
    found.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| a.home.cmp(&b.home)));
    found
}

/// The JDK in use: the saved choice if it is still installed, else the one
/// `JAVA_HOME` names, else the newest.
pub fn choose<'a>(jdks: &'a [Jdk], saved: Option<&Path>) -> Option<&'a Jdk> {
    let by_home = |home: &Path| {
        let real = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        jdks.iter().find(|j| j.home == real)
    };
    saved
        .and_then(by_home)
        .or_else(|| {
            std::env::var_os("JAVA_HOME")
                .map(PathBuf::from)
                .and_then(|h| by_home(&h))
        })
        .or_else(|| jdks.first())
}

/// A root folder with a Maven or Gradle build file.
pub fn is_java_project(folders: &[PathBuf]) -> bool {
    const BUILD_FILES: [&str; 5] = [
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ];
    folders
        .iter()
        .any(|f| BUILD_FILES.iter().any(|name| f.join(name).is_file()))
}

/// Where the user downloads a JDK when none is installed.
pub const DOWNLOAD_URL: &str = "https://adoptium.net/temurin/releases/";

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_jdk(root: &Path, name: &str, version: &str, vendor: &str) -> PathBuf {
        let home = root.join(name);
        std::fs::create_dir_all(home.join("bin")).unwrap();
        for bin in ["java", "javac"] {
            std::fs::write(home.join("bin").join(exe(bin)), "").unwrap();
        }
        std::fs::write(
            home.join("release"),
            format!("JAVA_VERSION=\"{version}\"\nIMPLEMENTOR=\"{vendor}\"\n"),
        )
        .unwrap();
        home
    }

    #[test]
    fn major_handles_legacy_and_modern_versions() {
        assert_eq!(major_of("1.8.0_392"), Some(8));
        assert_eq!(major_of("11.0.21"), Some(11));
        assert_eq!(major_of("21"), Some(21));
        assert_eq!(major_of("25-ea"), Some(25));
        assert_eq!(major_of("nope"), None);
    }

    #[test]
    fn inspect_reads_release_and_requires_javac() {
        let dir = tempfile::tempdir().unwrap();
        let home = fake_jdk(dir.path(), "temurin-21", "21.0.2", "Eclipse Adoptium");
        let jdk = inspect(&home).unwrap();
        assert_eq!(jdk.major, 21);
        assert_eq!(jdk.label(), "Eclipse Adoptium 21.0.2");
        assert_eq!(jdk.short_label(), "JDK 21");

        std::fs::remove_file(home.join("bin").join(exe("javac"))).unwrap();
        assert!(inspect(&home).is_none(), "a JRE is not enough");
    }

    #[test]
    fn inspect_finds_macos_bundle_home() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("jdk-17.jdk");
        fake_jdk(&bundle, "Contents/Home", "17.0.9", "Oracle");
        assert_eq!(inspect(&bundle).unwrap().major, 17);
    }

    #[test]
    fn collect_sorts_newest_first_and_dedupes() {
        let dir = tempfile::tempdir().unwrap();
        let a = fake_jdk(dir.path(), "a", "17.0.9", "X");
        let b = fake_jdk(dir.path(), "b", "21.0.2", "X");
        let found = collect(vec![a.clone(), b.clone(), a, dir.path().join("missing")]);
        assert_eq!(
            found.iter().map(|j| j.major).collect::<Vec<_>>(),
            vec![21, 17]
        );
    }

    #[test]
    fn choose_prefers_saved_then_newest() {
        let dir = tempfile::tempdir().unwrap();
        let old = fake_jdk(dir.path(), "old", "11.0.1", "X");
        let new = fake_jdk(dir.path(), "new", "21.0.1", "X");
        let jdks = collect(vec![old.clone(), new]);
        assert_eq!(choose(&jdks, Some(&old)).unwrap().major, 11);
        let gone = dir.path().join("gone");
        assert_eq!(choose(&jdks, Some(&gone)).unwrap().major, 21);
        assert!(choose(&[], None).is_none());
    }

    #[test]
    fn java_project_detects_build_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        assert!(!is_java_project(std::slice::from_ref(&root)));
        std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
        assert!(is_java_project(&[root]));
    }
}
