//! Build ACP `session/prompt` content blocks from text + attachments.

use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{
    ContentBlock, EmbeddedResource, EmbeddedResourceResource, ImageContent, ResourceLink,
    TextContent, TextResourceContents,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

/// What the agent advertised in `initialize.promptCapabilities`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromptCaps {
    pub image: bool,
    pub embedded_context: bool,
}

/// Soft ceiling for embedding file text (bigger → resource link only).
pub const MAX_EMBED_BYTES: usize = 256 * 1024;

/// Max chips in the composer.
pub const MAX_ATTACHMENTS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRange {
    /// 1-based inclusive.
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attachment {
    /// Whole file or a selection from it.
    File {
        path: PathBuf,
        /// When set and agent allows embedded context, send as `resource`.
        text: Option<String>,
        selection: Option<LineRange>,
    },
    /// Non-file text (e.g. terminal selection).
    Snippet {
        label: String,
        text: String,
    },
    Image {
        mime: String,
        data_b64: String,
        label: String,
        /// Disk path when attached from a file (thumbnail via `img(path)`).
        path: Option<PathBuf>,
    },
}

impl Attachment {
    /// Chip / title label — basename only (no leading `@`; the `@` is for typing).
    pub fn label(&self) -> String {
        match self {
            Self::File {
                path, selection, ..
            } => {
                let name = file_label(path);
                match selection {
                    Some(r) if r.start == r.end => format!("{name}:{start}", start = r.start),
                    Some(r) => format!("{name}:{}-{}", r.start, r.end),
                    None if path.is_dir() => format!("{name}/"),
                    None => name,
                }
            }
            Self::Snippet { label, .. } | Self::Image { label, .. } => label.clone(),
        }
    }
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| format!("file://{}", path.display()))
}

/// 1-based line of a byte offset in `text` (clamped).
pub fn line_at_offset(text: &str, offset: usize) -> u32 {
    let offset = offset.min(text.len());
    let line = text[..offset].bytes().filter(|&b| b == b'\n').count() + 1;
    line as u32
}

/// MIME types Claude ACP / vision models reliably accept.
const AGENT_IMAGE_MIMES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

pub fn image_from_bytes(mime: &str, bytes: &[u8], label: impl Into<String>) -> Attachment {
    let (mime, bytes) = normalize_for_agent(mime, bytes);
    Attachment::Image {
        mime,
        data_b64: B64.encode(bytes),
        label: label.into(),
        path: None,
    }
}

/// Load a raster image from disk for ACP `image` blocks + UI thumbnails.
///
/// UI preview uses `path` (original file). Wire bytes are normalized to an
/// agent-safe format (e.g. favicon `.ico` → PNG) so Claude can decode them.
pub fn image_from_path(path: &Path) -> Option<Attachment> {
    let mime = crate::editor::document::image_mime(path)?;
    if !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let (mime, bytes) = normalize_for_agent(mime, &bytes);
    Some(Attachment::Image {
        mime,
        data_b64: B64.encode(bytes),
        label: file_label(path),
        path: Some(path.to_path_buf()),
    })
}

/// Keep PNG/JPEG/GIF/WebP as-is; re-encode ICO/BMP/TIFF (etc.) to PNG.
fn normalize_for_agent(mime: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    if AGENT_IMAGE_MIMES.iter().any(|m| mime.eq_ignore_ascii_case(m)) {
        return (mime.to_owned(), bytes.to_vec());
    }
    match reencode_png(bytes) {
        Some(png) => ("image/png".to_owned(), png),
        None => (mime.to_owned(), bytes.to_vec()),
    }
}

fn reencode_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let mut out = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut out),
        image::ImageFormat::Png,
    )
    .ok()?;
    Some(out)
}

/// File / folder / image attachment from a disk path (for `@` mention resolve
/// and attach-button paths). Images become [`Attachment::Image`]; missing
/// paths return `None`.
pub fn attachment_from_path(path: &Path) -> Option<Attachment> {
    if let Some(image) = image_from_path(path) {
        return Some(image);
    }
    if path.is_file() {
        let text = std::fs::read_to_string(path).ok();
        Some(Attachment::File {
            path: path.to_path_buf(),
            text,
            selection: None,
        })
    } else if path.is_dir() {
        Some(Attachment::File {
            path: path.to_path_buf(),
            text: None,
            selection: None,
        })
    } else {
        None
    }
}

fn attachment_path(a: &Attachment) -> Option<&Path> {
    match a {
        Attachment::File { path, .. } => Some(path),
        Attachment::Image { path, .. } => path.as_deref(),
        Attachment::Snippet { .. } => None,
    }
}

/// Chip attachments first, then `@`-resolved paths not already in chips.
pub fn merge_attachments(chips: Vec<Attachment>, mentioned: Vec<Attachment>) -> Vec<Attachment> {
    let mut out = chips;
    for a in mentioned {
        let dup = attachment_path(&a).is_some_and(|p| {
            out.iter()
                .any(|b| attachment_path(b).is_some_and(|q| q == p))
        });
        if !dup {
            out.push(a);
        }
    }
    out
}

/// Assemble prompt blocks: attachments first (context), then the user text.
pub fn build_blocks(text: &str, attachments: &[Attachment], caps: PromptCaps) -> Vec<ContentBlock> {
    let mut blocks = Vec::with_capacity(attachments.len() + 1);
    for attachment in attachments {
        if let Some(block) = attachment_block(attachment, caps) {
            blocks.push(block);
        }
    }
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(trimmed)));
    }
    blocks
}

fn attachment_block(attachment: &Attachment, caps: PromptCaps) -> Option<ContentBlock> {
    match attachment {
        Attachment::File {
            path,
            text,
            selection,
        } => {
            let uri = file_uri(path);
            match (caps.embedded_context, text.as_ref()) {
                (true, Some(body)) if body.len() <= MAX_EMBED_BYTES => {
                    let mut contents = TextResourceContents::new(body.clone(), uri);
                    contents = contents.mime_type("text/plain".to_owned());
                    Some(ContentBlock::Resource(EmbeddedResource::new(
                        EmbeddedResourceResource::TextResourceContents(contents),
                    )))
                }
                // Agent didn't advertise embed (or file was huge): still put
                // bytes in a text block so the model sees them. ResourceLink
                // alone is only a markdown [@name](uri) in Claude ACP.
                (_, Some(body)) if body.len() <= MAX_EMBED_BYTES => {
                    Some(ContentBlock::Text(TextContent::new(format!(
                        "<context ref=\"{uri}\">\n{body}\n</context>"
                    ))))
                }
                _ => {
                    let mut link = ResourceLink::new(file_label(path), uri);
                    if let Some(r) = selection {
                        link = link.description(format!("lines {}–{}", r.start, r.end));
                    }
                    Some(ContentBlock::ResourceLink(link))
                }
            }
        }
        Attachment::Snippet { label, text } => {
            let body = text.as_str();
            if body.len() > MAX_EMBED_BYTES {
                return None;
            }
            let uri = format!("snippet://{label}");
            if caps.embedded_context {
                let mut contents = TextResourceContents::new(body.to_owned(), uri);
                contents = contents.mime_type("text/plain".to_owned());
                Some(ContentBlock::Resource(EmbeddedResource::new(
                    EmbeddedResourceResource::TextResourceContents(contents),
                )))
            } else {
                Some(ContentBlock::Text(TextContent::new(format!(
                    "<context ref=\"{uri}\">\n{body}\n</context>"
                ))))
            }
        }
        Attachment::Image {
            mime,
            data_b64,
            path: _,
            label: _,
        } => {
            if !caps.image {
                return None;
            }
            Some(ContentBlock::Image(ImageContent::new(
                data_b64.clone(),
                mime.clone(),
            )))
        }
    }
}

pub fn display_user_message(text: &str, attachments: &[Attachment]) -> String {
    let mut out = text.trim().to_owned();
    for attachment in attachments {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&attachment.label());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_from_path_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.rs");
        std::fs::write(&path, "fn main() {}").unwrap();
        let att = attachment_from_path(&path).expect("file");
        match att {
            Attachment::File {
                text: Some(body), ..
            } => assert!(body.contains("fn main")),
            other => panic!("expected File, got {other:?}"),
        }
        assert!(attachment_from_path(dir.path()).is_some());
    }

    #[test]
    fn merge_attachments_dedupes_by_path() {
        let path = PathBuf::from("/w/a.rs");
        let chip = Attachment::File {
            path: path.clone(),
            text: Some("chip".into()),
            selection: None,
        };
        let mention = Attachment::File {
            path,
            text: Some("mention".into()),
            selection: None,
        };
        let merged = merge_attachments(vec![chip.clone()], vec![mention]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0], chip);
    }

    #[test]
    fn embeds_when_capable_and_small() {
        let path = PathBuf::from("/w/a.rs");
        let blocks = build_blocks(
            "fix this",
            &[Attachment::File {
                path: path.clone(),
                text: Some("fn main() {}".into()),
                selection: None,
            }],
            PromptCaps {
                image: false,
                embedded_context: true,
            },
        );
        assert!(matches!(blocks[0], ContentBlock::Resource(_)));
        assert!(matches!(blocks[1], ContentBlock::Text(_)));
    }

    #[test]
    fn falls_back_to_text_context_without_embed_cap() {
        let blocks = build_blocks(
            "hi",
            &[Attachment::File {
                path: PathBuf::from("/w/a.rs"),
                text: Some("body".into()),
                selection: Some(LineRange { start: 2, end: 4 }),
            }],
            PromptCaps::default(),
        );
        let ContentBlock::Text(t) = &blocks[0] else {
            panic!("expected text context, got {:?}", blocks[0]);
        };
        assert!(t.text.contains("body"));
        assert!(t.text.contains("file://") || t.text.contains("/w/a.rs"));
        assert!(matches!(blocks[1], ContentBlock::Text(_)));
    }

    #[test]
    fn resource_link_when_no_text() {
        let blocks = build_blocks(
            "hi",
            &[Attachment::File {
                path: PathBuf::from("/w/a.rs"),
                text: None,
                selection: Some(LineRange { start: 2, end: 4 }),
            }],
            PromptCaps::default(),
        );
        let ContentBlock::ResourceLink(link) = &blocks[0] else {
            panic!("expected link");
        };
        assert_eq!(link.name, "a.rs");
        assert_eq!(link.description.as_deref(), Some("lines 2–4"));
    }

    #[test]
    fn skips_images_without_cap() {
        let blocks = build_blocks(
            "see",
            &[image_from_bytes("image/png", b"\x89PNG", "paste.png")],
            PromptCaps::default(),
        );
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], ContentBlock::Text(_)));
    }

    #[test]
    fn includes_images_when_capable() {
        let blocks = build_blocks(
            "",
            &[image_from_bytes("image/png", b"\x89PNG", "paste.png")],
            PromptCaps {
                image: true,
                embedded_context: false,
            },
        );
        assert!(matches!(blocks[0], ContentBlock::Image(_)));
    }

    #[test]
    fn image_from_path_reads_raster() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        // Minimal 1×1 PNG.
        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xFE,
            0x02, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        std::fs::write(&path, png).unwrap();
        let att = image_from_path(&path).expect("png attachment");
        match att {
            Attachment::Image {
                mime,
                label,
                path: Some(p),
                ..
            } => {
                assert_eq!(mime, "image/png");
                assert_eq!(label, "shot.png");
                assert_eq!(p, path);
            }
            other => panic!("expected Image, got {other:?}"),
        }
        assert!(image_from_path(dir.path()).is_none());
        assert!(image_from_path(&dir.path().join("a.rs")).is_none());
    }

    #[test]
    fn favicon_ico_is_sent_as_png() {
        let mut rgba = image::RgbaImage::new(16, 16);
        for px in rgba.pixels_mut() {
            *px = image::Rgba([0x20, 0x40, 0xc0, 0xff]);
        }
        let img = image::DynamicImage::ImageRgba8(rgba);
        let mut ico = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut ico), image::ImageFormat::Ico)
            .expect("encode ico");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("favicon.ico");
        std::fs::write(&path, &ico).unwrap();

        let att = image_from_path(&path).expect("ico attachment");
        let Attachment::Image {
            mime,
            data_b64,
            path: Some(p),
            ..
        } = att
        else {
            panic!("expected Image with path");
        };
        assert_eq!(p, path, "UI thumb still points at the .ico");
        assert_eq!(mime, "image/png", "agent wire format must be PNG");
        let wire = B64.decode(data_b64).unwrap();
        assert!(
            wire.starts_with(&[0x89, 0x50, 0x4E, 0x47]),
            "wire bytes must be a PNG file"
        );
        let caps = PromptCaps {
            image: true,
            embedded_context: false,
        };
        let blocks = build_blocks("", &[image_from_path(&path).unwrap()], caps);
        let ContentBlock::Image(img) = &blocks[0] else {
            panic!("expected Image block");
        };
        assert_eq!(img.mime_type, "image/png");
    }

    #[test]
    fn line_at_offset_is_one_based() {
        assert_eq!(line_at_offset("a\nb\nc", 0), 1);
        assert_eq!(line_at_offset("a\nb\nc", 2), 2);
        assert_eq!(line_at_offset("a\nb\nc", 4), 3);
    }

    #[test]
    fn snippet_becomes_context_text() {
        let blocks = build_blocks(
            "explain",
            &[Attachment::Snippet {
                label: "Terminal".into(),
                text: "cargo test".into(),
            }],
            PromptCaps::default(),
        );
        assert_eq!(blocks.len(), 2);
        let ContentBlock::Text(t) = &blocks[0] else {
            panic!("expected text context");
        };
        assert!(t.text.contains("snippet://Terminal"));
        assert!(t.text.contains("cargo test"));
    }
}

#[cfg(test)]
mod serde_shape {
    use super::*;

    #[test]
    fn resource_serializes_like_claude_expects() {
        let blocks = build_blocks(
            "fix",
            &[Attachment::File {
                path: PathBuf::from("/tmp/a.rs"),
                text: Some("fn main() {}".into()),
                selection: None,
            }],
            PromptCaps {
                image: false,
                embedded_context: true,
            },
        );
        let v = serde_json::to_value(&blocks[0]).unwrap();
        assert_eq!(v["type"], "resource");
        assert!(v["resource"]["text"].as_str().unwrap().contains("fn main"));
        assert!(v["resource"]["uri"].as_str().unwrap().starts_with("file://"));
    }
}
