//! How Isengard launches `jdtls` (Eclipse JDT Language Server).
//!
//! Two JDKs are involved: the **server JDK** runs jdtls itself (must be
//! [`JDTLS_MIN_JAVA`]+, picked automatically), the **project JDK** is the one
//! the user chose; it becomes jdtls' default runtime so diagnostics and
//! completions match what Maven/Gradle will compile with.
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::registry::ResolvedServerSpec;
use crate::jdk::{Jdk, java_exe};

/// jdtls itself needs this Java feature release (projects may target older).
pub const JDTLS_MIN_JAVA: u32 = 21;
/// Default `-Xmx` for jdtls; override with `config.json` `java.jdtls_xmx`.
pub const DEFAULT_XMX: &str = "1g";

/// Everything the host needs to start jdtls for the current workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaLaunch {
    /// JDK home that runs jdtls.
    pub server_home: PathBuf,
    /// Every installed JDK, offered to jdtls as a runtime.
    pub runtimes: Vec<(u32, PathBuf)>,
    /// The user's project JDK (jdtls' default runtime).
    pub project_home: Option<PathBuf>,
    pub xmx: String,
}

impl JavaLaunch {
    /// `None` when no installed JDK can run jdtls.
    pub fn from_jdks(jdks: &[Jdk], project: Option<&Jdk>, xmx: Option<&str>) -> Option<Self> {
        let server = jdks
            .iter()
            .filter(|j| j.major >= JDTLS_MIN_JAVA)
            .max_by_key(|j| j.major)?;
        Some(Self {
            server_home: server.home.clone(),
            runtimes: jdks.iter().map(|j| (j.major, j.home.clone())).collect(),
            project_home: project.map(|j| j.home.clone()),
            xmx: xmx
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(DEFAULT_XMX)
                .to_string(),
        })
    }

    /// Env / args for the resolved `jdtls`. Isengard's own launcher reads
    /// `ISENGARD_*`; a `jdtls` found on `PATH` (Python launcher) gets
    /// `JAVA_HOME` + `-data` instead.
    pub fn apply(&self, spec: &mut ResolvedServerSpec, folder: &Path, app_local: bool) {
        let data = data_dir(folder);
        let _ = std::fs::create_dir_all(&data);
        let java = java_exe(&self.server_home);
        if app_local {
            spec.env.extend([
                ("ISENGARD_JAVA".into(), java.to_string_lossy().into_owned()),
                ("ISENGARD_JDTLS_XMX".into(), self.xmx.clone()),
                (
                    "ISENGARD_JDTLS_DATA".into(),
                    data.to_string_lossy().into_owned(),
                ),
            ]);
        } else {
            spec.env.push((
                "JAVA_HOME".into(),
                self.server_home.to_string_lossy().into_owned(),
            ));
            spec.args
                .extend(["-data".into(), data.to_string_lossy().into_owned()]);
        }
    }

    /// LSP `initializationOptions`: the JDKs jdtls may compile against.
    pub fn init_options(&self) -> Value {
        let runtimes: Vec<Value> = self
            .runtimes
            .iter()
            .map(|(major, home)| {
                let mut entry = json!({ "name": runtime_name(*major), "path": home });
                if self.project_home.as_deref() == Some(home.as_path()) {
                    entry["default"] = json!(true);
                }
                entry
            })
            .collect();
        json!({ "settings": { "java": { "configuration": { "runtimes": runtimes } } } })
    }
}

/// jdtls execution-environment name: `JavaSE-1.8`, `JavaSE-17`, …
pub fn runtime_name(major: u32) -> String {
    if major <= 8 {
        "JavaSE-1.8".to_string()
    } else {
        format!("JavaSE-{major}")
    }
}

/// Per-workspace jdtls index (`-data`), under the cache dir so it can be wiped.
pub fn data_dir(folder: &Path) -> PathBuf {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    folder.hash(&mut hasher);
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("isengard")
        .join("jdtls-data")
        .join(format!("{:016x}", hasher.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jdk(major: u32, home: &str) -> Jdk {
        Jdk {
            home: home.into(),
            version: format!("{major}.0.1"),
            major,
            vendor: String::new(),
        }
    }

    fn spec() -> ResolvedServerSpec {
        ResolvedServerSpec {
            command: "jdtls".into(),
            args: vec![],
            language_id: "java".into(),
            env: vec![],
        }
    }

    #[test]
    fn server_jdk_is_newest_21_plus_even_for_a_java_8_project() {
        let jdks = [jdk(21, "/j21"), jdk(17, "/j17"), jdk(8, "/j8")];
        let launch = JavaLaunch::from_jdks(&jdks, Some(&jdks[2]), None).unwrap();
        assert_eq!(launch.server_home, Path::new("/j21"));
        assert_eq!(launch.project_home.as_deref(), Some(Path::new("/j8")));
        assert_eq!(launch.xmx, "1g");
    }

    #[test]
    fn no_jdk_21_means_no_launch() {
        let jdks = [jdk(17, "/j17"), jdk(8, "/j8")];
        assert!(JavaLaunch::from_jdks(&jdks, None, None).is_none());
    }

    #[test]
    fn runtimes_mark_the_project_jdk_default() {
        let jdks = [jdk(21, "/j21"), jdk(8, "/j8")];
        let launch = JavaLaunch::from_jdks(&jdks, Some(&jdks[1]), Some("512m")).unwrap();
        let opts = launch.init_options();
        let runtimes = opts["settings"]["java"]["configuration"]["runtimes"]
            .as_array()
            .unwrap();
        assert_eq!(runtimes[1]["name"], "JavaSE-1.8");
        assert_eq!(runtimes[1]["default"], true);
        assert!(runtimes[0].get("default").is_none());
        assert_eq!(launch.xmx, "512m");
    }

    #[test]
    fn apply_sets_launcher_env_or_path_args() {
        let launch = JavaLaunch::from_jdks(&[jdk(21, "/j21")], None, None).unwrap();
        let mut local = spec();
        launch.apply(&mut local, Path::new("/proj"), true);
        let keys: Vec<_> = local.env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            ["ISENGARD_JAVA", "ISENGARD_JDTLS_XMX", "ISENGARD_JDTLS_DATA"]
        );
        assert!(local.args.is_empty());

        let mut path = spec();
        launch.apply(&mut path, Path::new("/proj"), false);
        assert_eq!(path.env[0].0, "JAVA_HOME");
        assert_eq!(path.args[0], "-data");
    }

    #[test]
    fn data_dir_differs_per_folder() {
        assert_ne!(data_dir(Path::new("/a")), data_dir(Path::new("/b")));
    }
}
