//! Deterministic bead charts without system fonts or RGB-to-code guessing.
use std::collections::BTreeMap;
use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

use crate::{canvas::Canvas, palette};

const CELL: u32 = 40;
const LEFT: u32 = 48;
const TOP: u32 = 88;

pub fn counts(canvas: &Canvas) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for code in canvas.codes().iter().flatten() {
        *counts.entry(*code).or_default() += 1;
    }
    counts
}

pub fn encode_png(canvas: &Canvas) -> Result<Vec<u8>, image::ImageError> {
    let counts = counts(canvas);
    let width = (LEFT + canvas.width() * CELL + 24).max(480);
    let legend_y = TOP + canvas.height() * CELL + 32;
    let height = legend_y + (counts.len() as u32).div_ceil(4) * 42 + 24;
    let mut image = RgbImage::from_pixel(width, height, Rgb([255; 3]));
    text(
        &mut image,
        24,
        16,
        &format!("MARD {}", canvas.mode().as_str()),
        [0; 3],
        2,
    );
    text(
        &mut image,
        24,
        42,
        &format!(
            "{} X {}   TOTAL {}",
            canvas.width(),
            canvas.height(),
            counts.values().sum::<usize>()
        ),
        [0; 3],
        2,
    );
    for x in 0..canvas.width() {
        centered(
            &mut image,
            LEFT + x * CELL + CELL / 2,
            TOP - 22,
            &(x + 1).to_string(),
            [0; 3],
            1,
        );
    }
    for y in 0..canvas.height() {
        centered(
            &mut image,
            LEFT / 2,
            TOP + y * CELL + 16,
            &(y + 1).to_string(),
            [0; 3],
            1,
        );
        for x in 0..canvas.width() {
            if let Some(code) = canvas.codes()[(y * canvas.width() + x) as usize] {
                let color = palette::resolve(canvas.mode(), code)
                    .expect("canvas stores canonical palette codes");
                rect(
                    &mut image,
                    LEFT + x * CELL,
                    TOP + y * CELL,
                    CELL,
                    CELL,
                    color,
                );
                centered(
                    &mut image,
                    LEFT + x * CELL + CELL / 2,
                    TOP + y * CELL + 13,
                    code,
                    foreground(color),
                    2,
                );
            }
        }
    }
    for x in 0..=canvas.width() {
        rect(
            &mut image,
            LEFT + x * CELL,
            TOP,
            if x % 5 == 0 { 2 } else { 1 },
            canvas.height() * CELL + 1,
            [110; 3],
        );
    }
    for y in 0..=canvas.height() {
        rect(
            &mut image,
            LEFT,
            TOP + y * CELL,
            canvas.width() * CELL + 1,
            if y % 5 == 0 { 2 } else { 1 },
            [110; 3],
        );
    }
    let block = (width - 48) / 4;
    // Follow palette order (A1, A2, ..., A10), rather than lexical code order.
    for (i, color) in canvas
        .mode()
        .colors()
        .iter()
        .filter(|c| counts.contains_key(c.code))
        .enumerate()
    {
        let x = 24 + (i as u32 % 4) * block;
        let y = legend_y + (i as u32 / 4) * 42;
        rect(&mut image, x, y, block - 8, 32, [110; 3]);
        rect(&mut image, x + 1, y + 1, block - 10, 30, color.rgb);
        let label = format!("{} {}", color.code, counts[color.code]);
        let scale = if label.len() as u32 * 12 < block - 12 {
            2
        } else {
            1
        };
        centered(
            &mut image,
            x + (block - 8) / 2,
            y + (32 - 7 * scale) / 2,
            &label,
            foreground(color.rgb),
            scale,
        );
    }
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

fn foreground(rgb: [u8; 3]) -> [u8; 3] {
    let linear = rgb.map(|v| {
        let v = f64::from(v) / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    let luminance = linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
    if luminance > 0.179 { [0; 3] } else { [255; 3] }
}

fn rect(image: &mut RgbImage, x: u32, y: u32, width: u32, height: u32, rgb: [u8; 3]) {
    for yy in y..(y + height).min(image.height()) {
        for xx in x..(x + width).min(image.width()) {
            image.put_pixel(xx, yy, Rgb(rgb));
        }
    }
}

fn centered(image: &mut RgbImage, x: u32, y: u32, value: &str, rgb: [u8; 3], scale: u32) {
    let width = (value.len() as u32 * 6).saturating_sub(1) * scale;
    text(image, x.saturating_sub(width / 2), y, value, rgb, scale);
}

fn text(image: &mut RgbImage, x: u32, y: u32, value: &str, rgb: [u8; 3], scale: u32) {
    for (i, ch) in value.chars().enumerate() {
        for (row, bits) in glyph(ch).iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    rect(
                        image,
                        x + i as u32 * 6 * scale + col * scale,
                        y + row as u32 * scale,
                        scale,
                        scale,
                        rgb,
                    );
                }
            }
        }
    }
}

// 5x7 bitmap glyphs: only the ASCII characters used in MARD codes and headings.
fn glyph(ch: char) -> [u8; 7] {
    match ch {
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        canvas::{BrushSize, PixelUpdate},
        palette::PaletteMode,
    };

    #[test]
    fn chart_counts_final_beads_and_leaves_empty_cells_unlabelled() {
        let mut c = Canvas::new_empty(3, 1, PaletteMode::Colors221).unwrap();
        c.paint(
            &[
                PixelUpdate {
                    x: 0,
                    y: 0,
                    color: "H2".into(),
                },
                PixelUpdate {
                    x: 1,
                    y: 0,
                    color: "纯黑".into(),
                },
            ],
            BrushSize::One,
        )
        .unwrap();
        assert_eq!(counts(&c), BTreeMap::from([("H2", 1), ("H7", 1)]));
        let im = image::load_from_memory(&encode_png(&c).unwrap())
            .unwrap()
            .to_rgb8();
        // Empty cell interior is entirely white, white bead cell has a black label.
        assert!((TOP + 3..TOP + CELL - 3).all(|y| {
            (LEFT + 2 * CELL + 3..LEFT + 3 * CELL - 3).all(|x| im.get_pixel(x, y).0 == [255; 3])
        }));
        assert!(
            (TOP + 3..TOP + CELL - 3)
                .any(|y| (LEFT + 3..LEFT + CELL - 3).any(|x| im.get_pixel(x, y).0 == [0; 3]))
        );
        c.paint(
            &[PixelUpdate {
                x: 1,
                y: 0,
                color: "-".into(),
            }],
            BrushSize::One,
        )
        .unwrap();
        assert_eq!(counts(&c), BTreeMap::from([("H2", 1)]));
    }

    #[test]
    fn extremes_and_palette_glyphs_fit() {
        for mode in [
            PaletteMode::Colors24,
            PaletteMode::Colors144,
            PaletteMode::Colors221,
        ] {
            for color in mode.colors() {
                for ch in color.code.chars() {
                    assert_ne!(glyph(ch), [0; 7]);
                }
            }
        }
        assert_eq!(foreground([0; 3]), [255; 3]);
        assert_eq!(foreground([255; 3]), [0; 3]);
        let c = Canvas::new_empty(1, 1, PaletteMode::Colors24).unwrap();
        let im = image::load_from_memory(&encode_png(&c).unwrap()).unwrap();
        assert_eq!(im.width(), 480);
        assert!(counts(&c).is_empty());
    }
}
