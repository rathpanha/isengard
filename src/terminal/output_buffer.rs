//! Byte-capped capture for ACP `terminal/output` (truncate from the front).

use std::io::{self, Read};
use std::sync::{Arc, Mutex};

/// Default when the agent omits `outputByteLimit` (1 MiB).
pub const DEFAULT_OUTPUT_BYTE_LIMIT: usize = 1024 * 1024;

/// Ring-style buffer: keep the newest bytes, drop from the start at a UTF-8 boundary.
#[derive(Debug, Clone)]
pub struct OutputBuffer {
    limit: usize,
    bytes: Vec<u8>,
    truncated: bool,
}

impl OutputBuffer {
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            bytes: Vec::new(),
            truncated: false,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.bytes.extend_from_slice(data);
        self.trim();
    }

    fn trim(&mut self) {
        if self.bytes.len() <= self.limit {
            return;
        }
        self.truncated = true;
        let overflow = self.bytes.len() - self.limit;
        let drop = char_boundary_at_or_after(&self.bytes, overflow);
        self.bytes.drain(..drop);
    }

    pub fn snapshot(&self) -> (String, bool) {
        (
            String::from_utf8_lossy(&self.bytes).into_owned(),
            self.truncated,
        )
    }
}

/// Skip forward from `index` until a UTF-8 character boundary (or end).
fn char_boundary_at_or_after(bytes: &[u8], mut index: usize) -> usize {
    if index >= bytes.len() {
        return bytes.len();
    }
    while index < bytes.len() && (bytes[index] & 0b1100_0000) == 0b1000_0000 {
        index += 1;
    }
    index
}

/// Copies every read into `capture` while forwarding to the terminal view.
pub struct TeeReader<R> {
    inner: R,
    capture: Arc<Mutex<OutputBuffer>>,
}

impl<R> TeeReader<R> {
    pub fn new(inner: R, capture: Arc<Mutex<OutputBuffer>>) -> Self {
        Self { inner, capture }
    }
}

impl<R: Read> Read for TeeReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.capture.lock().unwrap().push(&buf[..n]);
        }
        Ok(n)
    }
}

/// Exit status shared with ACP `wait_for_exit` / `output`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedExit {
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
}

impl CapturedExit {
    pub fn from_portable(status: &portable_pty::ExitStatus) -> Self {
        if let Some(signal) = status.signal() {
            Self {
                exit_code: None,
                signal: Some(signal.to_owned()),
            }
        } else {
            Self {
                exit_code: Some(status.exit_code()),
                signal: None,
            }
        }
    }
}

/// One-shot exit: late waiters get the stored status immediately.
#[derive(Default)]
pub struct SharedExit {
    status: Mutex<Option<CapturedExit>>,
    waiters: Mutex<Vec<flume::Sender<CapturedExit>>>,
}

impl SharedExit {
    pub fn set(&self, exit: CapturedExit) {
        let mut slot = self.status.lock().unwrap();
        if slot.is_some() {
            return;
        }
        *slot = Some(exit.clone());
        drop(slot);
        for waiter in self.waiters.lock().unwrap().drain(..) {
            let _ = waiter.send(exit.clone());
        }
    }

    pub fn get(&self) -> Option<CapturedExit> {
        self.status.lock().unwrap().clone()
    }

    /// Receiver that yields as soon as the process has exited.
    pub fn subscribe(&self) -> flume::Receiver<CapturedExit> {
        let (tx, rx) = flume::bounded(1);
        let slot = self.status.lock().unwrap();
        if let Some(exit) = slot.clone() {
            let _ = tx.send(exit);
        } else {
            drop(slot);
            self.waiters.lock().unwrap().push(tx);
        }
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_from_the_front_at_a_char_boundary() {
        let mut buf = OutputBuffer::new(5);
        buf.push(b"hello world");
        let (text, truncated) = buf.snapshot();
        assert!(truncated);
        assert!(text.len() <= 5);
        assert!(text.is_char_boundary(0));
        assert_eq!(text, "world");
    }

    #[test]
    fn tee_reader_copies_bytes() {
        let capture = Arc::new(Mutex::new(OutputBuffer::new(1024)));
        let mut tee = TeeReader::new(std::io::Cursor::new(b"abc"), capture.clone());
        let mut out = Vec::new();
        tee.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"abc");
        assert_eq!(capture.lock().unwrap().snapshot().0, "abc");
    }

    #[test]
    fn shared_exit_notifies_late_and_early_waiters() {
        let exit = SharedExit::default();
        let early = exit.subscribe();
        exit.set(CapturedExit {
            exit_code: Some(0),
            signal: None,
        });
        assert_eq!(early.recv().unwrap().exit_code, Some(0));
        let late = exit.subscribe();
        assert_eq!(late.recv().unwrap().exit_code, Some(0));
    }
}
