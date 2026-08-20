/// One MARD bead color exposed to the drawing tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub name: &'static str,
    pub code: &'static str,
    pub rgb: [u8; 3],
}

/// Stage-1 palette: 24 MARD colors covering common hues.
/// HEX values follow the pixel-beads.com 2026 screen reference chart.
pub const COLORS: &[Color] = &[
    Color {
        name: "白色",
        code: "H2",
        rgb: [0xFF, 0xFF, 0xFF],
    },
    Color {
        name: "浅灰",
        code: "H3",
        rgb: [0xB3, 0xB3, 0xB3],
    },
    Color {
        name: "灰色",
        code: "H4",
        rgb: [0x86, 0x86, 0x86],
    },
    Color {
        name: "黑色",
        code: "H7",
        rgb: [0x00, 0x00, 0x00],
    },
    Color {
        name: "黄色",
        code: "A4",
        rgb: [0xFF, 0xE9, 0x53],
    },
    Color {
        name: "橙色",
        code: "A6",
        rgb: [0xFD, 0xAD, 0x49],
    },
    Color {
        name: "深橙",
        code: "A7",
        rgb: [0xFF, 0x7C, 0x2F],
    },
    Color {
        name: "红色",
        code: "F5",
        rgb: [0xD8, 0x01, 0x27],
    },
    Color {
        name: "亮红",
        code: "F2",
        rgb: [0xFC, 0x3D, 0x45],
    },
    Color {
        name: "粉色",
        code: "E6",
        rgb: [0xFF, 0x34, 0x6B],
    },
    Color {
        name: "肤色",
        code: "E11",
        rgb: [0xFC, 0xDD, 0xD2],
    },
    Color {
        name: "浅肤",
        code: "E16",
        rgb: [0xFF, 0xEC, 0xDE],
    },
    Color {
        name: "棕色",
        code: "G8",
        rgb: [0x59, 0x2A, 0x21],
    },
    Color {
        name: "浅棕",
        code: "G13",
        rgb: [0xB7, 0x71, 0x4A],
    },
    Color {
        name: "绿色",
        code: "B5",
        rgb: [0x00, 0xBD, 0x35],
    },
    Color {
        name: "深绿",
        code: "B8",
        rgb: [0x02, 0x9D, 0x26],
    },
    Color {
        name: "亮绿",
        code: "B2",
        rgb: [0x5B, 0xE4, 0x19],
    },
    Color {
        name: "天蓝",
        code: "C4",
        rgb: [0x42, 0xCC, 0xFF],
    },
    Color {
        name: "蓝色",
        code: "C8",
        rgb: [0x10, 0x54, 0xC0],
    },
    Color {
        name: "深蓝",
        code: "C12",
        rgb: [0x1C, 0x33, 0x4D],
    },
    Color {
        name: "青色",
        code: "C11",
        rgb: [0x03, 0xB9, 0xB9],
    },
    Color {
        name: "紫色",
        code: "D7",
        rgb: [0x6E, 0x39, 0x9A],
    },
    Color {
        name: "品红",
        code: "D13",
        rgb: [0xB9, 0x02, 0x95],
    },
    Color {
        name: "暗红",
        code: "F11",
        rgb: [0x6F, 0x20, 0x1F],
    },
];

pub const WHITE: Color = COLORS[0];

/// Resolve a Chinese color name or MARD code (case-insensitive) to RGB.
pub fn resolve(input: &str) -> Option<[u8; 3]> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    COLORS.iter().find_map(|color| {
        if color.name == trimmed || color.code.eq_ignore_ascii_case(trimmed) {
            Some(color.rgb)
        } else {
            None
        }
    })
}

pub fn names() -> impl Iterator<Item = &'static str> {
    COLORS.iter().map(|color| color.name)
}

pub fn names_joined() -> String {
    names().collect::<Vec<_>>().join("、")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_has_24_colors() {
        assert_eq!(COLORS.len(), 24);
        assert_eq!(names().count(), 24);
        assert!(names_joined().contains("红色"));
    }

    #[test]
    fn resolves_chinese_name() {
        assert_eq!(resolve("红色"), Some([0xD8, 0x01, 0x27]));
        assert_eq!(resolve(" 白色 "), Some([0xFF, 0xFF, 0xFF]));
    }

    #[test]
    fn resolves_mard_code() {
        assert_eq!(resolve("H7"), Some([0, 0, 0]));
        assert_eq!(resolve("h2"), Some([0xFF, 0xFF, 0xFF]));
        assert_eq!(resolve("C12"), Some([0x1C, 0x33, 0x4D]));
    }

    #[test]
    fn rejects_unknown_color() {
        assert_eq!(resolve("彩虹"), None);
        assert_eq!(resolve(""), None);
        assert_eq!(resolve("#FF0000"), None);
    }
}
