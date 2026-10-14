//! Minimal JSON-RPC LSP client over stdio (no Tokio).

use std::collections::HashMap;
use std::io::BufReader;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use lsp_types::notification::{Notification as _, Initialized};
use lsp_types::request::{Initialize, Request as _, Shutdown};
use lsp_types::{InitializeParams, InitializedParams};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::protocol::{read_message, write_message};
use super::registry::ResolvedServerSpec;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

type PendingTx = flume::Sender<Result<Value, String>>;
type PendingMap = HashMap<i64, PendingTx>;

#[derive(Debug)]
pub enum Incoming {
    Notification { method: String, params: Value },
    /// Server → client request; caller must [`LspClient::respond`] or [`LspClient::respond_error`].
    ServerRequest { id: Value, method: String, params: Value },
}

pub struct LspClient {
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<PendingMap>>,
    next_id: AtomicI64,
    child: Mutex<Child>,
    alive: Arc<AtomicBool>,
    _reader: JoinHandle<()>,
}

impl LspClient {
    /// Spawn `spec.command`, initialize, and return the client plus an event channel.
    pub fn start(
        spec: &ResolvedServerSpec,
        init: InitializeParams,
    ) -> anyhow::Result<(Self, flume::Receiver<Incoming>)> {
        // Catch rustup shims / missing components before we block on initialize.
        preflight(spec)?;

        let mut child = Command::new(&spec.command)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn `{}` (is it on PATH?)", spec.command))?;

        let stdin = child.stdin.take().context("LSP stdin")?;
        let stdout = child.stdout.take().context("LSP stdout")?;

        let pending: Arc<Mutex<PendingMap>> = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let (incoming_tx, incoming_rx) = flume::unbounded();

        let pending_reader = pending.clone();
        let alive_reader = alive.clone();
        let reader = thread::Builder::new()
            .name("isengard-lsp-read".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                while alive_reader.load(Ordering::SeqCst) {
                    let body = match read_message(&mut reader) {
                        Ok(Some(b)) => b,
                        Ok(None) => break,
                        Err(err) => {
                            log::warn!("LSP read error: {err:#}");
                            break;
                        }
                    };
                    if let Err(err) =
                        dispatch_message(&body, &pending_reader, &incoming_tx)
                    {
                        log::warn!("LSP dispatch: {err:#}");
                    }
                }
                alive_reader.store(false, Ordering::SeqCst);
                // Fail any waiters so UI tasks don't hang.
                if let Ok(mut map) = pending_reader.lock() {
                    for (_, tx) in map.drain() {
                        let _ = tx.send(Err("language server exited".into()));
                    }
                }
            })
            .context("spawn LSP reader thread")?;

        let client = Self {
            stdin: Mutex::new(stdin),
            pending,
            next_id: AtomicI64::new(1),
            child: Mutex::new(child),
            alive,
            _reader: reader,
        };

        let _init_result: Value = client.request_raw(Initialize::METHOD, init)?;
        client.notify(Initialized::METHOD, InitializedParams {})?;

        Ok((client, incoming_rx))
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn notify<P: Serialize>(&self, method: &str, params: P) -> anyhow::Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_json(&msg)
    }

    pub fn request<R>(&self, params: R::Params) -> anyhow::Result<R::Result>
    where
        R: lsp_types::request::Request,
        R::Params: Serialize,
        R::Result: DeserializeOwned,
    {
        let value = self.request_raw(R::METHOD, params)?;
        serde_json::from_value(value).context("decode LSP result")
    }

    pub fn request_raw<P: Serialize>(&self, method: &str, params: P) -> anyhow::Result<Value> {
        if !self.is_alive() {
            bail!("language server is not running");
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = flume::bounded(1);
        self.pending
            .lock()
            .map_err(|_| anyhow!("LSP pending lock poisoned"))?
            .insert(id, tx);

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(err) = self.write_json(&msg) {
            self.pending.lock().ok().map(|mut m| m.remove(&id));
            return Err(err);
        }

        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(msg)) => bail!("{msg}"),
            Err(_) => {
                self.pending.lock().ok().map(|mut m| m.remove(&id));
                bail!("LSP request `{method}` timed out")
            }
        }
    }

    pub fn respond(&self, id: Value, result: Value) -> anyhow::Result<()> {
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
    }

    pub fn respond_error(&self, id: Value, message: &str) -> anyhow::Result<()> {
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": message },
        }))
    }

    pub fn shutdown(&self) {
        if !self.is_alive() {
            return;
        }
        let _ = self.request::<Shutdown>(());
        let _ = self.notify("exit", json!(null));
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn write_json(&self, value: &Value) -> anyhow::Result<()> {
        let body = serde_json::to_vec(value).context("serialize LSP message")?;
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| anyhow!("LSP stdin lock poisoned"))?;
        write_message(&mut *stdin, &body).context("write LSP message")?;
        Ok(())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn preflight(spec: &ResolvedServerSpec) -> anyhow::Result<()> {
    ensure_command_exists(&spec.command)?;
    // rustup installs a shim that exists on PATH but is not a real server until
    // `rustup component add rust-analyzer`.
    if command_basename(&spec.command) == "rust-analyzer" {
        let status = Command::new(&spec.command)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("`rust-analyzer --version`")?;
        if !status.success() {
            bail!(
                "`rust-analyzer` is not installed (try `rustup component add rust-analyzer`)"
            );
        }
    }
    Ok(())
}

fn command_basename(command: &str) -> &str {
    std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command)
}

fn ensure_command_exists(command: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new(command);
    if path.is_absolute() || command.contains('/') || command.contains('\\') {
        if path.is_file() {
            return Ok(());
        }
        bail!("`{command}` is not an executable file");
    }
    let Some(path_os) = std::env::var_os("PATH") else {
        bail!("`{command}` not found (PATH is empty)");
    };
    for dir in std::env::split_paths(&path_os) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Ok(());
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{command}.exe"));
            if exe.is_file() {
                return Ok(());
            }
        }
    }
    bail!("`{command}` not found on PATH — install the language server yourself");
}

fn dispatch_message(
    body: &[u8],
    pending: &Mutex<PendingMap>,
    incoming: &flume::Sender<Incoming>,
) -> anyhow::Result<()> {
    let msg: Value = serde_json::from_slice(body).context("parse LSP JSON")?;
    let method = msg.get("method").and_then(|m| m.as_str());
    let id = msg.get("id").cloned();

    if let Some(method) = method {
        if let Some(id) = id {
            // Server request
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let _ = incoming.send(Incoming::ServerRequest {
                id,
                method: method.to_string(),
                params,
            });
        } else {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let _ = incoming.send(Incoming::Notification {
                method: method.to_string(),
                params,
            });
        }
        return Ok(());
    }

    // Response to our request
    let Some(id_val) = id else {
        bail!("LSP message with neither method nor id");
    };
    let id = id_val
        .as_i64()
        .or_else(|| id_val.as_u64().map(|u| u as i64))
        .context("LSP response id")?;
    let tx = pending
        .lock()
        .map_err(|_| anyhow!("pending lock poisoned"))?
        .remove(&id);
    let Some(tx) = tx else {
        log::debug!("LSP response for unknown id {id}");
        return Ok(());
    };
    if let Some(err) = msg.get("error") {
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("LSP error")
            .to_string();
        let _ = tx.send(Err(message));
    } else {
        let result = msg.get("result").cloned().unwrap_or(Value::Null);
        let _ = tx.send(Ok(result));
    }
    Ok(())
}
