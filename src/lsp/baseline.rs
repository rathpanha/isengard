//! Ground truth for Java support: run our exact jdtls launch against a real
//! Maven project and report what happens. Ignored by default — it needs jdtls
//! installed (status-bar LSP list → Install), a JDK 21+, and a few minutes.
//!
//! ```text
//! cargo test -- --ignored --nocapture jdtls_baseline
//! ISENGARD_JAVA_PROJECT=/path/to/maven/project cargo test -- --ignored --nocapture jdtls_baseline
//! ```
//!
//! The project is copied to `/tmp` first (without `target`, `.git`,
//! `node_modules`), so the original is never touched.
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::client::{Incoming, LspClient};
use super::host::initialize_params;
use super::java::{self, JavaLaunch};
use super::registry;
use super::uri::path_to_uri;
use crate::config::LspSettings;
use crate::editor::language::Language;

const DEFAULT_PROJECT: &str = "hell/extreme-jobs/java-learning/e-commerce-system";
const READY_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(120);

/// What the server told us, with timestamps (seconds since `initialize`).
#[derive(Default)]
struct Report {
    start: Option<Instant>,
    timeline: Vec<(f64, String)>,
    ready: bool,
    /// uri → (severity, line, message)
    diagnostics: HashMap<String, Vec<(u64, u64, String)>>,
    progress_titles: BTreeSet<String>,
    methods: BTreeSet<String>,
}

impl Report {
    fn log(&mut self, text: impl Into<String>) {
        let t = self.start.map_or(0.0, |s| s.elapsed().as_secs_f64());
        let text = text.into();
        eprintln!("[{t:7.1}s] {text}");
        self.timeline.push((t, text));
    }
}

fn copy_project(src: &Path, dst: &Path) {
    let _ = std::fs::remove_dir_all(dst);
    std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
    let status = std::process::Command::new("rsync")
        .args(["-a", "--exclude", "target", "--exclude", ".git"])
        .args(["--exclude", "node_modules"])
        .arg(format!("{}/", src.display()))
        .arg(dst)
        .status()
        .expect("rsync");
    assert!(status.success(), "rsync failed");
}

fn java_files(root: &Path) -> Vec<PathBuf> {
    ignore::WalkBuilder::new(root)
        .hidden(true)
        .require_git(false)
        .filter_entry(|e| e.file_name() != "target")
        .build()
        .flatten()
        .map(|e| e.into_path())
        .filter(|p| p.extension().is_some_and(|e| e == "java"))
        .collect()
}

/// Eclipse metadata jdtls may write into the project (what would pollute a repo).
fn eclipse_files(root: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let walk = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(false)
        .require_git(false)
        .filter_entry(|e| e.file_name() != "target")
        .build();
    for entry in walk.flatten() {
        let name = entry.file_name().to_string_lossy();
        if matches!(name.as_ref(), ".project" | ".classpath" | ".factorypath" | ".settings") {
            found.insert(entry.into_path());
        }
    }
    found
}

fn rss_kb(pid: u32, field: &str) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix(field)?.trim().trim_end_matches("kB").trim().parse().ok())
}

/// Server → client traffic: log it, answer what jdtls waits for.
fn handle(client: &LspClient, report: &Mutex<Report>, folders: &Value, msg: Incoming) {
    match msg {
        Incoming::Notification { method, params } => {
            let mut r = report.lock().unwrap();
            r.methods.insert(method.clone());
            match method.as_str() {
                "language/status" => {
                    let kind = params["type"].as_str().unwrap_or("?").to_string();
                    let message = params["message"].as_str().unwrap_or("").to_string();
                    r.log(format!("language/status {kind}: {message}"));
                    if kind == "ServiceReady" {
                        r.ready = true;
                    }
                }
                "$/progress" => {
                    let v = &params["value"];
                    let title = v["title"].as_str().unwrap_or("").to_string();
                    if !title.is_empty() {
                        r.progress_titles.insert(title.clone());
                    }
                    if v["kind"] != "report" || v["percentage"].as_u64().is_some_and(|p| p % 25 == 0)
                    {
                        let kind = v["kind"].as_str().unwrap_or("?");
                        let msg = v["message"].as_str().unwrap_or("");
                        r.log(format!("$/progress {kind} {title} {msg} {}", v["percentage"]));
                    }
                }
                "language/progressReport" => {
                    let task = params["task"].as_str().unwrap_or("");
                    let sub = params["subTask"].as_str().unwrap_or("");
                    r.log(format!("progressReport {task} · {sub}"));
                }
                "textDocument/publishDiagnostics" => {
                    let uri = params["uri"].as_str().unwrap_or("").to_string();
                    let list = params["diagnostics"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|d| {
                                    (
                                        d["severity"].as_u64().unwrap_or(0),
                                        d["range"]["start"]["line"].as_u64().unwrap_or(0),
                                        d["message"].as_str().unwrap_or("").to_string(),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    r.diagnostics.insert(uri, list);
                }
                "window/logMessage" | "window/showMessage" => {
                    let text = params["message"].as_str().unwrap_or("");
                    if params["type"].as_u64().is_some_and(|t| t <= 2) {
                        r.log(format!("{method}: {text}"));
                    }
                }
                _ => {}
            }
        }
        Incoming::ServerRequest { id, method, params } => {
            report.lock().unwrap().methods.insert(format!("(request) {method}"));
            let result = match method.as_str() {
                "workspace/configuration" => {
                    let n = params["items"].as_array().map_or(1, Vec::len);
                    json!(vec![json!({}); n])
                }
                "workspace/workspaceFolders" => folders.clone(),
                "workspace/applyEdit" => json!({ "applied": true }),
                "client/registerCapability"
                | "client/unregisterCapability"
                | "window/workDoneProgress/create" => Value::Null,
                _ => {
                    let _ = client.respond_error(id, &format!("Method Not Found: {method}"));
                    return;
                }
            };
            let _ = client.respond(id, result);
        }
    }
}

#[test]
#[ignore]
fn jdtls_baseline_on_a_real_maven_project() {
    let project = std::env::var_os("ISENGARD_JAVA_PROJECT")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(DEFAULT_PROJECT));
    if !project.join("pom.xml").is_file() {
        eprintln!("skipping: no Maven project at {}", project.display());
        return;
    }
    let root = std::env::temp_dir()
        .join("isengard-jdtls-baseline")
        .join(project.file_name().unwrap());
    copy_project(&project, &root);
    let before = eclipse_files(&root);

    let jdks = crate::jdk::scan(&[]);
    let xmx = std::env::var("ISENGARD_JDTLS_XMX").ok();
    let launch = JavaLaunch::from_jdks(&jdks, jdks.first(), xmx.as_deref())
        .expect("a JDK 21+ must be installed");
    let mut spec = registry::resolve(Language::Java, &LspSettings::default()).unwrap();
    launch.apply(&mut spec, &root, super::install::is_app_local("jdtls"));
    let mut init = initialize_params(std::slice::from_ref(&root), &spec.command).unwrap();
    init.initialization_options = Some(launch.init_options());

    eprintln!("project : {}", root.display());
    eprintln!("server  : JDK {} ({})", jdks[0].major, launch.server_home.display());
    eprintln!("heap    : -Xmx{}", launch.xmx);
    let report = Arc::new(Mutex::new(Report {
        start: Some(Instant::now()),
        ..Default::default()
    }));
    let (client, incoming) = LspClient::start(&spec, init).expect("jdtls must start");
    report.lock().unwrap().log("initialize answered");
    let client = Arc::new(client);
    let pid = client.pid().unwrap();

    let folders = json!([{ "uri": path_to_uri(&root).unwrap(), "name": "root" }]);
    {
        let (client, report) = (client.clone(), report.clone());
        std::thread::spawn(move || {
            while let Ok(msg) = incoming.recv() {
                handle(&client, &report, &folders, msg);
            }
        });
    }

    // 1. import
    let deadline = Instant::now() + READY_TIMEOUT;
    while !report.lock().unwrap().ready && Instant::now() < deadline && client.is_alive() {
        std::thread::sleep(Duration::from_millis(500));
    }
    let ready = report.lock().unwrap().ready;
    let imported_in = report.lock().unwrap().start.unwrap().elapsed();
    let rss_ready = rss_kb(pid, "VmRSS:").unwrap_or(0);
    eprintln!(
        "\n=== import: ready={ready} after {:.0}s, jdtls RSS {} MB ===",
        imported_in.as_secs_f64(),
        rss_ready / 1024
    );

    // 2. Lombok / MapStruct samples
    let files = java_files(&root);
    let pick = |markers: &[&str], n: usize| -> Vec<(PathBuf, String)> {
        files
            .iter()
            .filter_map(|p| {
                let text = std::fs::read_to_string(p).ok()?;
                markers.iter().any(|m| text.contains(m)).then_some((p.clone(), text))
            })
            .take(n)
            .collect()
    };
    let groups = [
        ("lombok", pick(&["@Slf4j", "@RequiredArgsConstructor", "@Builder", "@Data"], 4)),
        ("mapstruct", pick(&["@Mapper("], 3)),
        ("mapper user", pick(&["Mapper mapper", "Mapper;"], 3)),
    ];
    let mut opened: Vec<(&str, PathBuf)> = Vec::new();
    for (label, samples) in &groups {
        for (path, text) in samples {
            let uri = path_to_uri(path).unwrap();
            let _ = client.notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri, "languageId": "java", "version": 1, "text": text } }),
            );
            opened.push((label, path.clone()));
        }
    }
    let wait_until = Instant::now() + DIAGNOSTICS_TIMEOUT;
    while Instant::now() < wait_until {
        let r = report.lock().unwrap();
        let all = opened.iter().all(|(_, p)| {
            r.diagnostics.contains_key(&path_to_uri(p).unwrap().to_string())
        });
        drop(r);
        if all {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    eprintln!("\n=== diagnostics on {} sampled files ===", opened.len());
    let mut error_files = 0;
    {
        let r = report.lock().unwrap();
        for (label, path) in &opened {
            let key = path_to_uri(path).unwrap().to_string();
            let rel = path.strip_prefix(&root).unwrap_or(path).display().to_string();
            match r.diagnostics.get(&key) {
                None => eprintln!("[{label}] {rel}: (no diagnostics published)"),
                Some(list) => {
                    let errors: Vec<_> = list.iter().filter(|d| d.0 == 1).collect();
                    if !errors.is_empty() {
                        error_files += 1;
                    }
                    eprintln!("[{label}] {rel}: {} error(s), {} other", errors.len(), list.len() - errors.len());
                    for (_, line, msg) in errors.iter().take(3) {
                        eprintln!("      line {}: {msg}", line + 1);
                    }
                }
            }
        }
    }

    // 3. footprint
    let hwm = rss_kb(pid, "VmHWM:").unwrap_or(0);
    let after = eclipse_files(&root);
    let created: Vec<_> = after.difference(&before).collect();
    eprintln!("\n=== footprint ===");
    eprintln!("jdtls peak RSS {} MB (-Xmx{})", hwm / 1024, launch.xmx);
    eprintln!("Eclipse metadata written into the project: {} path(s)", created.len());
    for p in created.iter().take(5) {
        eprintln!("      {}", p.strip_prefix(&root).unwrap_or(p).display());
    }
    {
        let r = report.lock().unwrap();
        eprintln!("progress titles: {:?}", r.progress_titles);
        eprintln!("server → client methods: {:?}", r.methods);
    }
    eprintln!("files with errors: {error_files}/{}", opened.len());

    client.shutdown();
    let _ = std::fs::remove_dir_all(java::data_dir(&root));
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
    assert!(ready, "jdtls never reported ServiceReady");
}
