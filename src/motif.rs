//! Embedded v1 motif library.
//!
//! A motif is a fixed set of occupied cells inside a bounding box. Placement
//! is top-left anchored and paints only those cells; gaps in the box stay
//! untouched. This is not a cell-center geometry test.

/// Occupied cells of one motif, relative to the top-left of its bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotifPoints {
    pub width: u32,
    pub height: u32,
    /// Row-major occupied cells. Each coordinate is inside `width` × `height`.
    pub cells: &'static [(u8, u8)],
}

/// 小爱心 and 扑克-红心 are two names for this one bitmap.
const HEART: &[(u8, u8)] = &[
    (1, 0),
    (3, 0),
    (0, 1),
    (1, 1),
    (2, 1),
    (3, 1),
    (4, 1),
    (1, 2),
    (2, 2),
    (3, 2),
    (2, 3),
];

/// 箭头-左 is 4×3, six cells.
///
/// Issue #12 wrote the rows as `. # . . / # # # # / . . # . .`. The stated
/// size is 4×3, but the last row has five characters. The fifth character is
/// an empty cell past that width. Dropping it leaves `.#../####/..#.` and
/// keeps every occupied coordinate: (1,0), (0,1), (1,1), (2,1), (3,1), (2,2).
/// That bitmap is not the left-right mirror of 箭头-右 (`..#./####/..#.`),
/// which would put the lower cell at (1,2) instead of (2,2).
const ARROW_LEFT: &[(u8, u8)] = &[(1, 0), (0, 1), (1, 1), (2, 1), (3, 1), (2, 2)];

const DICE: [&[(u8, u8)]; 6] = [
    &[(1, 1)],
    &[(0, 0), (2, 2)],
    &[(0, 0), (1, 1), (2, 2)],
    &[(0, 0), (2, 0), (0, 2), (2, 2)],
    &[(0, 0), (2, 0), (1, 1), (0, 2), (2, 2)],
    &[(0, 0), (2, 0), (0, 1), (2, 1), (0, 2), (2, 2)],
];

struct Entry {
    name: &'static str,
    width: u8,
    height: u8,
    cells: &'static [(u8, u8)],
    dice: bool,
}

const ENTRIES: &[Entry] = &[
    Entry {
        name: "十字",
        width: 3,
        height: 3,
        cells: &[(1, 0), (0, 1), (1, 1), (2, 1), (1, 2)],
        dice: false,
    },
    Entry {
        name: "叉",
        width: 3,
        height: 3,
        cells: &[(0, 0), (2, 0), (1, 1), (0, 2), (2, 2)],
        dice: false,
    },
    Entry {
        name: "品字-缺左上",
        width: 2,
        height: 2,
        cells: &[(1, 0), (0, 1), (1, 1)],
        dice: false,
    },
    Entry {
        name: "品字-缺右上",
        width: 2,
        height: 2,
        cells: &[(0, 0), (0, 1), (1, 1)],
        dice: false,
    },
    Entry {
        name: "斜线-短",
        width: 2,
        height: 2,
        cells: &[(0, 0), (1, 1)],
        dice: false,
    },
    Entry {
        name: "斜线-长",
        width: 3,
        height: 3,
        cells: &[(0, 0), (1, 1), (2, 2)],
        dice: false,
    },
    Entry {
        name: "四点",
        width: 3,
        height: 3,
        cells: &[(0, 0), (2, 0), (0, 2), (2, 2)],
        dice: false,
    },
    Entry {
        name: "T字-下",
        width: 3,
        height: 3,
        cells: &[(0, 0), (1, 0), (2, 0), (1, 1), (1, 2)],
        dice: false,
    },
    Entry {
        name: "T字-上",
        width: 3,
        height: 3,
        cells: &[(1, 0), (1, 1), (0, 2), (1, 2), (2, 2)],
        dice: false,
    },
    Entry {
        name: "T字-右",
        width: 3,
        height: 3,
        cells: &[(0, 0), (0, 1), (1, 1), (2, 1), (0, 2)],
        dice: false,
    },
    Entry {
        name: "T字-左",
        width: 3,
        height: 3,
        cells: &[(2, 0), (0, 1), (1, 1), (2, 1), (2, 2)],
        dice: false,
    },
    Entry {
        name: "拐角-左上",
        width: 2,
        height: 2,
        cells: &[(0, 0), (1, 0), (0, 1)],
        dice: false,
    },
    Entry {
        name: "拐角-右上",
        width: 2,
        height: 2,
        cells: &[(0, 0), (1, 0), (1, 1)],
        dice: false,
    },
    Entry {
        name: "拐角-左下",
        width: 2,
        height: 2,
        cells: &[(0, 0), (0, 1), (1, 1)],
        dice: false,
    },
    Entry {
        name: "拐角-右下",
        width: 2,
        height: 2,
        cells: &[(1, 0), (0, 1), (1, 1)],
        dice: false,
    },
    Entry {
        name: "箭头-上",
        width: 3,
        height: 4,
        cells: &[(1, 0), (0, 1), (1, 1), (2, 1), (1, 2), (1, 3)],
        dice: false,
    },
    Entry {
        name: "箭头-下",
        width: 3,
        height: 4,
        cells: &[(1, 0), (1, 1), (0, 2), (1, 2), (2, 2), (1, 3)],
        dice: false,
    },
    Entry {
        name: "箭头-左",
        width: 4,
        height: 3,
        cells: ARROW_LEFT,
        dice: false,
    },
    Entry {
        name: "箭头-右",
        width: 4,
        height: 3,
        cells: &[(2, 0), (0, 1), (1, 1), (2, 1), (3, 1), (2, 2)],
        dice: false,
    },
    Entry {
        name: "对勾",
        width: 3,
        height: 3,
        cells: &[(2, 0), (1, 1), (0, 2), (1, 2)],
        dice: false,
    },
    Entry {
        name: "感叹号",
        width: 1,
        height: 5,
        cells: &[(0, 0), (0, 1), (0, 2), (0, 4)],
        dice: false,
    },
    Entry {
        name: "四芒闪光",
        width: 3,
        height: 3,
        cells: &[(1, 0), (0, 1), (2, 1), (1, 2)],
        dice: false,
    },
    Entry {
        name: "五角星",
        width: 5,
        height: 5,
        cells: &[
            (2, 0),
            (1, 1),
            (2, 1),
            (3, 1),
            (0, 2),
            (1, 2),
            (2, 2),
            (3, 2),
            (4, 2),
            (1, 3),
            (3, 3),
            (0, 4),
            (4, 4),
        ],
        dice: false,
    },
    Entry {
        name: "闪电",
        width: 3,
        height: 5,
        cells: &[(2, 0), (1, 1), (2, 1), (1, 2), (0, 3), (1, 3), (0, 4)],
        dice: false,
    },
    Entry {
        name: "小爱心",
        width: 5,
        height: 4,
        cells: HEART,
        dice: false,
    },
    Entry {
        name: "水滴",
        width: 3,
        height: 4,
        cells: &[(2, 0), (1, 1), (2, 1), (0, 2), (1, 2), (2, 2), (1, 3)],
        dice: false,
    },
    Entry {
        name: "小花",
        width: 5,
        height: 5,
        cells: &[
            (1, 0),
            (3, 0),
            (0, 1),
            (1, 1),
            (3, 1),
            (4, 1),
            (2, 2),
            (0, 3),
            (1, 3),
            (3, 3),
            (4, 3),
            (1, 4),
            (3, 4),
        ],
        dice: false,
    },
    Entry {
        name: "骰子面",
        width: 3,
        height: 3,
        cells: &[],
        dice: true,
    },
    Entry {
        name: "扑克-红心",
        width: 5,
        height: 4,
        cells: HEART,
        dice: false,
    },
    Entry {
        name: "扑克-方块",
        width: 5,
        height: 5,
        cells: &[
            (2, 0),
            (1, 1),
            (2, 1),
            (3, 1),
            (0, 2),
            (1, 2),
            (2, 2),
            (3, 2),
            (4, 2),
            (1, 3),
            (2, 3),
            (3, 3),
            (2, 4),
        ],
        dice: false,
    },
    Entry {
        name: "扑克-梅花",
        width: 5,
        height: 5,
        cells: &[
            (1, 0),
            (3, 0),
            (0, 1),
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 1),
            (1, 2),
            (2, 2),
            (3, 2),
            (2, 3),
            (2, 4),
        ],
        dice: false,
    },
    Entry {
        name: "扑克-黑桃",
        width: 5,
        height: 5,
        cells: &[
            (2, 0),
            (1, 1),
            (2, 1),
            (3, 1),
            (0, 2),
            (1, 2),
            (2, 2),
            (3, 2),
            (4, 2),
            (0, 3),
            (2, 3),
            (4, 3),
            (2, 4),
        ],
        dice: false,
    },
];

/// Every v1 motif name, in catalog order. 骰子面 is one name.
#[cfg(test)]
fn motif_names() -> impl Iterator<Item = &'static str> {
    ENTRIES.iter().map(|entry| entry.name)
}

/// Occupied cells for `name`. `face` is required for 骰子面 and rejected otherwise.
pub fn motif_points(name: &str, face: Option<u32>) -> Result<MotifPoints, String> {
    let entry = ENTRIES
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| format!("未知小图形「{name}」。请使用 draw_motif 工具描述中的图形名。"))?;
    if entry.dice {
        let face = face.ok_or_else(|| "骰子面必须指定 face，取 1 到 6。".to_string())?;
        if !(1..=6).contains(&face) {
            return Err(format!(
                "骰子面的 face 必须是 1 到 6 的整数，当前为 {face}。"
            ));
        }
        return Ok(MotifPoints {
            width: u32::from(entry.width),
            height: u32::from(entry.height),
            cells: DICE[(face - 1) as usize],
        });
    }
    if face.is_some() {
        return Err(format!(
            "只有「骰子面」接受 face，图形「{name}」不能指定点数。"
        ));
    }
    Ok(MotifPoints {
        width: u32::from(entry.width),
        height: u32::from(entry.height),
        cells: entry.cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Spec matrices. `#` is occupied, `.` is a gap inside the box.
    /// Rows are separated by `/` and contain no spaces.
    ///
    /// 箭头-左 is the one resolved reading of issue #12. The issue text is
    /// `. # . . / # # # # / . . # . .` under a 4×3, 6-cell heading. The last
    /// row's fifth character is an empty cell beyond the declared width.
    /// The locked bitmap is `.#../####/..#.` so the six occupied coordinates
    /// stay (1,0), (0,1), (1,1), (2,1), (3,1), (2,2). It is intentionally not
    /// rewritten into the conventional left arrow `.#../####/.#..`, which is
    /// the left-right mirror of 箭头-右 and would move the lower cell.
    const GOLDEN: &[(&str, u32, u32, &str)] = &[
        ("十字", 3, 3, ".#./###/.#."),
        ("叉", 3, 3, "#.#/.#./#.#"),
        ("品字-缺左上", 2, 2, ".#/##"),
        ("品字-缺右上", 2, 2, "#./##"),
        ("斜线-短", 2, 2, "#./.#"),
        ("斜线-长", 3, 3, "#../.#./..#"),
        ("四点", 3, 3, "#.#/.../#.#"),
        ("T字-下", 3, 3, "###/.#./.#."),
        ("T字-上", 3, 3, ".#./.#./###"),
        ("T字-右", 3, 3, "#../###/#.."),
        ("T字-左", 3, 3, "..#/###/..#"),
        ("拐角-左上", 2, 2, "##/#."),
        ("拐角-右上", 2, 2, "##/.#"),
        ("拐角-左下", 2, 2, "#./##"),
        ("拐角-右下", 2, 2, ".#/##"),
        ("箭头-上", 3, 4, ".#./###/.#./.#."),
        ("箭头-下", 3, 4, ".#./.#./###/.#."),
        ("箭头-左", 4, 3, ".#../####/..#."),
        ("箭头-右", 4, 3, "..#./####/..#."),
        ("对勾", 3, 3, "..#/.#./##."),
        ("感叹号", 1, 5, "#/#/#/./#"),
        ("四芒闪光", 3, 3, ".#./#.#/.#."),
        ("五角星", 5, 5, "..#../.###./#####/.#.#./#...#"),
        ("闪电", 3, 5, "..#/.##/.#./##./#.."),
        ("小爱心", 5, 4, ".#.#./#####/.###./..#.."),
        ("水滴", 3, 4, "..#/.##/###/.#."),
        ("小花", 5, 5, ".#.#./##.##/..#../##.##/.#.#."),
        ("扑克-红心", 5, 4, ".#.#./#####/.###./..#.."),
        ("扑克-方块", 5, 5, "..#../.###./#####/.###./..#.."),
        ("扑克-梅花", 5, 5, ".#.#./#####/.###./..#../..#.."),
        ("扑克-黑桃", 5, 5, "..#../.###./#####/#.#.#/..#.."),
    ];

    const DICE_GOLDEN: &[&str] = &[
        ".../.#./...",
        "#../.../..#",
        "#../.#./..#",
        "#.#/.../#.#",
        "#.#/.#./#.#",
        "#.#/#.#/#.#",
    ];

    const SPEC_COUNTS: &[(&str, usize)] = &[
        ("十字", 5),
        ("叉", 5),
        ("品字-缺左上", 3),
        ("品字-缺右上", 3),
        ("斜线-短", 2),
        ("斜线-长", 3),
        ("四点", 4),
        ("T字-下", 5),
        ("T字-上", 5),
        ("T字-右", 5),
        ("T字-左", 5),
        ("拐角-左上", 3),
        ("拐角-右上", 3),
        ("拐角-左下", 3),
        ("拐角-右下", 3),
        ("箭头-上", 6),
        ("箭头-下", 6),
        ("箭头-左", 6),
        ("箭头-右", 6),
        ("对勾", 4),
        ("感叹号", 4),
        ("四芒闪光", 4),
        ("五角星", 13),
        ("闪电", 7),
        ("小爱心", 11),
        ("水滴", 7),
        ("小花", 13),
        ("扑克-红心", 11),
        ("扑克-方块", 13),
        ("扑克-梅花", 12),
        ("扑克-黑桃", 13),
    ];

    fn parse_matrix(spec: &str) -> (u32, u32, Vec<(u8, u8)>) {
        let rows: Vec<&str> = spec.split('/').collect();
        let height = rows.len();
        let width = rows[0].len();
        assert!(
            rows.iter().all(|row| row.len() == width),
            "ragged matrix {spec}"
        );
        let mut cells = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                match ch {
                    '#' => cells.push((x as u8, y as u8)),
                    '.' => {}
                    other => panic!("bad cell {other} in {spec}"),
                }
            }
        }
        (width as u32, height as u32, cells)
    }

    fn cell_set(cells: &[(u8, u8)]) -> HashSet<(u8, u8)> {
        cells.iter().copied().collect()
    }

    #[test]
    fn every_catalog_bitmap_matches_its_matrix() {
        assert_eq!(GOLDEN.len(), 31);
        assert_eq!(SPEC_COUNTS.len(), 31);
        assert_eq!(motif_names().count(), 32, "31 fixed names plus 骰子面");
        let names: Vec<_> = motif_names().collect();
        assert_eq!(names.iter().collect::<HashSet<_>>().len(), names.len());

        for (&(name, width, height, matrix), &(count_name, count)) in GOLDEN.iter().zip(SPEC_COUNTS)
        {
            assert_eq!(name, count_name);
            let (parsed_w, parsed_h, parsed) = parse_matrix(matrix);
            assert_eq!((parsed_w, parsed_h), (width, height), "{name}");
            assert_eq!(parsed.len(), count, "{name} matrix count");
            assert_eq!(cell_set(&parsed).len(), parsed.len(), "{name} duplicates");
            let points = motif_points(name, None).unwrap();
            assert_eq!((points.width, points.height), (width, height), "{name}");
            assert_eq!(points.cells, parsed.as_slice(), "{name}");
            assert!(names.contains(&name), "{name} missing from the library");
        }

        let heart = motif_points("小爱心", None).unwrap();
        let suit = motif_points("扑克-红心", None).unwrap();
        assert_eq!(heart, suit);
        assert_eq!(heart.cells, HEART);
    }

    #[test]
    fn arrow_left_keeps_the_literal_cells_and_is_not_mirrored() {
        let points = motif_points("箭头-左", None).unwrap();
        assert_eq!((points.width, points.height), (4, 3));
        let cells = cell_set(points.cells);
        assert_eq!(
            cells,
            HashSet::from([(1, 0), (0, 1), (1, 1), (2, 1), (3, 1), (2, 2)])
        );
        // Conventional mirror of 箭头-右. The lower tip would sit at x=1.
        let mirrored = parse_matrix(".#../####/.#..").2;
        assert_ne!(cells, cell_set(&mirrored));
        assert!(!cells.contains(&(1, 2)));
        assert!(cells.contains(&(2, 2)));
    }

    #[test]
    fn symmetric_motifs_match_their_mirrors_and_quarter_turns() {
        for name in ["十字", "叉", "四点", "四芒闪光", "扑克-方块"] {
            let points = motif_points(name, None).unwrap();
            assert_eq!(points.width, points.height, "{name} is square");
            let side = u8::try_from(points.width).unwrap();
            let cells = cell_set(points.cells);
            for &(x, y) in points.cells {
                assert!(
                    cells.contains(&(side - 1 - x, y)),
                    "{name} lost horizontal mirror of ({x},{y})"
                );
                assert!(
                    cells.contains(&(x, side - 1 - y)),
                    "{name} lost vertical mirror of ({x},{y})"
                );
                assert!(
                    cells.contains(&(side - 1 - y, x)),
                    "{name} lost quarter turn of ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn dice_faces_are_distinct_and_count_matches_the_face() {
        let mut seen = HashSet::new();
        for face in 1..=6 {
            let points = motif_points("骰子面", Some(face)).unwrap();
            assert_eq!((points.width, points.height), (3, 3));
            let (width, height, parsed) = parse_matrix(DICE_GOLDEN[(face - 1) as usize]);
            assert_eq!((width, height), (3, 3));
            assert_eq!(points.cells, parsed.as_slice(), "face {face}");
            assert_eq!(points.cells.len(), face as usize, "face {face} count");
            let mut key: Vec<(u8, u8)> = points.cells.to_vec();
            key.sort_unstable();
            assert!(seen.insert(key), "face {face} repeats");
        }
        assert_eq!(seen.len(), 6);
        let missing = motif_points("骰子面", None).unwrap_err();
        assert!(missing.contains("必须指定 face"), "{missing}");
        for face in [0, 7, 8] {
            let err = motif_points("骰子面", Some(face)).unwrap_err();
            assert!(err.contains("1 到 6"), "{err}");
            assert!(err.contains(&face.to_string()), "{err}");
        }
    }

    #[test]
    fn unknown_names_and_stray_faces_are_rejected() {
        let unknown = motif_points("大爱心", None).unwrap_err();
        assert!(unknown.contains("未知小图形"), "{unknown}");
        assert!(unknown.contains("大爱心"), "{unknown}");
        for name in motif_names().filter(|name| *name != "骰子面") {
            let err = motif_points(name, Some(1)).unwrap_err();
            assert!(err.contains("只有「骰子面」接受 face"), "{name}: {err}");
            assert!(err.contains(name), "{err}");
        }
        assert!(motif_points("十字 ", None).is_err());
        assert!(motif_points("", None).is_err());
    }
}
