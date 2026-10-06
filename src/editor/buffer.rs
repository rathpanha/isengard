use std::path::{Path, PathBuf};

use anyhow::Context as _;

use super::language::Language;

/// An open file's contents. Phase 3 keeps a plain `String` for a simple egui bridge.
#[derive(Debug)]
pub struct TextBuffer {
    pub content: String,
    pub path: PathBuf,
    pub is_modified: bool,
    pub language: Language,
}

impl TextBuffer {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let content = String::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("{} is not a UTF-8 text file", path.display()))?;
        Ok(Self {
            content,
            path: path.to_path_buf(),
            is_modified: false,
            language: Language::from_path(path),
        })
    }

    pub fn save(&mut self) -> anyhow::Result<()> {
        std::fs::write(&self.path, &self.content)
            .with_context(|| format!("writing {}", self.path.display()))?;
        self.is_modified = false;
        Ok(())
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_edit_save_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app.js");
        std::fs::write(&path, "const x = 1;\n").unwrap();

        let mut buf = TextBuffer::open(&path).unwrap();
        assert_eq!(buf.language, Language::JavaScript);
        buf.content.push_str("x++;\n");
        buf.is_modified = true;
        buf.save().unwrap();

        assert!(!buf.is_modified);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "const x = 1;\nx++;\n");
    }

    #[test]
    fn rejects_binary_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blob.bin");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(TextBuffer::open(&path).is_err());
    }
}
