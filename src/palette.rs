use std::fmt::Write as _;

/// One MARD bead color exposed to the drawing tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub name: &'static str,
    pub code: &'static str,
    pub rgb: [u8; 3],
}

/// Palette locked onto a canvas at creation time.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    Colors24,
    Colors144,
    #[default]
    Colors221,
}

impl PaletteMode {
    /// Parse `"24"` / `"144"` / `"221"`. Missing or empty input defaults to 221.
    pub fn parse(input: Option<&str>) -> Result<Self, String> {
        match input.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::Colors221),
            Some("24") => Ok(Self::Colors24),
            Some("144") => Ok(Self::Colors144),
            Some("221") => Ok(Self::Colors221),
            Some(other) => Err(format!(
                "色表参数无效：「{other}」。请使用 \"24\"、\"144\" 或 \"221\"。"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Colors24 => "24",
            Self::Colors144 => "144",
            Self::Colors221 => "221",
        }
    }

    pub fn colors(self) -> &'static [Color] {
        match self {
            Self::Colors24 => COLORS_24,
            Self::Colors144 => COLORS_144,
            Self::Colors221 => COLORS_221,
        }
    }

    pub fn white_rgb(self) -> [u8; 3] {
        resolve(self, "H2").expect("H2 exists in all palettes")
    }
}

macro_rules! colors {
    ($($name:literal, $code:literal, $r:expr, $g:expr, $b:expr);+ $(;)?) => {
        &[
            $(
                Color {
                    name: $name,
                    code: $code,
                    rgb: [$r, $g, $b],
                }
            ),+
        ]
    };
}

#[path = "palette_221.rs"]
mod palette_221;
pub use palette_221::COLORS_221;

/// 24-color mode. Names, codes, and RGB stay as originally shipped.
pub const COLORS_24: &[Color] = colors![
    "白色", "H2", 0xFF, 0xFF, 0xFF;
    "浅灰", "H3", 0xB3, 0xB3, 0xB3;
    "灰色", "H4", 0x86, 0x86, 0x86;
    "黑色", "H7", 0x00, 0x00, 0x00;
    "黄色", "A4", 0xFF, 0xE9, 0x53;
    "橙色", "A6", 0xFD, 0xAD, 0x49;
    "深橙", "A7", 0xFF, 0x7C, 0x2F;
    "红色", "F5", 0xD8, 0x01, 0x27;
    "亮红", "F2", 0xFC, 0x3D, 0x45;
    "粉色", "E6", 0xFF, 0x34, 0x6B;
    "肤色", "E11", 0xFC, 0xDD, 0xD2;
    "浅肤", "E16", 0xFF, 0xEC, 0xDE;
    "棕色", "G8", 0x59, 0x2A, 0x21;
    "浅棕", "G13", 0xB7, 0x71, 0x4A;
    "绿色", "B5", 0x00, 0xBD, 0x35;
    "深绿", "B8", 0x02, 0x9D, 0x26;
    "亮绿", "B2", 0x5B, 0xE4, 0x19;
    "天蓝", "C4", 0x42, 0xCC, 0xFF;
    "蓝色", "C8", 0x10, 0x54, 0xC0;
    "深蓝", "C12", 0x1C, 0x33, 0x4D;
    "青色", "C11", 0x03, 0xB9, 0xB9;
    "紫色", "D7", 0x6E, 0x39, 0x9A;
    "品红", "D13", 0xB9, 0x02, 0x95;
    "暗红", "F11", 0x6F, 0x20, 0x1F;
];

/// 144-color mode. HEX from Pixelbead 2026 MARD chart. Names are independent of 24-color mode.
pub const COLORS_144: &[Color] = colors![
    "奶黄", "A1", 0xFA, 0xF4, 0xC8;
    "浅柠", "A3", 0xFE, 0xFF, 0x8B;
    "鹅黄", "A4", 0xFB, 0xED, 0x56;
    "正黄", "A5", 0xF4, 0xD7, 0x38;
    "杏橙", "A6", 0xFE, 0xAC, 0x4C;
    "橘橙", "A7", 0xFE, 0x8B, 0x4C;
    "金黄", "A8", 0xFF, 0xDA, 0x45;
    "蜜橙", "A9", 0xFF, 0x99, 0x5B;
    "柿橙", "A10", 0xF7, 0x7C, 0x31;
    "浅杏", "A11", 0xFF, 0xDD, 0x99;
    "肉橙", "A12", 0xFE, 0x9F, 0x72;
    "琥珀", "A13", 0xFF, 0xC3, 0x65;
    "橘红", "A14", 0xFD, 0x54, 0x3D;
    "亮柠", "A15", 0xFF, 0xF3, 0x65;
    "麦黄", "A17", 0xFF, 0xE3, 0x6E;
    "浅橘", "A18", 0xFE, 0xBE, 0x7D;
    "珊瑚", "A19", 0xFD, 0x7C, 0x72;
    "菊黄", "A26", 0xFF, 0xC8, 0x30;
    "黄绿", "B1", 0xE6, 0xEE, 0x31;
    "荧光绿", "B2", 0x63, 0xF3, 0x47;
    "嫩绿", "B3", 0x9E, 0xF7, 0x80;
    "鲜绿", "B4", 0x5D, 0xE0, 0x35;
    "草绿", "B5", 0x35, 0xE3, 0x52;
    "薄荷", "B6", 0x65, 0xE2, 0xA6;
    "翠绿", "B7", 0x3D, 0xAF, 0x80;
    "正绿", "B8", 0x1C, 0x9C, 0x4F;
    "墨绿", "B9", 0x27, 0x52, 0x3A;
    "水绿", "B10", 0x95, 0xD3, 0xC2;
    "军绿", "B11", 0x5D, 0x72, 0x2A;
    "森绿", "B12", 0x16, 0x6F, 0x41;
    "芽绿", "B13", 0xCA, 0xEB, 0x7B;
    "叶绿", "B14", 0xAD, 0xE9, 0x46;
    "暗绿", "B15", 0x2E, 0x51, 0x32;
    "橄榄绿", "B17", 0x9B, 0xB1, 0x3A;
    "青绿", "B19", 0x24, 0xB8, 0x8C;
    "松绿", "B21", 0x15, 0x6A, 0x6B;
    "深松", "B22", 0x0B, 0x3C, 0x43;
    "褐绿", "B23", 0x30, 0x3A, 0x21;
    "苔绿", "B25", 0x4E, 0x84, 0x6D;
    "茶绿", "B32", 0x9C, 0xAB, 0x5A;
    "冰青", "C2", 0xA9, 0xF9, 0xFC;
    "浅青", "C3", 0xA0, 0xE2, 0xFB;
    "天蓝", "C4", 0x41, 0xCC, 0xFF;
    "湖蓝", "C5", 0x01, 0xAC, 0xEB;
    "亮蓝", "C6", 0x50, 0xAA, 0xF0;
    "钴蓝", "C7", 0x36, 0x77, 0xD2;
    "正蓝", "C8", 0x0F, 0x54, 0xC0;
    "宝蓝", "C9", 0x32, 0x4B, 0xCA;
    "碧青", "C11", 0x28, 0xDD, 0xDE;
    "海军", "C12", 0x1C, 0x33, 0x4D;
    "雾蓝", "C13", 0xCD, 0xE8, 0xFF;
    "玉青", "C15", 0x22, 0xC4, 0xC6;
    "靛蓝", "C16", 0x15, 0x57, 0xA8;
    "墨蓝", "C18", 0x1D, 0x33, 0x44;
    "孔雀", "C19", 0x18, 0x87, 0xA2;
    "海蓝", "C20", 0x17, 0x6D, 0xAF;
    "灰青", "C22", 0x67, 0xB4, 0xBE;
    "晴空", "C24", 0x7C, 0xC4, 0xFF;
    "矢车菊", "C26", 0x3C, 0xAE, 0xD8;
    "群青", "C29", 0x34, 0x48, 0x8E;
    "浅紫", "D1", 0xAE, 0xB4, 0xF2;
    "薰衣草", "D2", 0x85, 0x8E, 0xDD;
    "蓝紫", "D3", 0x2F, 0x54, 0xAF;
    "深紫", "D4", 0x18, 0x2A, 0x84;
    "亮紫", "D5", 0xB8, 0x43, 0xC5;
    "丁香", "D6", 0xAC, 0x7B, 0xDE;
    "葡萄紫", "D7", 0x88, 0x54, 0xB3;
    "淡紫", "D8", 0xE2, 0xD3, 0xFF;
    "暗紫", "D10", 0x36, 0x18, 0x51;
    "粉紫", "D12", 0xDE, 0x9A, 0xD4;
    "洋红", "D13", 0xB9, 0x00, 0x95;
    "兰紫", "D14", 0x8B, 0x27, 0x9B;
    "靛紫", "D15", 0x2F, 0x1F, 0x90;
    "紫藤", "D18", 0xA4, 0x5E, 0xC7;
    "玫紫", "D21", 0x9A, 0x00, 0x9B;
    "鸢尾", "D25", 0x49, 0x4F, 0xC7;
    "裸粉", "E1", 0xFD, 0xD3, 0xCC;
    "桃粉", "E4", 0xE8, 0x64, 0x9E;
    "亮粉", "E5", 0xF5, 0x51, 0xA2;
    "玫红", "E6", 0xF1, 0x3D, 0x74;
    "深粉", "E7", 0xC6, 0x34, 0x78;
    "兰粉", "E9", 0xE9, 0x70, 0xCC;
    "紫粉", "E10", 0xD3, 0x37, 0x93;
    "暖肤", "E11", 0xFC, 0xDD, 0xD2;
    "浅玫", "E12", 0xF7, 0x8F, 0xC3;
    "绛粉", "E13", 0xB5, 0x00, 0x6D;
    "蜜肤", "E14", 0xFF, 0xD1, 0xBA;
    "瓷肤", "E16", 0xFF, 0xF3, 0xEB;
    "樱粉", "E18", 0xFF, 0xC7, 0xDB;
    "灰粉", "E21", 0xBD, 0x9D, 0xA1;
    "藕紫", "E22", 0xB7, 0x85, 0xA1;
    "浅藕", "E24", 0xE1, 0xBC, 0xE8;
    "鲑红", "F1", 0xFD, 0x95, 0x7B;
    "鲜红", "F2", 0xFC, 0x3D, 0x46;
    "朱红", "F3", 0xF7, 0x49, 0x41;
    "艳红", "F4", 0xFC, 0x28, 0x3C;
    "正红", "F5", 0xE7, 0x00, 0x2F;
    "砖红", "F6", 0x94, 0x36, 0x30;
    "绛红", "F7", 0x97, 0x19, 0x37;
    "深红", "F8", 0xBC, 0x00, 0x28;
    "蔷薇", "F9", 0xE2, 0x67, 0x7A;
    "赭石", "F10", 0x8A, 0x45, 0x26;
    "褐红", "F11", 0x5A, 0x21, 0x21;
    "西瓜", "F12", 0xFD, 0x4E, 0x6A;
    "橘绯", "F13", 0xF3, 0x57, 0x44;
    "浅鲑", "F14", 0xFF, 0xA9, 0xAD;
    "血红", "F15", 0xD3, 0x00, 0x22;
    "陶土", "F17", 0xE6, 0x9C, 0x79;
    "锈红", "F19", 0xC1, 0x44, 0x4A;
    "番茄", "F25", 0xE5, 0x4B, 0x4F;
    "浅杏肤", "G1", 0xFF, 0xE2, 0xCE;
    "桃肤", "G2", 0xFF, 0xC4, 0xAA;
    "暖杏", "G3", 0xF4, 0xC3, 0xA5;
    "浅咖", "G4", 0xE1, 0xB3, 0x83;
    "姜黄", "G6", 0xE9, 0x9C, 0x17;
    "可可", "G7", 0x9D, 0x5B, 0x3E;
    "深棕", "G8", 0x75, 0x38, 0x32;
    "金棕", "G10", 0xD9, 0x8C, 0x39;
    "驼棕", "G13", 0xB7, 0x71, 0x4A;
    "灰棕", "G14", 0x8D, 0x61, 0x4C;
    "米棕", "G16", 0xF2, 0xD9, 0xBA;
    "咖啡", "G17", 0x78, 0x52, 0x4B;
    "铜棕", "G19", 0xE0, 0x79, 0x35;
    "赤棕", "G20", 0xA9, 0x40, 0x23;
    "纯白", "H2", 0xFE, 0xFF, 0xFF;
    "银灰", "H3", 0xB6, 0xB1, 0xBA;
    "中灰", "H4", 0x89, 0x85, 0x8C;
    "深灰", "H5", 0x48, 0x46, 0x4E;
    "炭黑", "H6", 0x2F, 0x2B, 0x2F;
    "纯黑", "H7", 0x00, 0x00, 0x00;
    "雾白", "H9", 0xED, 0xED, 0xED;
    "暖白", "H12", 0xFF, 0xF5, 0xED;
    "米白", "H13", 0xF5, 0xEC, 0xD2;
    "青灰", "H15", 0x98, 0xA6, 0xA8;
    "褐黑", "H16", 0x1D, 0x14, 0x14;
    "鼠灰", "H20", 0x94, 0x9F, 0xA3;
    "灰绿", "M1", 0xBC, 0xC6, 0xB8;
    "鼠绿", "M2", 0x8A, 0xA3, 0x86;
    "卡其", "M4", 0xE3, 0xD2, 0xBC;
    "橄榄灰", "M6", 0xB0, 0xA7, 0x82;
    "沙褐", "M9", 0xA5, 0x87, 0x67;
    "灰褐", "M12", 0x64, 0x47, 0x49;
    "肉桂", "M13", 0xD1, 0x90, 0x66;
    "赤陶", "M14", 0xC7, 0x73, 0x62;
];

/// Resolve a Chinese name or MARD code against the locked palette only.
pub fn resolve(mode: PaletteMode, input: &str) -> Option<[u8; 3]> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    mode.colors().iter().find_map(|color| {
        if color.name == trimmed || color.code.eq_ignore_ascii_case(trimmed) {
            Some(color.rgb)
        } else {
            None
        }
    })
}

pub fn format_list(mode: PaletteMode) -> String {
    let colors = mode.colors();
    let mut out = format!(
        "{} 色模式，共 {} 色。请使用下列中文名或 MARD 色号（色号大小写不敏感）：\n",
        mode.as_str(),
        colors.len()
    );
    for color in colors {
        let _ = writeln!(out, "{} {}", color.name, color.code);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn assert_unique(colors: &[Color]) {
        let mut names = HashSet::new();
        let mut codes = HashSet::new();
        for color in colors {
            assert!(
                names.insert(color.name),
                "duplicate name in palette: {}",
                color.name
            );
            let code = color.code.to_ascii_uppercase();
            assert!(
                codes.insert(code.clone()),
                "duplicate code in palette: {code}"
            );
        }
    }

    #[test]
    fn default_mode_is_221() {
        assert_eq!(PaletteMode::parse(None).unwrap(), PaletteMode::Colors221);
        assert_eq!(
            PaletteMode::parse(Some("")).unwrap(),
            PaletteMode::Colors221
        );
        assert_eq!(
            PaletteMode::parse(Some("221")).unwrap(),
            PaletteMode::Colors221
        );
        assert_eq!(
            PaletteMode::parse(Some("144")).unwrap(),
            PaletteMode::Colors144
        );
        assert_eq!(
            PaletteMode::parse(Some("24")).unwrap(),
            PaletteMode::Colors24
        );
        assert!(PaletteMode::parse(Some("96")).is_err());
        assert_eq!(PaletteMode::default(), PaletteMode::Colors221);
    }

    #[test]
    fn palette_lengths() {
        assert_eq!(COLORS_24.len(), 24);
        assert_eq!(COLORS_144.len(), 144);
        assert_eq!(COLORS_221.len(), 221);
    }

    #[test]
    fn names_and_codes_unique_within_each_mode() {
        assert_unique(COLORS_24);
        assert_unique(COLORS_144);
        assert_unique(COLORS_221);
    }

    #[test]
    fn colors_144_series_quotas() {
        let mut counts = HashMap::new();
        for color in COLORS_144 {
            let series = color
                .code
                .chars()
                .next()
                .expect("MARD codes start with a letter");
            *counts.entry(series).or_insert(0usize) += 1;
        }
        assert_eq!(counts.get(&'A'), Some(&18));
        assert_eq!(counts.get(&'B'), Some(&22));
        assert_eq!(counts.get(&'C'), Some(&20));
        assert_eq!(counts.get(&'D'), Some(&16));
        assert_eq!(counts.get(&'E'), Some(&16));
        assert_eq!(counts.get(&'F'), Some(&18));
        assert_eq!(counts.get(&'G'), Some(&14));
        assert_eq!(counts.get(&'H'), Some(&12));
        assert_eq!(counts.get(&'M'), Some(&8));
    }

    #[test]
    fn colors_221_series_counts() {
        let mut counts = HashMap::new();
        for color in COLORS_221 {
            let series = color
                .code
                .chars()
                .next()
                .expect("MARD codes start with a letter");
            *counts.entry(series).or_insert(0usize) += 1;
        }
        assert_eq!(counts.get(&'A'), Some(&26));
        assert_eq!(counts.get(&'B'), Some(&32));
        assert_eq!(counts.get(&'C'), Some(&29));
        assert_eq!(counts.get(&'D'), Some(&26));
        assert_eq!(counts.get(&'E'), Some(&24));
        assert_eq!(counts.get(&'F'), Some(&25));
        assert_eq!(counts.get(&'G'), Some(&21));
        assert_eq!(counts.get(&'H'), Some(&23));
        assert_eq!(counts.get(&'M'), Some(&15));
    }

    #[test]
    fn colors_221_reuses_144_names_for_shared_codes() {
        for color in COLORS_144 {
            let found = COLORS_221
                .iter()
                .find(|candidate| candidate.code == color.code)
                .unwrap_or_else(|| panic!("221 missing code {}", color.code));
            assert_eq!(found.name, color.name, "name mismatch for {}", color.code);
            assert_eq!(found.rgb, color.rgb, "rgb mismatch for {}", color.code);
        }
    }

    #[test]
    fn resolves_in_locked_mode_only() {
        assert_eq!(
            resolve(PaletteMode::Colors24, "红色"),
            Some([0xD8, 0x01, 0x27])
        );
        assert_eq!(
            resolve(PaletteMode::Colors24, " 白色 "),
            Some([0xFF, 0xFF, 0xFF])
        );
        assert_eq!(resolve(PaletteMode::Colors144, "红色"), None);
        assert_eq!(resolve(PaletteMode::Colors221, "红色"), None);
        assert_eq!(
            resolve(PaletteMode::Colors144, "正红"),
            Some([0xE7, 0x00, 0x2F])
        );
        assert_eq!(
            resolve(PaletteMode::Colors221, "正红"),
            Some([0xE7, 0x00, 0x2F])
        );
        assert_eq!(
            resolve(PaletteMode::Colors221, "雪白"),
            Some([0xFD, 0xFB, 0xFF])
        );
        assert_eq!(resolve(PaletteMode::Colors144, "雪白"), None);
        assert_eq!(resolve(PaletteMode::Colors24, "正红"), None);
        assert_eq!(resolve(PaletteMode::Colors24, "鹅黄"), None);
    }

    #[test]
    fn resolves_mard_code_in_mode() {
        assert_eq!(resolve(PaletteMode::Colors24, "H7"), Some([0, 0, 0]));
        assert_eq!(
            resolve(PaletteMode::Colors24, "h2"),
            Some([0xFF, 0xFF, 0xFF])
        );
        assert_eq!(
            resolve(PaletteMode::Colors144, "h2"),
            Some([0xFE, 0xFF, 0xFF])
        );
        assert_eq!(
            resolve(PaletteMode::Colors221, "h2"),
            Some([0xFE, 0xFF, 0xFF])
        );
        assert_eq!(
            resolve(PaletteMode::Colors221, "H1"),
            Some([0xFD, 0xFB, 0xFF])
        );
    }

    #[test]
    fn rejects_unknown_color() {
        assert_eq!(resolve(PaletteMode::Colors221, "彩虹"), None);
        assert_eq!(resolve(PaletteMode::Colors24, ""), None);
        assert_eq!(resolve(PaletteMode::Colors221, "#FF0000"), None);
    }

    #[test]
    fn format_list_mentions_mode_and_a_name() {
        let list_221 = format_list(PaletteMode::Colors221);
        assert!(list_221.contains("221 色模式"));
        assert!(list_221.contains("正红 F5"));
        assert!(list_221.contains("雪白 H1"));
        assert!(!list_221.contains("红色 F5"));
        let list_144 = format_list(PaletteMode::Colors144);
        assert!(list_144.contains("144 色模式"));
        assert!(list_144.contains("正红 F5"));
        assert!(!list_144.contains("红色 F5"));
        let list_24 = format_list(PaletteMode::Colors24);
        assert!(list_24.contains("24 色模式"));
        assert!(list_24.contains("红色 F5"));
    }
}
