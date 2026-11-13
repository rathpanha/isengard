//! `file://` URI helpers for LSP document / workspace paths.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context as _, anyhow};
use lsp_types::Uri;

pub fn path_to_uri(path: &Path) -> anyhow::Result<Uri> {
    let url = url::Url::from_file_path(path)
        .map_err(|_| anyhow!("not an absolute path: {}", path.display()))?;
    // `Url` leaves `[ ] ^ |` raw in paths (Next.js `[locale]` dirs), but
    // `lsp_types::Uri` rejects them — encode so such files still get an LSP.
    let mut encoded = String::with_capacity(url.as_str().len());
    for c in url.as_str().chars() {
        match c {
            '[' => encoded.push_str("%5B"),
            ']' => encoded.push_str("%5D"),
            '^' => encoded.push_str("%5E"),
            '|' => encoded.push_str("%7C"),
            _ => encoded.push(c),
        }
    }
    Uri::from_str(&encoded).context("parse file URI")
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

    #[test]
    fn brackets_in_path_are_encoded_and_round_trip() {
        let path = PathBuf::from("/tmp/app/[locale]/articles/page.tsx");
        let uri = path_to_uri(&path).unwrap();
        assert!(uri.as_str().ends_with("/%5Blocale%5D/articles/page.tsx"));
        assert_eq!(uri_to_path(&uri).unwrap(), path);
    }
}
