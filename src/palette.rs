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

/// 24-color subset. Each code uses the same name and RGB as the 221 palette.
pub const COLORS_24: &[Color] = colors![
    "雪白", "H2", 0xFE, 0xFF, 0xFF;
    "浅灰", "H3", 0xB6, 0xB1, 0xBA;
    "中灰", "H4", 0x89, 0x85, 0x8C;
    "纯黑", "H7", 0x00, 0x00, 0x00;
    "明黄", "A4", 0xFB, 0xED, 0x56;
    "橘黄", "A6", 0xFE, 0xAC, 0x4C;
    "橙黄", "A7", 0xFE, 0x8B, 0x4C;
    "中国红", "F5", 0xE7, 0x00, 0x2F;
    "正红", "F2", 0xFC, 0x3D, 0x46;
    "玫红", "E6", 0xF1, 0x3D, 0x74;
    "肉粉", "E11", 0xFC, 0xDD, 0xD2;
    "米粉", "E16", 0xFF, 0xF3, 0xEB;
    "暗棕", "G8", 0x75, 0x38, 0x32;
    "赤棕", "G13", 0xB7, 0x71, 0x4A;
    "碧绿", "B5", 0x35, 0xE3, 0x52;
    "森林绿", "B8", 0x1C, 0x9C, 0x4F;
    "亮绿", "B2", 0x63, 0xF3, 0x47;
    "湖蓝", "C4", 0x41, 0xCC, 0xFF;
    "深蓝", "C8", 0x0F, 0x54, 0xC0;
    "海军蓝", "C12", 0x1C, 0x33, 0x4D;
    "青色", "C11", 0x28, 0xDD, 0xDE;
    "暗紫", "D7", 0x88, 0x54, 0xB3;
    "品红", "D13", 0xB9, 0x00, 0x95;
    "深棕", "F11", 0x5A, 0x21, 0x21;
];

/// 144-color subset. Each code uses the same name and RGB as the 221 palette.
pub const COLORS_144: &[Color] = colors![
    "淡鹅黄", "A1", 0xFA, 0xF4, 0xC8;
    "柠檬黄", "A3", 0xFE, 0xFF, 0x8B;
    "明黄", "A4", 0xFB, 0xED, 0x56;
    "金黄", "A5", 0xF4, 0xD7, 0x38;
    "橘黄", "A6", 0xFE, 0xAC, 0x4C;
    "橙黄", "A7", 0xFE, 0x8B, 0x4C;
    "向日葵黄", "A8", 0xFF, 0xDA, 0x45;
    "杏橙", "A9", 0xFF, 0x99, 0x5B;
    "橘橙", "A10", 0xF7, 0x7C, 0x31;
    "浅杏", "A11", 0xFF, 0xDD, 0x99;
    "珊瑚橙", "A12", 0xFE, 0x9F, 0x72;
    "琥珀橙", "A13", 0xFF, 0xC3, 0x65;
    "朱红", "A14", 0xFD, 0x54, 0x3D;
    "鹅黄", "A15", 0xFF, 0xF3, 0x65;
    "芥末黄", "A17", 0xFF, 0xE3, 0x6E;
    "麦黄", "A18", 0xFE, 0xBE, 0x7D;
    "鲑鱼粉", "A19", 0xFD, 0x7C, 0x72;
    "向日葵", "A26", 0xFF, 0xC8, 0x30;
    "荧光黄绿", "B1", 0xE6, 0xEE, 0x31;
    "亮绿", "B2", 0x63, 0xF3, 0x47;
    "薄荷绿", "B3", 0x9E, 0xF7, 0x80;
    "翠绿", "B4", 0x5D, 0xE0, 0x35;
    "碧绿", "B5", 0x35, 0xE3, 0x52;
    "青碧", "B6", 0x65, 0xE2, 0xA6;
    "松石绿", "B7", 0x3D, 0xAF, 0x80;
    "森林绿", "B8", 0x1C, 0x9C, 0x4F;
    "墨绿", "B9", 0x27, 0x52, 0x3A;
    "水绿", "B10", 0x95, 0xD3, 0xC2;
    "橄榄绿", "B11", 0x5D, 0x72, 0x2A;
    "翡翠绿", "B12", 0x16, 0x6F, 0x41;
    "嫩绿", "B13", 0xCA, 0xEB, 0x7B;
    "草绿", "B14", 0xAD, 0xE9, 0x46;
    "深橄榄", "B15", 0x2E, 0x51, 0x32;
    "苔绿", "B17", 0x9B, 0xB1, 0x3A;
    "孔雀绿", "B19", 0x24, 0xB8, 0x8C;
    "深青", "B21", 0x15, 0x6A, 0x6B;
    "暗青", "B22", 0x0B, 0x3C, 0x43;
    "暗橄榄", "B23", 0x30, 0x3A, 0x21;
    "灰绿", "B25", 0x4E, 0x84, 0x6D;
    "橄榄", "B32", 0x9C, 0xAB, 0x5A;
    "冰蓝", "C2", 0xA9, 0xF9, 0xFC;
    "天蓝", "C3", 0xA0, 0xE2, 0xFB;
    "湖蓝", "C4", 0x41, 0xCC, 0xFF;
    "亮蓝", "C5", 0x01, 0xAC, 0xEB;
    "钴蓝", "C6", 0x50, 0xAA, 0xF0;
    "宝蓝", "C7", 0x36, 0x77, 0xD2;
    "深蓝", "C8", 0x0F, 0x54, 0xC0;
    "藏蓝", "C9", 0x32, 0x4B, 0xCA;
    "青色", "C11", 0x28, 0xDD, 0xDE;
    "海军蓝", "C12", 0x1C, 0x33, 0x4D;
    "雾蓝", "C13", 0xCD, 0xE8, 0xFF;
    "湖青", "C15", 0x22, 0xC4, 0xC6;
    "湛蓝", "C16", 0x15, 0x57, 0xA8;
    "暗蓝", "C18", 0x1D, 0x33, 0x44;
    "深青蓝", "C19", 0x18, 0x87, 0xA2;
    "海蓝", "C20", 0x17, 0x6D, 0xAF;
    "灰蓝", "C22", 0x67, 0xB4, 0xBE;
    "晴蓝", "C24", 0x7C, 0xC4, 0xFF;
    "碧蓝", "C26", 0x3C, 0xAE, 0xD8;
    "靛蓝", "C29", 0x34, 0x48, 0x8E;
    "淡紫", "D1", 0xAE, 0xB4, 0xF2;
    "蓝紫", "D2", 0x85, 0x8E, 0xDD;
    "深蓝紫", "D3", 0x2F, 0x54, 0xAF;
    "暗靛", "D4", 0x18, 0x2A, 0x84;
    "洋紫", "D5", 0xB8, 0x43, 0xC5;
    "浅紫", "D6", 0xAC, 0x7B, 0xDE;
    "暗紫", "D7", 0x88, 0x54, 0xB3;
    "薰衣草", "D8", 0xE2, 0xD3, 0xFF;
    "暗紫罗兰", "D10", 0x36, 0x18, 0x51;
    "粉紫", "D12", 0xDE, 0x9A, 0xD4;
    "品红", "D13", 0xB9, 0x00, 0x95;
    "紫罗兰", "D14", 0x8B, 0x27, 0x9B;
    "深紫蓝", "D15", 0x2F, 0x1F, 0x90;
    "葡萄紫", "D18", 0xA4, 0x5E, 0xC7;
    "紫红", "D21", 0x9A, 0x00, 0x9B;
    "群青紫", "D25", 0x49, 0x4F, 0xC7;
    "浅粉", "E1", 0xFD, 0xD3, 0xCC;
    "玫瑰粉", "E4", 0xE8, 0x64, 0x9E;
    "洋红", "E5", 0xF5, 0x51, 0xA2;
    "玫红", "E6", 0xF1, 0x3D, 0x74;
    "酒红", "E7", 0xC6, 0x34, 0x78;
    "紫粉", "E9", 0xE9, 0x70, 0xCC;
    "深紫红", "E10", 0xD3, 0x37, 0x93;
    "肉粉", "E11", 0xFC, 0xDD, 0xD2;
    "樱花粉", "E12", 0xF7, 0x8F, 0xC3;
    "暗紫红", "E13", 0xB5, 0x00, 0x6D;
    "蜜桃粉", "E14", 0xFF, 0xD1, 0xBA;
    "米粉", "E16", 0xFF, 0xF3, 0xEB;
    "芙蓉粉", "E18", 0xFF, 0xC7, 0xDB;
    "玫瑰灰", "E21", 0xBD, 0x9D, 0xA1;
    "紫灰", "E22", 0xB7, 0x85, 0xA1;
    "紫藤", "E24", 0xE1, 0xBC, 0xE8;
    "珊瑚红", "F1", 0xFD, 0x95, 0x7B;
    "正红", "F2", 0xFC, 0x3D, 0x46;
    "火红", "F3", 0xF7, 0x49, 0x41;
    "大红", "F4", 0xFC, 0x28, 0x3C;
    "中国红", "F5", 0xE7, 0x00, 0x2F;
    "栗棕", "F6", 0x94, 0x36, 0x30;
    "暗酒红", "F7", 0x97, 0x19, 0x37;
    "深红", "F8", 0xBC, 0x00, 0x28;
    "玫瑰红", "F9", 0xE2, 0x67, 0x7A;
    "棕褐", "F10", 0x8A, 0x45, 0x26;
    "深棕", "F11", 0x5A, 0x21, 0x21;
    "桃红", "F12", 0xFD, 0x4E, 0x6A;
    "橘红", "F13", 0xF3, 0x57, 0x44;
    "浅红", "F14", 0xFF, 0xA9, 0xAD;
    "暗红", "F15", 0xD3, 0x00, 0x22;
    "赤褐", "F17", 0xE6, 0x9C, 0x79;
    "砖红", "F19", 0xC1, 0x44, 0x4A;
    "朱砂红", "F25", 0xE5, 0x4B, 0x4F;
    "浅肤色", "G1", 0xFF, 0xE2, 0xCE;
    "肤色", "G2", 0xFF, 0xC4, 0xAA;
    "自然肤", "G3", 0xF4, 0xC3, 0xA5;
    "小麦色", "G4", 0xE1, 0xB3, 0x83;
    "深琥珀", "G6", 0xE9, 0x9C, 0x17;
    "巧克力", "G7", 0x9D, 0x5B, 0x3E;
    "暗棕", "G8", 0x75, 0x38, 0x32;
    "焦橙", "G10", 0xD9, 0x8C, 0x39;
    "赤棕", "G13", 0xB7, 0x71, 0x4A;
    "灰棕", "G14", 0x8D, 0x61, 0x4C;
    "杏色", "G16", 0xF2, 0xD9, 0xBA;
    "咖啡", "G17", 0x78, 0x52, 0x4B;
    "橙棕", "G19", 0xE0, 0x79, 0x35;
    "铁锈红", "G20", 0xA9, 0x40, 0x23;
    "雪白", "H2", 0xFE, 0xFF, 0xFF;
    "浅灰", "H3", 0xB6, 0xB1, 0xBA;
    "中灰", "H4", 0x89, 0x85, 0x8C;
    "深灰", "H5", 0x48, 0x46, 0x4E;
    "炭灰", "H6", 0x2F, 0x2B, 0x2F;
    "纯黑", "H7", 0x00, 0x00, 0x00;
    "银灰", "H9", 0xED, 0xED, 0xED;
    "象牙白", "H12", 0xFF, 0xF5, 0xED;
    "米白", "H13", 0xF5, 0xEC, 0xD2;
    "雾灰", "H15", 0x98, 0xA6, 0xA8;
    "墨黑", "H16", 0x1D, 0x14, 0x14;
    "钢灰", "H20", 0x94, 0x9F, 0xA3;
    "莫兰迪绿", "M1", 0xBC, 0xC6, 0xB8;
    "莫兰迪灰绿", "M2", 0x8A, 0xA3, 0x86;
    "莫兰迪米", "M4", 0xE3, 0xD2, 0xBC;
    "灰卡其", "M6", 0xB0, 0xA7, 0x82;
    "莫兰迪灰棕", "M9", 0xA5, 0x87, 0x67;
    "深灰紫", "M12", 0x64, 0x47, 0x49;
    "灰橙", "M13", 0xD1, 0x90, 0x66;
    "灰红", "M14", 0xC7, 0x73, 0x62;
];

/// Resolve a Chinese name or MARD code against the locked palette only.
pub fn resolve(mode: PaletteMode, input: &str) -> Option<[u8; 3]> {
    resolve_color(mode, input).map(|color| color.rgb)
}

/// Resolve identity as well as RGB; different bead codes may share an RGB value.
pub fn resolve_color(mode: PaletteMode, input: &str) -> Option<&'static Color> {
    let input = input.trim();
    mode.colors()
        .iter()
        .find(|color| color.name == input || color.code.eq_ignore_ascii_case(input))
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
    fn shared_codes_use_the_same_name_and_rgb() {
        for subset in [COLORS_24, COLORS_144] {
            for color in subset {
                let found = COLORS_221
                    .iter()
                    .find(|candidate| candidate.code == color.code)
                    .unwrap_or_else(|| panic!("221 missing code {}", color.code));
                assert_eq!(found.name, color.name, "name mismatch for {}", color.code);
                assert_eq!(found.rgb, color.rgb, "rgb mismatch for {}", color.code);
            }
        }
    }

    #[test]
    fn resolves_in_locked_mode_only() {
        assert_eq!(
            resolve(PaletteMode::Colors24, "中国红"),
            Some([0xE7, 0x00, 0x2F])
        );
        assert_eq!(
            resolve(PaletteMode::Colors24, " 雪白 "),
            Some([0xFE, 0xFF, 0xFF])
        );
        assert_eq!(
            resolve(PaletteMode::Colors24, "中国红"),
            resolve(PaletteMode::Colors221, "中国红")
        );
        assert_eq!(
            resolve(PaletteMode::Colors144, "正红"),
            Some([0xFC, 0x3D, 0x46])
        );
        assert_eq!(
            resolve(PaletteMode::Colors221, "正红"),
            Some([0xFC, 0x3D, 0x46])
        );
        assert_eq!(
            resolve(PaletteMode::Colors24, "正红"),
            Some([0xFC, 0x3D, 0x46])
        );
        assert_eq!(
            resolve(PaletteMode::Colors221, "纯白"),
            Some([0xFD, 0xFB, 0xFF])
        );
        assert_eq!(resolve(PaletteMode::Colors144, "纯白"), None);
        assert_eq!(resolve(PaletteMode::Colors24, "纯白"), None);
        assert_eq!(resolve(PaletteMode::Colors24, "鹅黄"), None);
    }

    #[test]
    fn resolves_mard_code_in_mode() {
        assert_eq!(resolve(PaletteMode::Colors24, "H7"), Some([0, 0, 0]));
        assert_eq!(
            resolve(PaletteMode::Colors24, "h2"),
            Some([0xFE, 0xFF, 0xFF])
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
        assert!(list_221.contains("正红 F2"));
        assert!(list_221.contains("纯白 H1"));
        assert!(list_221.contains("中国红 F5"));
        let list_144 = format_list(PaletteMode::Colors144);
        assert!(list_144.contains("144 色模式"));
        assert!(list_144.contains("正红 F2"));
        assert!(list_144.contains("中国红 F5"));
        let list_24 = format_list(PaletteMode::Colors24);
        assert!(list_24.contains("24 色模式"));
        assert!(list_24.contains("中国红 F5"));
        assert!(list_24.contains("雪白 H2"));
    }
}
