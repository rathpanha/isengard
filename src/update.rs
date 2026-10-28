//! Update discovery, download, verification and install (no UI).
//!
//! Releases come from GitHub. Every download is checked against a
//! `SHA256SUMS` file whose minisign signature (`SHA256SUMS.minisig`) must
//! verify against [`PUBLIC_KEY`]; nothing unverified is ever installed.
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const LATEST_URL: &str = "https://api.github.com/repos/rathpanha/isengard/releases/latest";
/// Where users land when the app cannot update itself.
pub const RELEASES_URL: &str = "https://github.com/rathpanha/isengard/releases/latest";
const USER_AGENT: &str = concat!("isengard/", env!("CARGO_PKG_VERSION"));
const SUMS_NAME: &str = "SHA256SUMS";
const SIG_NAME: &str = "SHA256SUMS.minisig";

/// minisign public key (the base64 line of `minisign.pub`, starts `RW…`).
/// While empty the app only notifies and links to the release page — it
/// never installs. Matches the `MINISIGN_KEY` secret used by the release
/// workflow (key id 1242B7D78089071B).
pub const PUBLIC_KEY: &str = "RWQbB4mA17dCElWr4j6a897OzAzdEm5LyFvTfGCEd+2EKyoin6HtzQZH";

/// A published GitHub release.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    /// `tag` without the leading `v`.
    pub version: String,
    /// Markdown body of the release notes.
    pub notes: String,
    pub page_url: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

pub fn parse_release(json: &str) -> Result<Release> {
    let api: ApiRelease = serde_json::from_str(json).context("release JSON")?;
    Ok(Release {
        version: api.tag_name.trim_start_matches('v').to_string(),
        tag: api.tag_name,
        notes: api.body.unwrap_or_default(),
        page_url: api.html_url,
        assets: api
            .assets
            .into_iter()
            .map(|a| Asset {
                name: a.name,
                url: a.browser_download_url,
            })
            .collect(),
    })
}

fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// True when `latest` is a strictly newer release than `current`. Anything
/// unparseable counts as "not newer" so a odd tag never nags the user.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether releases are signed with a key this build knows about.
pub fn signing_configured() -> bool {
    !PUBLIC_KEY.is_empty()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Rpm,
    Deb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// The binary's directory is writable: swap the binary, then restart.
    InPlace { exe: PathBuf },
    /// Installed by the system package manager: download, then hand the user
    /// an install command to run themselves (it needs root).
    Package {
        kind: PackageKind,
        manager: &'static str,
    },
    /// macOS `Isengard.app` in a writable folder (`/Applications` for admin
    /// users): swap the whole bundle, then restart.
    MacApp { bundle: PathBuf },
    /// Cannot update itself (dev build, `/usr/local`, other OS): link only.
    Manual,
}

pub fn detect_install_kind() -> InstallKind {
    let Ok(exe) = std::env::current_exe() else {
        return InstallKind::Manual;
    };
    if cfg!(target_os = "macos") {
        let writable = app_bundle_of(&exe)
            .and_then(Path::parent)
            .is_some_and(dir_writable);
        return classify_macos(&exe, writable);
    }
    if !cfg!(target_os = "linux") {
        return InstallKind::Manual;
    }
    let writable = exe.parent().is_some_and(dir_writable);
    classify(&exe, writable, command_on_path)
}

/// `…/Isengard.app` when `exe` is `…/Isengard.app/Contents/MacOS/<bin>`.
fn app_bundle_of(exe: &Path) -> Option<&Path> {
    let macos_dir = exe.parent()?;
    let contents = macos_dir.parent()?;
    let bundle = contents.parent()?;
    (macos_dir.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then_some(bundle)
}

/// Pure decision behind [`detect_install_kind`] on macOS (unit-tested).
pub fn classify_macos(exe: &Path, parent_writable: bool) -> InstallKind {
    // `cargo run` builds and anything outside a bundle are never updated.
    match app_bundle_of(exe) {
        Some(bundle) if parent_writable => InstallKind::MacApp {
            bundle: bundle.to_path_buf(),
        },
        _ => InstallKind::Manual,
    }
}

/// Pure decision behind [`detect_install_kind`] (unit-tested).
pub fn classify(exe: &Path, dir_writable: bool, has: impl Fn(&str) -> bool) -> InstallKind {
    // `cargo run` / `target/release` builds are never updated in place.
    if exe.ancestors().any(|a| a.file_name().is_some_and(|n| n == "target")) {
        return InstallKind::Manual;
    }
    if exe.starts_with("/usr/bin") || exe.starts_with("/bin") {
        for (manager, kind) in [
            ("dnf", PackageKind::Rpm),
            ("zypper", PackageKind::Rpm),
            ("apt", PackageKind::Deb),
            ("apt-get", PackageKind::Deb),
        ] {
            if has(manager) {
                return InstallKind::Package { kind, manager };
            }
        }
        return InstallKind::Manual;
    }
    if dir_writable {
        return InstallKind::InPlace {
            exe: exe.to_path_buf(),
        };
    }
    InstallKind::Manual
}

fn command_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".isengard-write-test-{}", std::process::id()));
    let ok = fs::File::create(&probe).is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// File names end up in a shell command; accept only boring ones.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-' | '~'))
}

/// The release asset that installs on this machine via `kind`.
pub fn pick_asset<'a>(release: &'a Release, kind: &InstallKind) -> Option<&'a Asset> {
    let (rpm_arch, deb_arch) = match std::env::consts::ARCH {
        "x86_64" => ("x86_64", "amd64"),
        "aarch64" => ("aarch64", "arm64"),
        _ => return None,
    };
    release.assets.iter().find(|a| {
        is_safe_name(&a.name)
            && match kind {
                InstallKind::InPlace { .. } => {
                    a.name.ends_with(&format!("-linux-{rpm_arch}.tar.gz"))
                }
                InstallKind::Package {
                    kind: PackageKind::Rpm,
                    ..
                } => a.name.ends_with(".rpm") && a.name.contains(rpm_arch),
                InstallKind::Package {
                    kind: PackageKind::Deb,
                    ..
                } => a.name.ends_with(".deb") && a.name.contains(deb_arch),
                InstallKind::MacApp { .. } => a.name.ends_with("-macos-universal.tar.gz"),
                InstallKind::Manual => false,
            }
    })
}

/// Shell command that installs the downloaded package. The user runs it.
pub fn install_command(manager: &str, package: &Path) -> String {
    let path = package.to_string_lossy().replace('\'', r"'\''");
    format!("sudo {manager} install '{path}'")
}

/// Why [`install_package`] failed.
#[derive(Debug)]
pub enum InstallError {
    /// sudo rejected the password; ask again.
    WrongPassword,
    /// Anything else, with the tail of the package manager's output.
    Failed(String),
}

/// Program and arguments that install `package` without prompting.
pub fn install_argv(manager: &str, package: &Path) -> Vec<String> {
    let path = package.to_string_lossy().into_owned();
    let mut argv: Vec<String> = match manager {
        "apt" | "apt-get" => vec!["apt-get".into(), "install".into(), "-y".into()],
        "zypper" => vec![
            "zypper".into(),
            "--non-interactive".into(),
            // The minisign + checksum check already vouched for this file.
            "--no-gpg-checks".into(),
            "install".into(),
        ],
        other => vec![other.into(), "install".into(), "-y".into()],
    };
    argv.push(path);
    argv
}

/// Run the install through `sudo`, feeding `password` on stdin and reporting
/// each line of output to `on_line`. Blocks until the package manager exits.
pub fn install_package(
    manager: &str,
    package: &Path,
    password: &str,
    mut on_line: impl FnMut(&str),
) -> std::result::Result<(), InstallError> {
    use std::process::{Command, Stdio};
    let fail = |e: std::io::Error| InstallError::Failed(format!("could not run sudo: {e}"));
    let mut child = Command::new("sudo")
        // -k: ignore cached credentials so a wrong password is always caught.
        .args(["-S", "-k", "-p", ""])
        .args(install_argv(manager, package))
        .env("LC_ALL", "C")
        .env("DEBIAN_FRONTEND", "noninteractive")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(fail)?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{password}");
    }
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let mut readers = Vec::new();
    for stream in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            let mut stream = stream;
            let (mut buf, mut line) = ([0u8; 4096], Vec::new());
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for &b in &buf[..n] {
                    // Package managers redraw progress with bare `\r`.
                    if b == b'\n' || b == b'\r' {
                        if !line.is_empty() {
                            let _ = tx.send(String::from_utf8_lossy(&line).into_owned());
                            line.clear();
                        }
                    } else {
                        line.push(b);
                    }
                }
            }
            if !line.is_empty() {
                let _ = tx.send(String::from_utf8_lossy(&line).into_owned());
            }
        }));
    }
    drop(tx);
    let mut tail: Vec<String> = Vec::new();
    for line in rx {
        on_line(&line);
        tail.push(line);
        if tail.len() > 12 {
            tail.remove(0);
        }
    }
    for reader in readers {
        let _ = reader.join();
    }
    let status = child.wait().map_err(fail)?;
    if status.success() {
        return Ok(());
    }
    let text = tail.join("\n");
    let lower = text.to_lowercase();
    if lower.contains("incorrect password")
        || lower.contains("try again")
        || lower.contains("no password was provided")
    {
        return Err(InstallError::WrongPassword);
    }
    Err(InstallError::Failed(text))
}

/// `<cache dir>/isengard/updates`.
pub fn updates_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("isengard")
        .join("updates")
}

/// HTTP agent that gives up instead of hanging (a stalled connect or silent
/// server would otherwise leave "Check for Updates" with no answer).
/// `global` also caps the whole request; downloads pass `None` and rely on the
/// connect / first-byte limits so big files can take as long as they need.
fn agent(global: Option<std::time::Duration>) -> ureq::Agent {
    use std::time::Duration;
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_global(global)
        .build()
        .into()
}

const API_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// UI tests stand in for GitHub with this canned release.
#[cfg(test)]
pub static TEST_LATEST: std::sync::Mutex<Option<Release>> = std::sync::Mutex::new(None);

/// Query GitHub for the latest published release.
pub fn fetch_latest() -> Result<Release> {
    #[cfg(test)]
    if let Some(release) = TEST_LATEST.lock().unwrap().clone() {
        return Ok(release);
    }
    let mut response = agent(Some(API_TIMEOUT))
        .get(LATEST_URL)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .context("could not reach GitHub")?;
    let body = response
        .body_mut()
        .read_to_string()
        .context("reading release info")?;
    parse_release(&body)
}

/// Download progress shared with the UI thread.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    /// 0 when the server sent no length.
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

impl Progress {
    /// 0.0..=100.0, or `None` while the size is unknown.
    pub fn percent(&self) -> Option<f32> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| {
            (self.done.load(Ordering::Relaxed) as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
        })
    }
}

fn download_to(url: &str, dest: &Path, progress: &Progress) -> Result<()> {
    let response = agent(None)
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("download {url}"))?;
    if let Some(len) = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        progress.total.store(len, Ordering::Relaxed);
    }
    let mut reader = response.into_body().into_reader();
    let mut file = fs::File::create(dest).with_context(|| format!("create {}", dest.display()))?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let n = reader.read(&mut buf).context("download read")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).context("write download")?;
        progress.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    file.flush().context("flush download")?;
    Ok(())
}

fn download_text(url: &str) -> Result<String> {
    let mut response = agent(Some(API_TIMEOUT))
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("download {url}"))?;
    response
        .body_mut()
        .read_to_string()
        .with_context(|| format!("read {url}"))
}

/// Verify a minisign signature over `message` with `public_key`.
pub fn verify_signature(public_key: &str, message: &[u8], signature: &str) -> Result<()> {
    if public_key.is_empty() {
        bail!("this build has no update signing key");
    }
    let key = minisign_verify::PublicKey::from_base64(public_key)
        .map_err(|e| anyhow::anyhow!("bad update public key: {e}"))?;
    let signature = minisign_verify::Signature::decode(signature)
        .map_err(|e| anyhow::anyhow!("bad signature file: {e}"))?;
    key.verify(message, &signature, false)
        .map_err(|e| anyhow::anyhow!("signature does not match: {e}"))
}

/// Hash listed for `name` in a `shasum -a 256` file (`<hash>  ./<name>`).
pub fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim().trim_start_matches('*').trim_start_matches("./");
        (file == name).then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).context("hash read")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// `SHA256SUMS` text, only if its signature verifies.
fn fetch_verified_sums(release: &Release, public_key: &str) -> Result<String> {
    let find = |name: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.url.clone())
            .with_context(|| format!("release has no {name}"))
    };
    let sums = download_text(&find(SUMS_NAME)?)?;
    let signature = download_text(&find(SIG_NAME)?)?;
    verify_signature(public_key, sums.as_bytes(), &signature)?;
    Ok(sums)
}

/// Download `asset` into [`updates_dir`] and verify it. Returns the path.
/// Any older downloads are removed first; a bad file is deleted.
pub fn download_verified(release: &Release, asset: &Asset, progress: &Progress) -> Result<PathBuf> {
    if !is_safe_name(&asset.name) {
        bail!("unexpected asset name {:?}", asset.name);
    }
    let sums = fetch_verified_sums(release, PUBLIC_KEY)?;
    let expected = expected_hash(&sums, &asset.name)
        .with_context(|| format!("{} is not listed in {SUMS_NAME}", asset.name))?;

    let dir = updates_dir();
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let dest = dir.join(&asset.name);
    if let Err(err) = download_to(&asset.url, &dest, progress) {
        let _ = fs::remove_file(&dest);
        return Err(err);
    }
    if sha256_file(&dest)? != expected {
        let _ = fs::remove_file(&dest);
        bail!("checksum mismatch for {} — download discarded", asset.name);
    }
    Ok(dest)
}

/// Replace `exe` with `bin/isengard` from the release tarball. The new file
/// is written next to `exe` and renamed over it (atomic; Linux lets a running
/// executable be replaced).
pub fn install_in_place(tarball: &Path, exe: &Path) -> Result<()> {
    let file = fs::File::open(tarball).with_context(|| format!("open {}", tarball.display()))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let staged = exe.with_file_name(".isengard.update");
    let mut found = false;
    for entry in archive.entries().context("read archive")? {
        let mut entry = entry.context("archive entry")?;
        let path = entry.path().context("entry path")?.into_owned();
        let is_binary = path.file_name().is_some_and(|n| n == "isengard")
            && path.parent().and_then(Path::file_name).is_some_and(|n| n == "bin");
        if !is_binary {
            continue;
        }
        let mut out =
            fs::File::create(&staged).with_context(|| format!("create {}", staged.display()))?;
        std::io::copy(&mut entry, &mut out).context("extract binary")?;
        out.flush().context("flush binary")?;
        found = true;
        break;
    }
    if !found {
        bail!("archive has no bin/isengard");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
            .context("chmod new binary")?;
    }
    if let Err(err) = fs::rename(&staged, exe) {
        let _ = fs::remove_file(&staged);
        return Err(err).with_context(|| format!("replace {}", exe.display()));
    }
    let _ = fs::remove_file(tarball);
    Ok(())
}

/// Name of the app inside the macOS release tarball.
const MAC_APP_NAME: &str = "Isengard.app";

/// Unpack the release tarball into a hidden folder next to `bundle` (same
/// volume, so the final rename is atomic). Returns the new `Isengard.app`.
/// Files we write ourselves carry no quarantine flag, so Gatekeeper stays out.
fn stage_app_bundle(tarball: &Path, bundle: &Path) -> Result<PathBuf> {
    let parent = bundle.parent().context("app bundle has no parent folder")?;
    let staging = parent.join(".isengard-update");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).with_context(|| format!("create {}", staging.display()))?;
    let unpack = || -> Result<PathBuf> {
        let file =
            fs::File::open(tarball).with_context(|| format!("open {}", tarball.display()))?;
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
        archive.set_preserve_permissions(true);
        archive.unpack(&staging).context("unpack archive")?;
        let new = staging.join(MAC_APP_NAME);
        if !new.join("Contents/MacOS/isengard").is_file() {
            bail!("archive has no {MAC_APP_NAME}");
        }
        Ok(new)
    };
    unpack().inspect_err(|_| {
        let _ = fs::remove_dir_all(&staging);
    })
}

/// Put `new` where `bundle` is. The old bundle is moved aside first and
/// restored if the second rename fails, so an interrupted update never leaves
/// the user without an app.
fn swap_app_bundle(new: &Path, bundle: &Path) -> Result<()> {
    let parent = bundle.parent().context("app bundle has no parent folder")?;
    let old = parent.join(".isengard-old.app");
    let _ = fs::remove_dir_all(&old);
    fs::rename(bundle, &old).with_context(|| format!("move aside {}", bundle.display()))?;
    if let Err(err) = fs::rename(new, bundle) {
        let _ = fs::rename(&old, bundle);
        return Err(err).with_context(|| format!("replace {}", bundle.display()));
    }
    let _ = fs::remove_dir_all(&old);
    if let Some(staging) = new.parent() {
        let _ = fs::remove_dir(staging);
    }
    Ok(())
}

/// `codesign --verify`: the unpacked bundle must be intact (any identity).
#[cfg(target_os = "macos")]
fn verify_bundle_intact(new: &Path) -> Result<()> {
    let output = std::process::Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(new)
        .output()
        .context("run codesign")?;
    if !output.status.success() {
        bail!(
            "downloaded app failed its code-signature check: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Replace the running `bundle` with `Isengard.app` from the release tarball
/// and return the binary to restart into.
pub fn install_app_bundle(tarball: &Path, bundle: &Path) -> Result<PathBuf> {
    let new = stage_app_bundle(tarball, bundle)?;
    #[cfg(target_os = "macos")]
    if let Err(err) = verify_bundle_intact(&new) {
        if let Some(staging) = new.parent() {
            let _ = fs::remove_dir_all(staging);
        }
        return Err(err);
    }
    swap_app_bundle(&new, bundle)?;
    let _ = fs::remove_file(tarball);
    Ok(bundle.join("Contents/MacOS/isengard"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE_JSON: &str = r#"{
        "tag_name": "v0.2.0",
        "html_url": "https://github.com/rathpanha/isengard/releases/tag/v0.2.0",
        "body": "- shiny",
        "assets": [
            {"name": "isengard-0.2.0-linux-x86_64.tar.gz", "browser_download_url": "https://x/t.tgz"},
            {"name": "isengard-0.2.0-1.x86_64.rpm", "browser_download_url": "https://x/a.rpm"},
            {"name": "isengard_0.2.0_amd64.deb", "browser_download_url": "https://x/a.deb"},
            {"name": "isengard_0.2.0_arm64.deb", "browser_download_url": "https://x/arm.deb"},
            {"name": "SHA256SUMS", "browser_download_url": "https://x/sums"}
        ]
    }"#;

    #[test]
    fn parses_release_and_strips_v() {
        let r = parse_release(RELEASE_JSON).unwrap();
        assert_eq!((r.tag.as_str(), r.version.as_str()), ("v0.2.0", "0.2.0"));
        assert_eq!(r.notes, "- shiny");
        assert_eq!(r.assets.len(), 5);
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.1.4", "0.1.3"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.3", "0.1.3"));
        assert!(!is_newer("0.1.2", "0.1.3"));
        assert!(!is_newer("nightly", "0.1.3"));
        assert!(is_newer("0.10.0", "0.9.0"), "numeric, not lexicographic");
    }

    #[test]
    fn classifies_installs() {
        let none = |_: &str| false;
        let dnf = |n: &str| n == "dnf";
        let apt = |n: &str| n == "apt";
        let exe = Path::new("/usr/bin/isengard");
        assert_eq!(
            classify(exe, false, dnf),
            InstallKind::Package { kind: PackageKind::Rpm, manager: "dnf" }
        );
        assert_eq!(
            classify(exe, true, apt),
            InstallKind::Package { kind: PackageKind::Deb, manager: "apt" },
            "root-writable /usr/bin is still package-managed"
        );
        assert_eq!(classify(exe, false, none), InstallKind::Manual);
        let home = Path::new("/home/u/.local/bin/isengard");
        assert_eq!(
            classify(home, true, none),
            InstallKind::InPlace { exe: home.to_path_buf() }
        );
        assert_eq!(classify(home, false, none), InstallKind::Manual);
        assert_eq!(
            classify(Path::new("/usr/local/bin/isengard"), false, dnf),
            InstallKind::Manual
        );
        assert_eq!(
            classify(Path::new("/home/u/isengard/target/release/isengard"), true, dnf),
            InstallKind::Manual,
            "cargo builds are never updated"
        );
    }

    #[test]
    fn picks_matching_asset() {
        if std::env::consts::ARCH != "x86_64" {
            return;
        }
        let r = parse_release(RELEASE_JSON).unwrap();
        let name = |kind: InstallKind| pick_asset(&r, &kind).map(|a| a.name.as_str());
        assert_eq!(
            name(InstallKind::InPlace { exe: "/x".into() }),
            Some("isengard-0.2.0-linux-x86_64.tar.gz")
        );
        assert_eq!(
            name(InstallKind::Package { kind: PackageKind::Rpm, manager: "dnf" }),
            Some("isengard-0.2.0-1.x86_64.rpm")
        );
        assert_eq!(
            name(InstallKind::Package { kind: PackageKind::Deb, manager: "apt" }),
            Some("isengard_0.2.0_amd64.deb")
        );
        assert_eq!(name(InstallKind::Manual), None);
    }

    #[test]
    fn rejects_unsafe_asset_names() {
        assert!(is_safe_name("isengard_0.2.0_amd64.deb"));
        assert!(!is_safe_name("evil; rm -rf ~.rpm"));
        assert!(!is_safe_name("$(x).rpm"));
        assert!(!is_safe_name("-rf"));
    }

    #[test]
    fn builds_noninteractive_install_argv() {
        let rpm = Path::new("/tmp/a.rpm");
        assert_eq!(install_argv("dnf", rpm), ["dnf", "install", "-y", "/tmp/a.rpm"]);
        assert_eq!(
            install_argv("apt", Path::new("/tmp/a.deb")),
            ["apt-get", "install", "-y", "/tmp/a.deb"]
        );
        assert_eq!(install_argv("zypper", rpm)[1], "--non-interactive");
    }

    #[test]
    fn quotes_install_command() {
        assert_eq!(
            install_command("dnf", Path::new("/home/u/.cache/isengard/updates/a.rpm")),
            "sudo dnf install '/home/u/.cache/isengard/updates/a.rpm'"
        );
        assert_eq!(
            install_command("apt", Path::new("/tmp/it's/a.deb")),
            r"sudo apt install '/tmp/it'\''s/a.deb'"
        );
    }

    #[test]
    fn finds_expected_hash() {
        let sums = "AAAA  ./isengard_0.2.0_amd64.deb\nbbbb *./other.rpm\n";
        assert_eq!(
            expected_hash(sums, "isengard_0.2.0_amd64.deb").as_deref(),
            Some("aaaa")
        );
        assert_eq!(expected_hash(sums, "other.rpm").as_deref(), Some("bbbb"));
        assert_eq!(expected_hash(sums, "missing"), None);
    }

    #[test]
    fn empty_key_refuses_everything() {
        let err = verify_signature("", b"data", "sig").unwrap_err();
        assert!(err.to_string().contains("no update signing key"));
    }

    #[test]
    fn embedded_public_key_is_valid() {
        assert!(signing_configured());
        assert!(minisign_verify::PublicKey::from_base64(PUBLIC_KEY).is_ok());
        assert!(
            verify_signature(PUBLIC_KEY, b"data", "not a signature")
                .unwrap_err()
                .to_string()
                .contains("bad signature file")
        );
    }

    #[test]
    fn rejects_garbage_signature() {
        // Valid key from the minisign-verify docs; the signature is junk.
        let key = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        assert!(verify_signature(key, b"data", "not a signature").is_err());
    }

    #[test]
    fn hashes_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn installs_binary_from_tarball() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("isengard-0.2.0-linux-x86_64.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                fs::File::create(&tarball).unwrap(),
                flate2::Compression::fast(),
            );
            let mut tar = tar::Builder::new(gz);
            let body = b"new binary";
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, "isengard-0.2.0-linux-x86_64/bin/isengard", &body[..])
                .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let exe = dir.path().join("isengard");
        fs::write(&exe, b"old binary").unwrap();
        install_in_place(&tarball, &exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new binary");
        assert!(!dir.path().join(".isengard.update").exists());
        assert!(!tarball.exists(), "tarball removed after install");
    }

    #[test]
    fn classifies_macos_installs() {
        let exe = Path::new("/Applications/Isengard.app/Contents/MacOS/isengard");
        let bundle = PathBuf::from("/Applications/Isengard.app");
        assert_eq!(
            classify_macos(exe, true),
            InstallKind::MacApp { bundle: bundle.clone() }
        );
        assert_eq!(classify_macos(exe, false), InstallKind::Manual);
        assert_eq!(
            classify_macos(Path::new("/Users/u/isengard/target/release/isengard"), true),
            InstallKind::Manual,
            "bare binaries (cargo run) are never updated"
        );
        assert_eq!(
            classify_macos(Path::new("/Applications/Foo/Contents/MacOS/isengard"), true),
            InstallKind::Manual,
            "the folder must be an .app"
        );
    }

    #[test]
    fn picks_macos_tarball() {
        let asset = |name: &str| Asset { name: name.into(), url: format!("https://x/{name}") };
        let release = Release {
            tag: "v0.2.0".into(),
            version: "0.2.0".into(),
            notes: String::new(),
            page_url: String::new(),
            assets: vec![
                asset("isengard-0.2.0-linux-aarch64.tar.gz"),
                asset("isengard-0.2.0-macos-universal.tar.gz"),
                asset("isengard-0.2.0-macos-universal.dmg"),
            ],
        };
        let kind = InstallKind::MacApp { bundle: "/Applications/Isengard.app".into() };
        assert_eq!(
            pick_asset(&release, &kind).map(|a| a.name.as_str()),
            Some("isengard-0.2.0-macos-universal.tar.gz")
        );
    }

    /// A gzip tarball holding `Isengard.app/Contents/MacOS/isengard` = `body`.
    fn mac_tarball(path: &Path, body: &[u8], with_binary: bool) {
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        let name = if with_binary {
            "Isengard.app/Contents/MacOS/isengard"
        } else {
            "Isengard.app/Contents/Info.plist"
        };
        tar.append_data(&mut header, name, body).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn swaps_app_bundle_from_tarball() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Isengard.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join("Contents/MacOS/isengard"), b"old").unwrap();
        fs::write(bundle.join("Contents/stale"), b"gone after update").unwrap();
        let tarball = dir.path().join("isengard-0.2.0-macos-universal.tar.gz");
        mac_tarball(&tarball, b"new", true);

        let new = stage_app_bundle(&tarball, &bundle).unwrap();
        swap_app_bundle(&new, &bundle).unwrap();

        assert_eq!(fs::read(bundle.join("Contents/MacOS/isengard")).unwrap(), b"new");
        assert!(!bundle.join("Contents/stale").exists());
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers.len(), 2, "only app + tarball remain: {leftovers:?}");
    }

    #[test]
    fn bad_mac_tarball_leaves_app_alone() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Isengard.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join("Contents/MacOS/isengard"), b"old").unwrap();
        let tarball = dir.path().join("t.tar.gz");
        mac_tarball(&tarball, b"x", false);

        assert!(stage_app_bundle(&tarball, &bundle).is_err());
        assert_eq!(fs::read(bundle.join("Contents/MacOS/isengard")).unwrap(), b"old");
        assert!(!dir.path().join(".isengard-update").exists(), "staging cleaned up");
    }

    #[test]
    fn tarball_without_binary_leaves_exe_alone() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("t.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                fs::File::create(&tarball).unwrap(),
                flate2::Compression::fast(),
            );
            tar::Builder::new(gz).into_inner().unwrap().finish().unwrap();
        }
        let exe = dir.path().join("isengard");
        fs::write(&exe, b"old binary").unwrap();
        assert!(install_in_place(&tarball, &exe).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"old binary");
    }
}
