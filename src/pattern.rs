//! Bead chart laid out like a printable pattern sheet: header, four-sided
//! index, counting lines every 10 beads from the center, and a swatch legend.
//! Cell labels come from stored MARD codes. Empty cells and H2 share an RGB
//! and must not be recovered from color.
use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use std::sync::OnceLock;

use fontdue::{Font, Metrics};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

use crate::{canvas::Canvas, palette};

const FONT_BYTES: &[u8] = include_bytes!("../assets/SheetFont.ttf");
const LOGO_PNG_BYTES: &[u8] = include_bytes!("../assets/xiaohongshu-logo.png");

const PAGE_RGB: [u8; 3] = [0xFF, 0xFF, 0xFF];
const INDEX_RGB: [u8; 3] = [0x6E, 0xB5, 0xE0];
const INK_RGB: [u8; 3] = [0x2A, 0x21, 0x18];
const MAJOR_LINE_RGB: [u8; 3] = [0x00, 0x00, 0x00];
const MAJOR_LINE_EVERY: u32 = 10;
const MAJOR_LINE_W: u32 = 2;
const MAX_SHEET_SIDE: u32 = 4096;
const LUMINANCE_THRESHOLD: f32 = 140.0;
const HEADER_H: u32 = 44;
const BRAND_NAME: &str = "豆图工坊";
const BRAND_GAP: f32 = 8.0;

pub fn counts(canvas: &Canvas) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for code in canvas.codes().iter().flatten() {
        *counts.entry(*code).or_default() += 1;
    }
    counts
}

pub fn encode_png(canvas: &Canvas) -> Result<Vec<u8>, image::ImageError> {
    let counts = counts(canvas);
    let font = sheet_font();
    let mut glyphs = GlyphCache::default();
    let grid_w = canvas.width();
    let grid_h = canvas.height();
    let cell = resolve_cell_size(grid_w, grid_h);
    let grid_px_w = (grid_w + 2) * cell;
    let grid_px_h = (grid_h + 2) * cell;
    let pad = cell.max(16);
    let header_gap = pad / 2;

    let ui_px = (HEADER_H as f32 * 0.42).clamp(12.0, 22.0);
    let brand_px = (HEADER_H as f32 * 0.48).clamp(14.0, 24.0);
    let header = format!(
        "尺寸 {grid_w}\u{00D7}{grid_h}  /  色板 MARD {}  /  总颜色数 {}  /  总拼豆数 {}",
        canvas.mode().as_str(),
        counts.len(),
        counts.values().sum::<usize>()
    );
    let text_gap = 16.0;
    let (logo_w, _) = logo_dest_size(sheet_logo());
    let content_w = grid_px_w.max(
        (measure_text(font, &header, ui_px)
            + text_gap
            + logo_w as f32
            + BRAND_GAP
            + measure_text(font, BRAND_NAME, brand_px))
        .ceil() as u32,
    );

    let legend_gap = (cell / 4).max(6);
    let swatch = (cell * 2).clamp(36, 64);
    let per_row = ((content_w + legend_gap) / (swatch + legend_gap)).max(1);
    let used: Vec<_> = canvas
        .mode()
        .colors()
        .iter()
        .filter(|color| counts.contains_key(color.code))
        .collect();
    let legend_rows = if used.is_empty() {
        0
    } else {
        (used.len() as u32).div_ceil(per_row)
    };
    let legend_block_h = if legend_rows == 0 {
        0
    } else {
        pad + legend_rows * (swatch + legend_gap) - legend_gap + pad
    };
    let section_gap = if legend_rows == 0 { 0 } else { pad };
    let sheet_w = content_w + pad * 2;
    let sheet_h = pad + HEADER_H + header_gap + grid_px_h + section_gap + legend_block_h + pad;

    let mut image = RgbImage::from_pixel(sheet_w, sheet_h, Rgb(PAGE_RGB));
    let origin_x = pad;
    let origin_y = pad + HEADER_H + header_gap;
    draw_header(
        &mut image,
        font,
        &mut glyphs,
        &header,
        ui_px,
        brand_px,
        pad,
        content_w,
    );

    let code_px = (cell as f32 * 0.38).clamp(8.0, 18.0);
    let index_px = (cell as f32 * 0.32).clamp(7.0, 14.0);
    for gy in 0..(grid_h + 2) {
        for gx in 0..(grid_w + 2) {
            let x0 = origin_x + gx * cell;
            let y0 = origin_y + gy * cell;
            let cx = x0 as f32 + cell as f32 * 0.5;
            let cy = y0 as f32 + cell as f32 * 0.5;
            let is_index = gx == 0 || gy == 0 || gx == grid_w + 1 || gy == grid_h + 1;
            if is_index {
                fill_rect(&mut image, x0, y0, x0 + cell, y0 + cell, INDEX_RGB);
                let label = if gy == 0 || gy == grid_h + 1 {
                    (1..=grid_w).contains(&gx).then(|| gx.to_string())
                } else if gx == 0 || gx == grid_w + 1 {
                    (1..=grid_h).contains(&gy).then(|| gy.to_string())
                } else {
                    None
                };
                if let Some(text) = label {
                    draw_text_centered(
                        &mut image,
                        font,
                        &mut glyphs,
                        &text,
                        cx,
                        cy,
                        index_px,
                        [255, 255, 255],
                    );
                }
                continue;
            }
            let x = gx - 1;
            let y = gy - 1;
            let code = canvas.codes()[(y * grid_w + x) as usize];
            let fill = code
                .map(|code| {
                    palette::resolve(canvas.mode(), code)
                        .expect("canvas stores canonical palette codes")
                })
                .unwrap_or(PAGE_RGB);
            fill_rect(&mut image, x0, y0, x0 + cell, y0 + cell, fill);
            if let Some(code) = code {
                draw_text_centered(
                    &mut image,
                    font,
                    &mut glyphs,
                    code,
                    cx,
                    cy,
                    code_px,
                    contrast_text_rgb(fill),
                );
            }
        }
    }

    let grid_right = origin_x + grid_px_w;
    let grid_bottom = origin_y + grid_px_h;
    for bound in major_line_bounds(grid_w, MAJOR_LINE_EVERY) {
        let x0 = (origin_x + (bound + 1) * cell).saturating_sub(MAJOR_LINE_W / 2);
        fill_rect(
            &mut image,
            x0,
            origin_y,
            x0 + MAJOR_LINE_W,
            grid_bottom,
            MAJOR_LINE_RGB,
        );
    }
    for bound in major_line_bounds(grid_h, MAJOR_LINE_EVERY) {
        let y0 = (origin_y + (bound + 1) * cell).saturating_sub(MAJOR_LINE_W / 2);
        fill_rect(
            &mut image,
            origin_x,
            y0,
            grid_right,
            y0 + MAJOR_LINE_W,
            MAJOR_LINE_RGB,
        );
    }

    if !used.is_empty() {
        let legend_top = origin_y + grid_px_h + section_gap;
        let swatch_px = (swatch as f32 * 0.28).clamp(9.0, 16.0);
        let line_gap = swatch_px * 1.15;
        for (i, color) in used.iter().enumerate() {
            let row = i as u32 / per_row;
            let col = i as u32 % per_row;
            let x0 = pad + col * (swatch + legend_gap);
            let y0 = legend_top + row * (swatch + legend_gap);
            fill_rect(&mut image, x0, y0, x0 + swatch, y0 + swatch, color.rgb);
            let text_rgb = contrast_text_rgb(color.rgb);
            let cx = x0 as f32 + swatch as f32 * 0.5;
            let cy = y0 as f32 + swatch as f32 * 0.5;
            let count = format!("({})", counts[color.code]);
            draw_text_centered(
                &mut image,
                font,
                &mut glyphs,
                color.code,
                cx,
                cy - line_gap * 0.5,
                swatch_px,
                text_rgb,
            );
            draw_text_centered(
                &mut image,
                font,
                &mut glyphs,
                &count,
                cx,
                cy + line_gap * 0.5,
                swatch_px,
                text_rgb,
            );
        }
    }

    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

fn resolve_cell_size(width: u32, height: u32) -> u32 {
    let long = width.max(height).saturating_add(2).max(1);
    (MAX_SHEET_SIDE / long).clamp(14, 32)
}

/// Bead-boundary indices in `0..=n`: outer edges plus every `every` beads
/// expanding from the middle pixel.
fn major_line_bounds(n: u32, every: u32) -> Vec<u32> {
    if n == 0 {
        return Vec::new();
    }
    let every = every.max(1);
    let center = n / 2;
    let mut bounds = vec![0, center, n];
    let mut right = center;
    while right + every <= n {
        right += every;
        bounds.push(right);
    }
    let mut left = center;
    while left >= every {
        left -= every;
        bounds.push(left);
    }
    bounds.sort_unstable();
    bounds.dedup();
    bounds
}

fn contrast_text_rgb(bg: [u8; 3]) -> [u8; 3] {
    let lum = 0.299 * bg[0] as f32 + 0.587 * bg[1] as f32 + 0.114 * bg[2] as f32;
    if lum >= LUMINANCE_THRESHOLD {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    }
}

fn sheet_font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
            .expect("embedded pattern-sheet font")
    })
}

struct Glyph {
    metrics: Metrics,
    bitmap: Vec<u8>,
}

#[derive(Default)]
struct GlyphCache {
    glyphs: HashMap<(char, u32), Glyph>,
}

impl GlyphCache {
    fn get(&mut self, font: &Font, ch: char, px: f32) -> &Glyph {
        self.glyphs.entry((ch, px.to_bits())).or_insert_with(|| {
            let (metrics, bitmap) = font.rasterize(ch, px);
            Glyph { metrics, bitmap }
        })
    }
}

fn measure_text(font: &Font, text: &str, px: f32) -> f32 {
    text.chars()
        .map(|ch| font.metrics(ch, px).advance_width)
        .sum()
}

struct LogoRgba {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

fn sheet_logo() -> &'static LogoRgba {
    static LOGO: OnceLock<LogoRgba> = OnceLock::new();
    LOGO.get_or_init(|| {
        let img = image::load_from_memory(LOGO_PNG_BYTES)
            .expect("embedded xiaohongshu logo")
            .to_rgba8();
        LogoRgba {
            width: img.width(),
            height: img.height(),
            data: img.into_raw(),
        }
    })
}

fn logo_dest_size(logo: &LogoRgba) -> (u32, u32) {
    let logo_h = (HEADER_H as f32 * 0.72).round().clamp(18.0, 36.0) as u32;
    let logo_w = if logo.height == 0 {
        logo_h
    } else {
        ((logo.width as f32) * (logo_h as f32 / logo.height as f32))
            .round()
            .max(1.0) as u32
    };
    (logo_w, logo_h)
}

fn blit_logo(image: &mut RgbImage, logo: &LogoRgba, dest_x: u32, dest_y: u32, dest_h: u32) -> u32 {
    if logo.width == 0 || logo.height == 0 || dest_h == 0 {
        return 0;
    }
    let scale = dest_h as f32 / logo.height as f32;
    let dest_w = ((logo.width as f32) * scale).round().max(1.0) as u32;
    for dy in 0..dest_h {
        for dx in 0..dest_w {
            let sx = ((dx as f32 + 0.5) / scale).floor() as u32;
            let sy = ((dy as f32 + 0.5) / scale).floor() as u32;
            if sx >= logo.width || sy >= logo.height {
                continue;
            }
            let si = ((sy * logo.width + sx) * 4) as usize;
            blend_rgba(
                image,
                dest_x + dx,
                dest_y + dy,
                [
                    logo.data[si],
                    logo.data[si + 1],
                    logo.data[si + 2],
                    logo.data[si + 3],
                ],
            );
        }
    }
    dest_w
}

fn draw_header(
    image: &mut RgbImage,
    font: &Font,
    cache: &mut GlyphCache,
    header: &str,
    ui_px: f32,
    brand_px: f32,
    pad: u32,
    content_w: u32,
) {
    let cy = pad as f32 + HEADER_H as f32 * 0.5;
    draw_text(
        image,
        font,
        cache,
        header,
        pad as f32,
        cy + ui_px * 0.35,
        ui_px,
        INK_RGB,
    );
    let logo = sheet_logo();
    let (logo_w_est, logo_h) = logo_dest_size(logo);
    let brand_w = measure_text(font, BRAND_NAME, brand_px);
    let brand_x = pad as f32 + content_w as f32 - brand_w;
    let logo_x = (brand_x - BRAND_GAP - logo_w_est as f32).round().max(0.0) as u32;
    let logo_y = pad + HEADER_H.saturating_sub(logo_h) / 2;
    let logo_w = blit_logo(image, logo, logo_x, logo_y, logo_h);
    draw_text(
        image,
        font,
        cache,
        BRAND_NAME,
        (logo_x + logo_w) as f32 + BRAND_GAP,
        cy + brand_px * 0.35,
        brand_px,
        INK_RGB,
    );
}

fn draw_text_centered(
    image: &mut RgbImage,
    font: &Font,
    cache: &mut GlyphCache,
    text: &str,
    cx: f32,
    cy: f32,
    px: f32,
    rgb: [u8; 3],
) {
    let text_w = measure_text(font, text, px);
    draw_text(
        image,
        font,
        cache,
        text,
        cx - text_w * 0.5,
        cy + px * 0.35,
        px,
        rgb,
    );
}

fn draw_text(
    image: &mut RgbImage,
    font: &Font,
    cache: &mut GlyphCache,
    text: &str,
    x: f32,
    baseline: f32,
    px: f32,
    rgb: [u8; 3],
) {
    if text.is_empty() || px < 4.0 {
        return;
    }
    let mut pen_x = x;
    for ch in text.chars() {
        let glyph = cache.get(font, ch, px);
        let metrics = &glyph.metrics;
        let glyph_x = pen_x + metrics.xmin as f32;
        let glyph_y = baseline - (metrics.height as f32 + metrics.ymin as f32);
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let cover = glyph.bitmap[row * metrics.width + col];
                if cover <= 2 {
                    continue;
                }
                blend(
                    image,
                    (glyph_x + col as f32).round() as i32,
                    (glyph_y + row as f32).round() as i32,
                    rgb,
                    cover,
                );
            }
        }
        pen_x += metrics.advance_width;
    }
}

fn blend_rgba(image: &mut RgbImage, x: u32, y: u32, src: [u8; 4]) {
    if x >= image.width() || y >= image.height() {
        return;
    }
    let a = f32::from(src[3]) / 255.0;
    if a <= 0.0 {
        return;
    }
    let inv = 1.0 - a;
    let pixel = image.get_pixel_mut(x, y);
    for i in 0..3 {
        pixel.0[i] = (f32::from(src[i]) * a + f32::from(pixel.0[i]) * inv).round() as u8;
    }
}

fn blend(image: &mut RgbImage, x: i32, y: i32, rgb: [u8; 3], cover: u8) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if x >= image.width() || y >= image.height() {
        return;
    }
    let a = f32::from(cover) / 255.0;
    let inv = 1.0 - a;
    let pixel = image.get_pixel_mut(x, y);
    for i in 0..3 {
        pixel.0[i] = (f32::from(rgb[i]) * a + f32::from(pixel.0[i]) * inv).round() as u8;
    }
}

fn fill_rect(image: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, rgb: [u8; 3]) {
    let x1 = x1.min(image.width());
    let y1 = y1.min(image.height());
    for y in y0..y1 {
        for x in x0..x1 {
            image.put_pixel(x, y, Rgb(rgb));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        canvas::{BrushSize, PixelUpdate},
        palette::PaletteMode,
    };

    fn origins(width: u32, height: u32) -> (u32, u32, u32) {
        let cell = resolve_cell_size(width, height);
        let pad = cell.max(16);
        (cell, pad, pad + HEADER_H + pad / 2)
    }

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
        let (cell, pad, origin_y) = origins(3, 1);
        let inset = |x: u32, y: u32| {
            let x0 = pad + (x + 1) * cell;
            let y0 = origin_y + (y + 1) * cell;
            (x0 + 3, y0 + 3, x0 + cell - 3, y0 + cell - 3)
        };
        let (x0, y0, x1, y1) = inset(2, 0);
        assert!((y0..y1).all(|y| (x0..x1).all(|x| im.get_pixel(x, y).0 == [255; 3])));
        let (x0, y0, x1, y1) = inset(0, 0);
        assert!((y0..y1).any(|y| {
            (x0..x1).any(|x| {
                let p = im.get_pixel(x, y).0;
                p[0] < 40 && p[1] < 40 && p[2] < 40
            })
        }));
        let (x0, y0, x1, y1) = inset(1, 0);
        assert!(
            (y0..y1).any(|y| { (x0..x1).any(|x| im.get_pixel(x, y).0.iter().all(|v| *v > 240)) })
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
    fn index_legend_and_counting_lines_follow_the_sheet_layout() {
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
                    color: "H7".into(),
                },
            ],
            BrushSize::One,
        )
        .unwrap();
        let im = image::load_from_memory(&encode_png(&c).unwrap())
            .unwrap()
            .to_rgb8();
        let (cell, pad, origin_y) = origins(3, 1);
        assert_eq!(im.get_pixel(pad + 4, origin_y + cell + 4).0, INDEX_RGB);
        let line_x = pad + cell - 1;
        assert_eq!(
            im.get_pixel(line_x, origin_y + cell + cell / 2).0,
            MAJOR_LINE_RGB
        );
        let swatch = (cell * 2).clamp(36, 64);
        let gap = (cell / 4).max(6);
        let legend_top = origin_y + (1 + 2) * cell + pad;
        assert_eq!(
            im.get_pixel(pad + 4, legend_top + 4).0,
            palette::resolve(PaletteMode::Colors221, "H2").unwrap()
        );
        assert_eq!(
            im.get_pixel(pad + swatch + gap + 4, legend_top + 4).0,
            [0, 0, 0]
        );
        assert!(
            (pad..pad + HEADER_H)
                .any(|y| { (pad..pad + 80).any(|x| im.get_pixel(x, y).0 != [255; 3]) })
        );
    }

    #[test]
    fn font_covers_codes_and_counting_lines_expand_from_center() {
        let font = sheet_font();
        for ch in "尺寸色板总颜色数拼豆×/MARD豆图工坊() 0123456789".chars() {
            assert_ne!(font.lookup_glyph_index(ch), 0, "{ch}");
        }
        for mode in [
            PaletteMode::Colors24,
            PaletteMode::Colors144,
            PaletteMode::Colors221,
        ] {
            for color in mode.colors() {
                for ch in color.code.chars() {
                    assert_ne!(font.lookup_glyph_index(ch), 0, "{}", color.code);
                }
            }
        }
        assert_eq!(major_line_bounds(20, 10), vec![0, 10, 20]);
        assert_eq!(major_line_bounds(15, 10), vec![0, 7, 15]);
        assert_eq!(major_line_bounds(30, 10), vec![0, 5, 15, 25, 30]);
        assert!(major_line_bounds(0, 10).is_empty());
        assert_eq!(contrast_text_rgb([0; 3]), [255; 3]);
        assert_eq!(contrast_text_rgb([255; 3]), [0; 3]);
        let c = Canvas::new_empty(1, 1, PaletteMode::Colors24).unwrap();
        let im = image::load_from_memory(&encode_png(&c).unwrap())
            .unwrap()
            .to_rgb8();
        let (cell, pad, _) = origins(1, 1);
        assert!(im.width() > (1 + 2) * cell + pad);
        assert!(counts(&c).is_empty());
        assert!((0..im.width()).any(|x| {
            (pad..pad + HEADER_H).any(|y| {
                let p = im.get_pixel(x, y).0;
                p[0] > 200 && p[1] < 80 && p[2] < 100
            })
        }));
    }
}
