//! `file://` URI helpers for LSP document / workspace paths.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context as _, anyhow};
use lsp_types::Uri;

pub fn path_to_uri(path: &Path) -> anyhow::Result<Uri> {
    let url = url::Url::from_file_path(path)
        .map_err(|_| anyhow!("not an absolute path: {}", path.display()))?;
    Uri::from_str(url.as_str()).context("parse file URI")
}

pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let url = url::Url::parse(uri.as_str()).ok()?;
    url.to_file_path().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_absolute_path() {
        let path = if cfg!(windows) {
            PathBuf::from(r"C:\Users\test\proj\src\main.rs")
        } else {
            PathBuf::from("/tmp/isengard-lsp-test/main.rs")
        };
        let uri = path_to_uri(&path).unwrap();
        assert!(uri.as_str().starts_with("file:"));
        let back = uri_to_path(&uri).unwrap();
        // macOS may canonicalize differently; compare via URL again.
        assert_eq!(path_to_uri(&back).unwrap().as_str(), uri.as_str());
    }
}
