//! Maven / Gradle detection and the task list behind the status-bar Run menu.
//!
//! Tasks run in a terminal tab (live output, Ctrl+C to stop) through the
//! user's own login shell, so `mvn` / `gradle` resolve like they do in any
//! terminal. A project wrapper (`mvnw` / `gradlew`) always wins over a
//! system install.
use std::path::{Path, PathBuf};

use crate::terminal::pty::{TerminalCommand, default_shell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Maven,
    Gradle,
}

/// A workspace folder that holds a Maven or Gradle build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    pub tool: Tool,
    pub root: PathBuf,
    /// Run `./mvnw` / `./gradlew` instead of a system install.
    pub wrapper: bool,
    pub tasks: Vec<Task>,
}

/// One menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub label: String,
    pub args: Vec<String>,
}

fn task(label: &str, args: &[&str]) -> Task {
    Task {
        label: label.to_string(),
        args: args.iter().map(|a| (*a).to_string()).collect(),
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The build in `folder` (Maven wins when both exist).
pub fn detect(folder: &Path) -> Option<Build> {
    if folder.join("pom.xml").is_file() {
        let pom = read(&folder.join("pom.xml"));
        let mut tasks = vec![
            task("clean", &["clean"]),
            task("compile", &["compile"]),
            task("test", &["test"]),
            task("package", &["package"]),
            task("clean install", &["clean", "install"]),
        ];
        if pom.contains("spring-boot") {
            tasks.push(task("spring-boot:run", &["spring-boot:run"]));
        }
        let wrapper = folder.join(wrapper_name(Tool::Maven)).is_file();
        return Some(Build {
            tool: Tool::Maven,
            root: folder.to_path_buf(),
            wrapper,
            tasks,
        });
    }
    let gradle_file = [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .into_iter()
    .map(|n| folder.join(n))
    .find(|p| p.is_file())?;
    let script = format!(
        "{}{}",
        read(&folder.join("build.gradle")),
        read(&folder.join("build.gradle.kts"))
    );
    let mut tasks = vec![
        task("clean", &["clean"]),
        task("build", &["build"]),
        task("test", &["test"]),
    ];
    if script.contains("org.springframework.boot") {
        tasks.push(task("bootRun", &["bootRun"]));
    } else if script.contains("application") {
        tasks.push(task("run", &["run"]));
    }
    let wrapper = folder.join(wrapper_name(Tool::Gradle)).is_file();
    let _ = gradle_file;
    Some(Build {
        tool: Tool::Gradle,
        root: folder.to_path_buf(),
        wrapper,
        tasks,
    })
}

/// Every workspace folder that has a build.
pub fn detect_all(folders: &[PathBuf]) -> Vec<Build> {
    folders.iter().filter_map(|f| detect(f)).collect()
}

fn wrapper_name(tool: Tool) -> &'static str {
    match (tool, cfg!(windows)) {
        (Tool::Maven, false) => "mvnw",
        (Tool::Maven, true) => "mvnw.cmd",
        (Tool::Gradle, false) => "gradlew",
        (Tool::Gradle, true) => "gradlew.bat",
    }
}

impl Build {
    /// Tab title, e.g. `mvn clean install`.
    pub fn title(&self, task: &Task) -> String {
        let tool = match self.tool {
            Tool::Maven => "mvn",
            Tool::Gradle => "gradle",
        };
        format!("{tool} {}", task.label)
    }

    /// `mvn` / `gradle`: what must be on `PATH` when there is no wrapper.
    pub fn tool_name(&self) -> &'static str {
        match self.tool {
            Tool::Maven => "mvn",
            Tool::Gradle => "gradle",
        }
    }

    fn program(&self) -> String {
        match (self.wrapper, self.tool, cfg!(windows)) {
            (true, tool, false) => format!("./{}", wrapper_name(tool)),
            (true, tool, true) => format!(".\\{}", wrapper_name(tool)),
            (false, Tool::Maven, _) => "mvn".to_string(),
            (false, Tool::Gradle, _) => "gradle".to_string(),
        }
    }

    /// Terminal program that runs `task` with `JAVA_HOME` pointing at `jdk`.
    pub fn command(&self, task: &Task, jdk: Option<&Path>) -> TerminalCommand {
        shell_command(&format!("{} {}", self.program(), task.args.join(" ")), jdk)
    }
}

/// Run `line` in the user's login shell with `JAVA_HOME` / `PATH` pointing at
/// `jdk`. The export is part of the command line (not only the env) because
/// shell rc files — sdkman, jenv — often reset `JAVA_HOME` for login shells.
fn shell_command(line: &str, jdk: Option<&Path>) -> TerminalCommand {
    if cfg!(windows) {
        let set = jdk.map_or(String::new(), |j| {
            format!(
                "set \"JAVA_HOME={0}\" && set \"PATH={0}\\bin;%PATH%\" && ",
                j.display()
            )
        });
        return TerminalCommand {
            program: PathBuf::from(default_shell()),
            args: vec!["/C".into(), format!("{set}{line}")],
            env: Vec::new(),
        };
    }
    let export = jdk.map_or(String::new(), |j| {
        let home = sh_quote(&j.to_string_lossy());
        format!("export JAVA_HOME={home}; export PATH=\"$JAVA_HOME/bin:$PATH\"; ")
    });
    TerminalCommand {
        program: PathBuf::from(default_shell()),
        args: vec![
            "-l".into(),
            "-i".into(),
            "-c".into(),
            format!("{export}{line}"),
        ],
        env: Vec::new(),
    }
}

/// A runnable class found in a `.java` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainClass {
    pub file: PathBuf,
    /// `pkg.Name` (just `Name` in the default package).
    pub qualified: String,
}

/// `Some` when `text` declares `public static void main(` (also Java 21+
/// instance `void main()` is not covered — it needs preview flags).
pub fn find_main(file: &Path, text: &str) -> Option<MainClass> {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !flat.contains("static void main(") {
        return None;
    }
    let package = text.lines().find_map(|l| {
        let l = l.trim();
        let name = l.strip_prefix("package ")?.trim_end_matches(';').trim();
        (!name.is_empty()).then(|| name.to_string())
    });
    let class = file.file_stem()?.to_string_lossy().into_owned();
    let qualified = match package {
        Some(p) => format!("{p}.{class}"),
        None => class,
    };
    Some(MainClass {
        file: file.to_path_buf(),
        qualified,
    })
}

impl Build {
    /// Run a `main` class: Maven via `exec:java` (works without configuring
    /// the plugin), Gradle by compiling then launching on the class output.
    pub fn main_task(&self, main: &MainClass) -> Task {
        let class = &main.qualified;
        match self.tool {
            Tool::Maven => task(
                &format!("run {}", simple_name(class)),
                &["compile", "exec:java", &format!("-Dexec.mainClass={class}")],
            ),
            Tool::Gradle => task(
                &format!("run {}", simple_name(class)),
                &["classes", "&&", "java", "-cp", gradle_classpath(), class],
            ),
        }
    }
}

/// A class that can be started: it declares `main`. Shown in the Run menu so
/// nobody has to open the file first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub main: MainClass,
    /// Annotated `@SpringBootApplication`.
    pub spring: bool,
    /// Nearest folder (up to the workspace root) with a `pom.xml` / Gradle build.
    pub module: Option<PathBuf>,
}

impl Service {
    pub fn name(&self) -> &str {
        simple_name(&self.main.qualified)
    }

    /// Folder name of the module (or of the file's folder outside a build).
    pub fn module_label(&self) -> String {
        let dir = self.module.as_deref().or_else(|| self.main.file.parent());
        dir.and_then(Path::file_name)
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    /// What to run in `build` (the service's own module): Spring Boot apps use
    /// the Boot plugin, everything else Maven `exec:java` / Gradle classes.
    pub fn task(&self, build: &Build) -> Task {
        let has_boot = build
            .tasks
            .iter()
            .any(|t| t.label == "spring-boot:run" || t.label == "bootRun");
        match (self.spring && has_boot, build.tool) {
            (true, Tool::Maven) => task(
                self.name(),
                &[
                    "spring-boot:run",
                    &format!("-Dspring-boot.run.main-class={}", self.main.qualified),
                ],
            ),
            (true, Tool::Gradle) => task(self.name(), &["bootRun"]),
            (false, _) => {
                let mut t = build.main_task(&self.main);
                t.label = self.name().to_string();
                t
            }
        }
    }
}

/// Nearest folder at or above `file` (not above its workspace root) holding a build.
pub fn nearest_build_dir(file: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let root = roots
        .iter()
        .filter(|r| file.starts_with(r))
        .max_by_key(|r| r.components().count())?;
    file.ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(root))
        .find(|dir| detect(dir).is_some())
        .map(Path::to_path_buf)
}

/// Every non-test `.java` file with a `main` under `folders`: Spring Boot apps
/// first, then by name.
pub fn scan_services(folders: &[PathBuf]) -> Vec<Service> {
    use ignore::WalkBuilder;
    const SKIP: [&str; 6] = [".git", "node_modules", "target", "build", "out", "dist"];
    const MAX_BYTES: u64 = 512 * 1024;
    let mut found: Vec<Service> = Vec::new();
    for root in folders {
        let walk = WalkBuilder::new(root)
            .hidden(true)
            .require_git(false)
            .max_filesize(Some(MAX_BYTES))
            .filter_entry(|e| !SKIP.iter().any(|d| e.file_name() == *d))
            .build();
        for entry in walk.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "java")
                || !entry.file_type().is_some_and(|t| t.is_file())
            {
                continue;
            }
            let in_tests = path
                .components()
                .zip(path.components().skip(1))
                .any(|(a, b)| a.as_os_str() == "src" && b.as_os_str() == "test");
            if in_tests {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            let Some(main) = find_main(path, &text) else {
                continue;
            };
            found.push(Service {
                spring: text.contains("@SpringBootApplication"),
                module: nearest_build_dir(path, folders),
                main,
            });
        }
    }
    found.sort_by(|a, b| {
        b.spring
            .cmp(&a.spring)
            .then_with(|| a.name().to_lowercase().cmp(&b.name().to_lowercase()))
            .then_with(|| a.main.file.cmp(&b.main.file))
    });
    found.dedup_by(|a, b| a.main.file == b.main.file);
    found
}

fn gradle_classpath() -> &'static str {
    if cfg!(windows) {
        "build\\classes\\java\\main;build\\resources\\main"
    } else {
        "build/classes/java/main:build/resources/main"
    }
}

fn simple_name(qualified: &str) -> &str {
    qualified.rsplit('.').next().unwrap_or(qualified)
}

/// Terminal program for a lone `.java` file outside any build: the JDK's
/// single-file source launcher (`java File.java`, JDK 11+).
pub fn single_file_command(main: &MainClass, jdk: Option<&Path>) -> TerminalCommand {
    let file = main
        .file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    shell_command(&format!("java {}", quote_arg(&file)), jdk)
}

fn quote_arg(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_alphanumeric() || "-_./:=+@%,".contains(c))
    {
        arg.to_string()
    } else if cfg!(windows) {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        sh_quote(arg)
    }
}

/// Is `program` (`mvn`, `gradle`) resolvable in the user's login shell? A
/// shell that does not answer within 5s counts as "yes" — never block a run
/// on a slow rc file.
pub fn tool_on_path(program: &str) -> bool {
    let mut cmd = crate::process::command(default_shell());
    if cfg!(windows) {
        cmd.args(["/C", &format!("where {program}")]);
    } else {
        cmd.args(["-l", "-i", "-c", &format!("command -v {program}")]);
    }
    let Ok(mut child) = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return true;
    };
    for _ in 0..100 {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return true,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    true
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        dir
    }

    fn labels(build: &Build) -> Vec<&str> {
        build.tasks.iter().map(|t| t.label.as_str()).collect()
    }

    #[test]
    fn maven_tasks_and_spring_boot_run() {
        let plain = dir_with(&[("pom.xml", "<project/>")]);
        let b = detect(plain.path()).unwrap();
        assert_eq!(b.tool, Tool::Maven);
        assert_eq!(
            labels(&b),
            ["clean", "compile", "test", "package", "clean install"]
        );

        let boot = dir_with(&[(
            "pom.xml",
            "<artifactId>spring-boot-starter-web</artifactId>",
        )]);
        assert!(labels(&detect(boot.path()).unwrap()).contains(&"spring-boot:run"));
    }

    #[test]
    fn gradle_tasks_by_plugin() {
        let boot = dir_with(&[("build.gradle", "id 'org.springframework.boot' version '3'")]);
        assert!(labels(&detect(boot.path()).unwrap()).contains(&"bootRun"));
        let app = dir_with(&[("build.gradle.kts", "plugins { application }")]);
        assert!(labels(&detect(app.path()).unwrap()).contains(&"run"));
        let bare = dir_with(&[("settings.gradle", "")]);
        assert_eq!(
            labels(&detect(bare.path()).unwrap()),
            ["clean", "build", "test"]
        );
        assert!(detect(dir_with(&[]).path()).is_none());
    }

    #[test]
    fn wrapper_wins_and_jdk_is_exported_in_the_command() {
        let dir = dir_with(&[("pom.xml", "<project/>"), (wrapper_name(Tool::Maven), "")]);
        let b = detect(dir.path()).unwrap();
        assert!(b.wrapper);
        let cmd = b.command(&b.tasks[4], Some(Path::new("/jdk 17")));
        let line = cmd.args.last().unwrap();
        if !cfg!(windows) {
            assert!(line.contains("export JAVA_HOME='/jdk 17'"));
            assert!(line.ends_with("./mvnw clean install"));
            assert_eq!(&cmd.args[..3], ["-l", "-i", "-c"]);
        }
        assert_eq!(b.title(&b.tasks[4]), "mvn clean install");
    }

    #[test]
    fn single_quotes_in_jdk_path_are_escaped() {
        assert_eq!(sh_quote("/a'b"), "'/a'\\''b'");
    }

    #[test]
    fn find_main_reads_package_and_class() {
        let text = "package com.acme;\n\npublic class App {\n  public static  void\n main(String[] a) {}\n}\n";
        let m = find_main(Path::new("/p/src/App.java"), text).unwrap();
        assert_eq!(m.qualified, "com.acme.App");
        assert!(find_main(Path::new("Lib.java"), "class Lib {}").is_none());
        assert_eq!(
            find_main(
                Path::new("Main.java"),
                "class Main { static void main(String[] a){} }"
            )
            .unwrap()
            .qualified,
            "Main"
        );
    }

    #[test]
    fn main_task_per_tool_and_nearest_build() {
        let maven = dir_with(&[("pom.xml", "<project/>")]);
        let b = detect(maven.path()).unwrap();
        let main = MainClass {
            file: maven.path().join("A.java"),
            qualified: "p.A".into(),
        };
        let t = b.main_task(&main);
        assert_eq!(t.label, "run A");
        assert_eq!(t.args, ["compile", "exec:java", "-Dexec.mainClass=p.A"]);

        let gradle = dir_with(&[("build.gradle", "")]);
        let g = detect(gradle.path()).unwrap();
        assert!(g.main_task(&main).args.contains(&"&&".to_string()));
    }

    #[test]
    fn single_file_launcher_quotes_the_name() {
        let main = MainClass {
            file: "/p/My File.java".into(),
            qualified: "My File".into(),
        };
        let cmd = single_file_command(&main, Some(Path::new("/jdk")));
        let line = cmd.args.last().unwrap();
        if !cfg!(windows) {
            assert!(line.ends_with("java 'My File.java'"));
        }
    }

    #[test]
    fn services_are_found_per_module_with_spring_first() {
        let tmp = tempfile::tempdir().unwrap();
        let write = |rel: &str, text: &str| {
            let p = tmp.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write("pom.xml", "<modules/>");
        write(
            "gateway/pom.xml",
            "<artifactId>spring-boot-starter</artifactId>",
        );
        write(
            "gateway/src/main/java/co/GatewayApplication.java",
            "package co;\n@SpringBootApplication\npublic class GatewayApplication { public static void main(String[] a) {} }",
        );
        write(
            "tools/src/main/java/Tool.java",
            "class Tool { public static void main(String[] a) {} }",
        );
        write(
            "gateway/src/test/java/co/TestMain.java",
            "class TestMain { public static void main(String[] a) {} }",
        );
        write(
            "gateway/target/classes/Gen.java",
            "class Gen { public static void main(String[] a) {} }",
        );
        write("gateway/src/main/java/co/Lib.java", "class Lib {}");

        let services = scan_services(&[tmp.path().to_path_buf()]);
        let names: Vec<_> = services.iter().map(Service::name).collect();
        assert_eq!(names, ["GatewayApplication", "Tool"]);
        assert!(services[0].spring);
        assert_eq!(
            services[0].module.as_deref(),
            Some(tmp.path().join("gateway").as_path())
        );
        assert_eq!(services[0].module_label(), "gateway");
        // `tools` has no build of its own: falls back to the aggregator pom.
        assert_eq!(services[1].module.as_deref(), Some(tmp.path()));

        let build = detect(services[0].module.as_ref().unwrap()).unwrap();
        let t = services[0].task(&build);
        assert_eq!(
            t.args,
            [
                "spring-boot:run",
                "-Dspring-boot.run.main-class=co.GatewayApplication"
            ]
        );
        let root_build = detect(tmp.path()).unwrap();
        assert_eq!(services[1].task(&root_build).args[1], "exec:java");
    }
}
