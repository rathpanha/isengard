use std::path::Path;

use anyhow::Context as _;

/// Image extensions GPUI can load via `img(path)` (see `ImageFormat`).
/// SVG is UTF-8 text with Edit/Preview (not a pure image tab).
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff",
];

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
}
