use egui::Color32;

use super::highlight::HIGHLIGHT_NAMES;

/// Color for a highlight index into [`HIGHLIGHT_NAMES`], or `None` for the default text color.
pub fn color_for(highlight: usize, dark: bool) -> Option<Color32> {
    let name = HIGHLIGHT_NAMES.get(highlight)?;
    let group = name.split('.').next().unwrap_or(name);
    let (d, l) = match (*name, group) {
        ("string.special.key", _) => (rgb(0x9c, 0xdc, 0xfe), rgb(0x00, 0x51, 0x9b)),
        ("constant.builtin", _) => (rgb(0x56, 0x9c, 0xd6), rgb(0x00, 0x00, 0xff)),
        ("variable.builtin", _) => (rgb(0x56, 0x9c, 0xd6), rgb(0x00, 0x00, 0xff)),
        ("variable.parameter", _) => (rgb(0x9c, 0xdc, 0xfe), rgb(0x00, 0x10, 0x80)),
        (_, "keyword" | "media" | "import" | "charset" | "keyframes" | "supports") => {
            (rgb(0xc5, 0x86, 0xc0), rgb(0xaf, 0x00, 0xdb))
        }
        (_, "string") => (rgb(0x98, 0xc3, 0x79), rgb(0x2e, 0x7d, 0x32)),
        (_, "escape") => (rgb(0xd7, 0xba, 0x7d), rgb(0xee, 0x00, 0x00)),
        (_, "comment") => (rgb(0x80, 0x80, 0x80), rgb(0x80, 0x80, 0x80)),
        (_, "function" | "constructor") => (rgb(0xdc, 0xdc, 0xaa), rgb(0x79, 0x5e, 0x26)),
        (_, "type" | "namespace") => (rgb(0x4e, 0xc9, 0xb0), rgb(0x26, 0x7f, 0x99)),
        (_, "number") => (rgb(0xb5, 0xce, 0xa8), rgb(0x09, 0x86, 0x58)),
        (_, "constant") => (rgb(0x4f, 0xc1, 0xff), rgb(0x00, 0x70, 0xc1)),
        (_, "operator") => (rgb(0xd4, 0xd4, 0xd4), rgb(0x38, 0x38, 0x38)),
        (_, "property") => (rgb(0x9c, 0xdc, 0xfe), rgb(0x00, 0x10, 0x80)),
        (_, "tag") => (rgb(0x56, 0x9c, 0xd6), rgb(0x80, 0x00, 0x00)),
        (_, "attribute") => (rgb(0x9c, 0xdc, 0xfe), rgb(0xe5, 0x00, 0x00)),
        (_, "punctuation") => (rgb(0xa0, 0xa0, 0xa0), rgb(0x60, 0x60, 0x60)),
        _ => return None,
    };
    Some(if dark { d } else { l })
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx(name: &str) -> usize {
        HIGHLIGHT_NAMES.iter().position(|n| *n == name).unwrap()
    }

    #[test]
    fn every_highlight_name_has_a_color() {
        for (i, name) in HIGHLIGHT_NAMES.iter().enumerate() {
            if *name == "variable" || *name == "embedded" {
                continue; // intentionally default-colored
            }
            assert!(color_for(i, true).is_some(), "{name} has no color");
        }
    }

    #[test]
    fn plan_colors() {
        assert_eq!(color_for(idx("keyword"), true), Some(rgb(0xc5, 0x86, 0xc0)));
        assert_eq!(color_for(idx("string"), true), Some(rgb(0x98, 0xc3, 0x79)));
        assert_eq!(color_for(idx("comment"), true), Some(rgb(0x80, 0x80, 0x80)));
    }
}
