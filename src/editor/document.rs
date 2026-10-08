use std::path::{Path, PathBuf};

use anyhow::Context as _;

/// Image extensions GPUI can load via `img(path)` (see `ImageFormat`).
/// SVG is UTF-8 text with Edit/Preview (not a pure image tab).
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff",
];

/// In-memory buffer id prefix (`untitled:1`, `untitled:2`, …). Not a real path.
const UNTITLED_PREFIX: &str = "untitled:";

/// True when `path` should open as an image preview instead of a text editor.
pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            IMAGE_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// Reads a file for editing. Fails for files that are not valid UTF-8 text.
pub fn read_text(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("{} is not a UTF-8 text file", path.display()))
}

pub fn write_text(path: &Path, text: &str) -> anyhow::Result<()> {
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// True for welcome-screen buffers that have no path on disk yet.
pub fn is_untitled(path: &Path) -> bool {
    path.to_string_lossy().starts_with(UNTITLED_PREFIX)
}

/// Virtual path for an unsaved buffer (`untitled:1`).
pub fn untitled_path(id: u64) -> PathBuf {
    PathBuf::from(format!("{UNTITLED_PREFIX}{id}"))
}

/// Tab / status label: `Untitled-1` for virtual buffers, else the file name.
pub fn tab_label(path: &Path) -> String {
    match untitled_id(path) {
        Some(id) => format!("Untitled-{id}"),
        None => file_name(path),
    }
}

fn untitled_id(path: &Path) -> Option<u64> {
    path.to_string_lossy()
        .strip_prefix(UNTITLED_PREFIX)?
        .parse()
        .ok()
}

/// Next free `untitled.txt` / `untitled1.txt` / … under `dir`.
pub fn unique_new_file_path(dir: &Path) -> PathBuf {
    let first = dir.join("untitled.txt");
    if !first.exists() {
        return first;
    }
    let mut n = 1u32;
    loop {
        let path = dir.join(format!("untitled{n}.txt"));
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}

/// Next free `New Folder` / `New Folder 1` / … under `dir`.
pub fn unique_new_folder_path(dir: &Path) -> PathBuf {
    let first = dir.join("New Folder");
    if !first.exists() {
        return first;
    }
    let mut n = 1u32;
    loop {
        let path = dir.join(format!("New Folder {n}"));
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}

/// Validates a single path segment for New File / New Folder / Rename.
pub fn validate_entry_name(name: &str) -> Result<&str, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name is required.");
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err("Name cannot contain path separators.");
    }
    if name == "." || name == ".." {
        return Err("Invalid name.");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn read_write_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app.js");
        write_text(&path, "const x = 1;\n").unwrap();
        assert_eq!(read_text(&path).unwrap(), "const x = 1;\n");
    }

    #[test]
    fn rejects_binary_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blob.bin");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(read_text(&path).is_err());
    }

    #[test]
    fn detects_image_extensions() {
        assert!(is_image(Path::new("shot.PNG")));
        assert!(is_image(Path::new("a/b/photo.jpeg")));
        // SVG is text + preview (Language::Svg), not a raster image tab.
        assert!(!is_image(Path::new("icon.svg")));
        assert!(!is_image(Path::new("readme.md")));
        assert!(!is_image(Path::new("blob.bin")));
    }

    #[test]
    fn untitled_helpers() {
        let p = untitled_path(3);
        assert!(is_untitled(&p));
        assert!(!is_untitled(Path::new("/tmp/x.txt")));
        assert_eq!(tab_label(&p), "Untitled-3");
        assert_eq!(tab_label(Path::new("/tmp/x.txt")), "x.txt");
    }

    #[test]
    fn unique_new_file_skips_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert_eq!(unique_new_file_path(dir), dir.join("untitled.txt"));
        std::fs::write(dir.join("untitled.txt"), "").unwrap();
        assert_eq!(unique_new_file_path(dir), dir.join("untitled1.txt"));
        std::fs::write(dir.join("untitled1.txt"), "").unwrap();
        assert_eq!(unique_new_file_path(dir), dir.join("untitled2.txt"));
    }

    #[test]
    fn unique_new_folder_skips_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert_eq!(unique_new_folder_path(dir), dir.join("New Folder"));
        std::fs::create_dir(dir.join("New Folder")).unwrap();
        assert_eq!(unique_new_folder_path(dir), dir.join("New Folder 1"));
    }

    #[test]
    fn validates_entry_names() {
        assert_eq!(validate_entry_name("  app.rs  ").unwrap(), "app.rs");
        assert!(validate_entry_name("").is_err());
        assert!(validate_entry_name("a/b").is_err());
        assert!(validate_entry_name("..").is_err());
    }
}
