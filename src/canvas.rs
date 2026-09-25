use std::io::Cursor;

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb, imageops::FilterType};

use crate::palette::{self, PaletteMode};
use crate::shapes;

pub const MAX_SIZE: u32 = 128;
pub const SCALE: u32 = 8;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BrushSize {
    #[default]
    One = 1,
    Two = 2,
    Four = 4,
    Eight = 8,
}

impl BrushSize {
    /// Parse `1` / `2` / `4` / `8`. Missing input defaults to 1.
    pub fn parse(input: Option<u32>) -> Result<Self, CanvasError> {
        match input {
            None | Some(1) => Ok(Self::One),
            Some(2) => Ok(Self::Two),
            Some(4) => Ok(Self::Four),
            Some(8) => Ok(Self::Eight),
            Some(size) => Err(CanvasError::InvalidBrush { size }),
        }
    }

    pub fn size(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelUpdate {
    pub x: u32,
    pub y: u32,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasError {
    InvalidSize {
        width: u32,
        height: u32,
    },
    InvalidBrush {
        size: u32,
    },
    OutOfBounds {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Unaligned {
        x: u32,
        y: u32,
        brush: u32,
    },
    UnknownColor {
        color: String,
        mode: PaletteMode,
    },
    InvalidOp {
        message: String,
    },
}

impl std::fmt::Display for CanvasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSize { width, height } => {
                write!(
                    f,
                    "图纸尺寸无效：{width}x{height}，宽和高必须在 1..={MAX_SIZE} 之间"
                )
            }
            Self::InvalidBrush { size } => {
                write!(f, "笔刷大小无效：{size}。只允许 1、2、4、8。")
            }
            Self::OutOfBounds {
                x,
                y,
                width,
                height,
            } => {
                write!(
                    f,
                    "坐标越界：({x}, {y}) 不在 0..{width} x 0..{height} 范围内"
                )
            }
            Self::Unaligned { x, y, brush } => {
                write!(
                    f,
                    "坐标未对齐：({x}, {y})。笔刷 {brush} 只能落在 0,{brush},{}… 的网格交点上，且 {brush}×{brush} 方块必须完全在图纸内。",
                    brush.saturating_mul(2)
                )
            }
            Self::UnknownColor { color, mode } => {
                write!(
                    f,
                    "未知颜色「{color}」。当前为 {} 色模式，请使用该模式中的中文名或 MARD 色号。调用 list_colors 查看完整列表。",
                    mode.as_str()
                )
            }
            Self::InvalidOp { message } => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for CanvasError {}

#[derive(Debug, Clone)]
pub struct Canvas {
    width: u32,
    height: u32,
    mode: PaletteMode,
    pixels: Vec<[u8; 3]>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, mode: PaletteMode) -> Result<Self, CanvasError> {
        if !(1..=MAX_SIZE).contains(&width) || !(1..=MAX_SIZE).contains(&height) {
            return Err(CanvasError::InvalidSize { width, height });
        }
        let len = (width as usize)
            .checked_mul(height as usize)
            .expect("width and height are capped at MAX_SIZE");
        Ok(Self {
            width,
            height,
            mode,
            pixels: vec![mode.white_rgb(); len],
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn mode(&self) -> PaletteMode {
        self.mode
    }

    /// Validate the whole batch, then paint. Any invalid stamp rejects everything.
    /// Returns the number of pixels written (stamps × brush²).
    pub fn paint(
        &mut self,
        updates: &[PixelUpdate],
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        let n = brush.size();
        let mut resolved = Vec::with_capacity(updates.len());
        for update in updates {
            if update.x % n != 0 || update.y % n != 0 {
                return Err(CanvasError::Unaligned {
                    x: update.x,
                    y: update.y,
                    brush: n,
                });
            }
            if update.x.saturating_add(n) > self.width || update.y.saturating_add(n) > self.height {
                return Err(CanvasError::OutOfBounds {
                    x: update.x,
                    y: update.y,
                    width: self.width,
                    height: self.height,
                });
            }
            let rgb = palette::resolve(self.mode, &update.color).ok_or_else(|| {
                CanvasError::UnknownColor {
                    color: update.color.clone(),
                    mode: self.mode,
                }
            })?;
            resolved.push((update.x, update.y, rgb));
        }
        for (x, y, rgb) in resolved {
            for dy in 0..n {
                for dx in 0..n {
                    let index = ((y + dy) * self.width + (x + dx)) as usize;
                    self.pixels[index] = rgb;
                }
            }
        }
        Ok(updates.len() * (n as usize) * (n as usize))
    }

    /// Stamp a brush-sized square centered on `(x, y)`, clipping at the edges.
    /// Returns how many pixels were written.
    fn stamp_centered(&mut self, x: i32, y: i32, rgb: [u8; 3], n: u32) -> usize {
        let offset = (n as i32) / 2;
        let left = x - offset;
        let top = y - offset;
        let mut written = 0;
        for dy in 0..n as i32 {
            for dx in 0..n as i32 {
                let px = left + dx;
                let py = top + dy;
                if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
                    continue;
                }
                let index = (py as u32 * self.width + px as u32) as usize;
                self.pixels[index] = rgb;
                written += 1;
            }
        }
        written
    }

    /// Paint pre-rasterized shape points with a brush stamp on each point.
    /// Coordinates may fall outside the canvas; those stamps are clipped.
    pub fn paint_shape_points(
        &mut self,
        points: &[(i32, i32)],
        color: &str,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        let rgb = palette::resolve(self.mode, color).ok_or_else(|| CanvasError::UnknownColor {
            color: color.to_string(),
            mode: self.mode,
        })?;
        let n = brush.size();
        let mut written = 0;
        for &(x, y) in points {
            written += self.stamp_centered(x, y, rgb, n);
        }
        Ok(written)
    }

    pub fn paint_line(
        &mut self,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        color: &str,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        self.require_point(x0, y0)?;
        self.require_point(x1, y1)?;
        let points = shapes::line_points(x0 as i32, y0 as i32, x1 as i32, y1 as i32);
        self.paint_shape_points(&points, color, brush)
    }

    pub fn paint_rect(
        &mut self,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        color: &str,
        fill: bool,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        self.require_point(x0, y0)?;
        self.require_point(x1, y1)?;
        let points = if fill {
            shapes::rect_fill_points(x0 as i32, y0 as i32, x1 as i32, y1 as i32)
        } else {
            shapes::rect_stroke_points(x0 as i32, y0 as i32, x1 as i32, y1 as i32)
        };
        // Fills paint the exact region; brush thickness only applies to strokes.
        let brush = if fill { BrushSize::One } else { brush };
        self.paint_shape_points(&points, color, brush)
    }

    pub fn paint_triangle(
        &mut self,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        color: &str,
        fill: bool,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        self.require_point(x0, y0)?;
        self.require_point(x1, y1)?;
        self.require_point(x2, y2)?;
        let points = if fill {
            shapes::triangle_fill_points(
                x0 as i32,
                y0 as i32,
                x1 as i32,
                y1 as i32,
                x2 as i32,
                y2 as i32,
            )
        } else {
            shapes::triangle_stroke_points(
                x0 as i32,
                y0 as i32,
                x1 as i32,
                y1 as i32,
                x2 as i32,
                y2 as i32,
            )
        };
        let brush = if fill { BrushSize::One } else { brush };
        self.paint_shape_points(&points, color, brush)
    }

    pub fn paint_circle(
        &mut self,
        cx: u32,
        cy: u32,
        radius: u32,
        color: &str,
        fill: bool,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        self.require_point(cx, cy)?;
        let r = radius as i32;
        let points = if fill {
            shapes::circle_fill_points(cx as i32, cy as i32, r)
        } else {
            shapes::circle_stroke_points(cx as i32, cy as i32, r)
        };
        let brush = if fill { BrushSize::One } else { brush };
        self.paint_shape_points(&points, color, brush)
    }

    pub fn paint_ellipse(
        &mut self,
        cx: u32,
        cy: u32,
        rx: u32,
        ry: u32,
        color: &str,
        fill: bool,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        self.require_point(cx, cy)?;
        let points = if fill {
            shapes::ellipse_fill_points(cx as i32, cy as i32, rx as i32, ry as i32)
        } else {
            shapes::ellipse_stroke_points(cx as i32, cy as i32, rx as i32, ry as i32)
        };
        let brush = if fill { BrushSize::One } else { brush };
        self.paint_shape_points(&points, color, brush)
    }

    /// 4-connected flood fill replacing the seed color. Returns pixels changed.
    pub fn flood_fill(&mut self, x: u32, y: u32, color: &str) -> Result<usize, CanvasError> {
        self.require_point(x, y)?;
        let rgb = palette::resolve(self.mode, color).ok_or_else(|| CanvasError::UnknownColor {
            color: color.to_string(),
            mode: self.mode,
        })?;
        let seed_index = (y * self.width + x) as usize;
        let target = self.pixels[seed_index];
        if target == rgb {
            return Ok(0);
        }
        let mut stack = vec![(x, y)];
        let mut written = 0;
        while let Some((px, py)) = stack.pop() {
            let index = (py * self.width + px) as usize;
            if self.pixels[index] != target {
                continue;
            }
            self.pixels[index] = rgb;
            written += 1;
            if px > 0 {
                stack.push((px - 1, py));
            }
            if px + 1 < self.width {
                stack.push((px + 1, py));
            }
            if py > 0 {
                stack.push((px, py - 1));
            }
            if py + 1 < self.height {
                stack.push((px, py + 1));
            }
        }
        Ok(written)
    }

    fn require_point(&self, x: u32, y: u32) -> Result<(), CanvasError> {
        if x >= self.width || y >= self.height {
            return Err(CanvasError::OutOfBounds {
                x,
                y,
                width: self.width,
                height: self.height,
            });
        }
        Ok(())
    }

    pub fn encode_png_8x(&self) -> Result<Vec<u8>, image::ImageError> {
        let img = ImageBuffer::from_fn(self.width, self.height, |x, y| {
            let index = (y * self.width + x) as usize;
            Rgb(self.pixels[index])
        });
        let scaled = image::imageops::resize(
            &img,
            self.width * SCALE,
            self.height * SCALE,
            FilterType::Nearest,
        );
        let mut buf = Vec::new();
        DynamicImage::ImageRgb8(scaled).write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_size() {
        assert!(matches!(
            Canvas::new(0, 16, PaletteMode::Colors144),
            Err(CanvasError::InvalidSize { .. })
        ));
        assert!(matches!(
            Canvas::new(16, 129, PaletteMode::Colors24),
            Err(CanvasError::InvalidSize { .. })
        ));
    }

    #[test]
    fn paints_and_rejects_out_of_bounds() {
        let mut canvas = Canvas::new(4, 4, PaletteMode::Colors24).unwrap();
        let ok = canvas
            .paint(
                &[PixelUpdate {
                    x: 1,
                    y: 2,
                    color: "红色".into(),
                }],
                BrushSize::One,
            )
            .unwrap();
        assert_eq!(ok, 1);

        let err = canvas
            .paint(
                &[PixelUpdate {
                    x: 4,
                    y: 0,
                    color: "黑色".into(),
                }],
                BrushSize::One,
            )
            .unwrap_err();
        assert!(matches!(err, CanvasError::OutOfBounds { x: 4, y: 0, .. }));
    }

    #[test]
    fn rejects_unknown_color_without_painting() {
        let mut canvas = Canvas::new(2, 2, PaletteMode::Colors24).unwrap();
        let err = canvas
            .paint(
                &[
                    PixelUpdate {
                        x: 0,
                        y: 0,
                        color: "黑色".into(),
                    },
                    PixelUpdate {
                        x: 1,
                        y: 1,
                        color: "彩虹".into(),
                    },
                ],
                BrushSize::One,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            CanvasError::UnknownColor {
                mode: PaletteMode::Colors24,
                ..
            }
        ));
        assert_eq!(canvas.pixels[0], PaletteMode::Colors24.white_rgb());
        assert!(err.to_string().contains("24 色模式"));
        assert!(err.to_string().contains("list_colors"));
        assert!(!err.to_string().contains("白色、"));
    }

    #[test]
    fn twenty_four_mode_rejects_144_only_name_without_painting() {
        let mut canvas = Canvas::new(2, 2, PaletteMode::Colors24).unwrap();
        let err = canvas
            .paint(
                &[
                    PixelUpdate {
                        x: 0,
                        y: 0,
                        color: "黑色".into(),
                    },
                    PixelUpdate {
                        x: 1,
                        y: 1,
                        color: "鹅黄".into(),
                    },
                ],
                BrushSize::One,
            )
            .unwrap_err();
        assert!(matches!(err, CanvasError::UnknownColor { .. }));
        assert_eq!(canvas.pixels[0], PaletteMode::Colors24.white_rgb());
    }

    #[test]
    fn default_221_mode_rejects_24_only_name() {
        let mut canvas = Canvas::new(2, 2, PaletteMode::default()).unwrap();
        assert_eq!(canvas.mode(), PaletteMode::Colors221);
        let err = canvas
            .paint(
                &[PixelUpdate {
                    x: 0,
                    y: 0,
                    color: "红色".into(),
                }],
                BrushSize::One,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            CanvasError::UnknownColor {
                mode: PaletteMode::Colors221,
                ..
            }
        ));
        canvas
            .paint(
                &[PixelUpdate {
                    x: 0,
                    y: 0,
                    color: "正红".into(),
                }],
                BrushSize::One,
            )
            .unwrap();
        assert_eq!(canvas.pixels[0], [0xE7, 0x00, 0x2F]);
        canvas
            .paint(
                &[PixelUpdate {
                    x: 1,
                    y: 1,
                    color: "雪白".into(),
                }],
                BrushSize::One,
            )
            .unwrap();
        assert_eq!(canvas.pixels[3], [0xFD, 0xFB, 0xFF]);
    }

    #[test]
    fn png_is_scaled_8x() {
        let canvas = Canvas::new(3, 2, PaletteMode::Colors144).unwrap();
        let png = canvas.encode_png_8x().unwrap();
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!(img.width(), 24);
        assert_eq!(img.height(), 16);
    }

    #[test]
    fn nearest_neighbor_keeps_solid_pixels() {
        let mut canvas = Canvas::new(2, 1, PaletteMode::Colors24).unwrap();
        canvas
            .paint(
                &[PixelUpdate {
                    x: 0,
                    y: 0,
                    color: "黑色".into(),
                }],
                BrushSize::One,
            )
            .unwrap();
        let png = canvas.encode_png_8x().unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(7, 7), &Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(8, 0), &Rgb([255, 255, 255]));
    }

    fn stamp(x: u32, y: u32, color: &str) -> PixelUpdate {
        PixelUpdate {
            x,
            y,
            color: color.into(),
        }
    }

    #[test]
    fn brush_parse_only_allows_1_2_4_8() {
        assert_eq!(BrushSize::parse(None).unwrap(), BrushSize::One);
        assert_eq!(BrushSize::parse(Some(1)).unwrap(), BrushSize::One);
        assert_eq!(BrushSize::parse(Some(2)).unwrap(), BrushSize::Two);
        assert_eq!(BrushSize::parse(Some(4)).unwrap(), BrushSize::Four);
        assert_eq!(BrushSize::parse(Some(8)).unwrap(), BrushSize::Eight);
        assert!(matches!(
            BrushSize::parse(Some(3)),
            Err(CanvasError::InvalidBrush { size: 3 })
        ));
        assert!(matches!(
            BrushSize::parse(Some(0)),
            Err(CanvasError::InvalidBrush { size: 0 })
        ));
    }

    #[test]
    fn brush_two_fills_aligned_block() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors221).unwrap();
        let written = canvas
            .paint(&[stamp(0, 0, "正红")], BrushSize::Two)
            .unwrap();
        assert_eq!(written, 4);
        let red = [0xE7, 0x00, 0x2F];
        let white = PaletteMode::Colors221.white_rgb();
        assert_eq!(canvas.pixels[0], red);
        assert_eq!(canvas.pixels[1], red);
        assert_eq!(canvas.pixels[8], red);
        assert_eq!(canvas.pixels[9], red);
        assert_eq!(canvas.pixels[2], white);
        assert_eq!(canvas.pixels[16], white);
    }

    #[test]
    fn brush_two_rejects_unaligned_without_painting() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors221).unwrap();
        let err = canvas
            .paint(&[stamp(0, 0, "正红"), stamp(1, 0, "纯黑")], BrushSize::Two)
            .unwrap_err();
        assert!(matches!(
            err,
            CanvasError::Unaligned {
                x: 1,
                y: 0,
                brush: 2
            }
        ));
        assert!(err.to_string().contains("笔刷 2"));
        assert_eq!(canvas.pixels[0], PaletteMode::Colors221.white_rgb());
    }

    #[test]
    fn shape_stroke_uses_brush_thickness_without_alignment() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        let written = canvas
            .paint_line(1, 1, 5, 1, "黑色", BrushSize::Two)
            .unwrap();
        assert!(written >= 10);
        let black = [0, 0, 0];
        assert_eq!(canvas.pixels[1 * 8 + 1], black);
        assert_eq!(canvas.pixels[0 * 8 + 1], black);
        assert_eq!(canvas.pixels[1 * 8 + 2], black);
    }

    #[test]
    fn rect_fill_and_stroke_differ() {
        let mut canvas = Canvas::new(6, 6, PaletteMode::Colors24).unwrap();
        canvas
            .paint_rect(1, 1, 3, 3, "红色", true, BrushSize::One)
            .unwrap();
        let red = [0xD8, 0x01, 0x27];
        let white = PaletteMode::Colors24.white_rgb();
        assert_eq!(canvas.pixels[1 * 6 + 1], red);
        assert_eq!(canvas.pixels[2 * 6 + 2], red);
        assert_eq!(canvas.pixels[0], white);

        let mut stroke = Canvas::new(6, 6, PaletteMode::Colors24).unwrap();
        stroke
            .paint_rect(1, 1, 3, 3, "黑色", false, BrushSize::One)
            .unwrap();
        assert_eq!(stroke.pixels[1 * 6 + 1], [0, 0, 0]);
        assert_eq!(stroke.pixels[2 * 6 + 2], white);
        assert_eq!(stroke.pixels[1 * 6 + 3], [0, 0, 0]);
    }

    #[test]
    fn triangle_fill_and_stroke_differ() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        canvas
            .paint_triangle(1, 1, 6, 1, 1, 6, "红色", true, BrushSize::One)
            .unwrap();
        let red = [0xD8, 0x01, 0x27];
        let white = PaletteMode::Colors24.white_rgb();
        assert_eq!(canvas.pixels[1 * 8 + 1], red);
        assert_eq!(canvas.pixels[2 * 8 + 2], red);
        assert_eq!(canvas.pixels[7 * 8 + 7], white);

        let mut stroke = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        stroke
            .paint_triangle(1, 1, 6, 1, 1, 6, "黑色", false, BrushSize::One)
            .unwrap();
        assert_eq!(stroke.pixels[1 * 8 + 1], [0, 0, 0]);
        assert_eq!(stroke.pixels[1 * 8 + 6], [0, 0, 0]);
        assert_eq!(stroke.pixels[6 * 8 + 1], [0, 0, 0]);
        assert_eq!(stroke.pixels[3 * 8 + 3], white);
    }

    #[test]
    fn circle_and_ellipse_fill_and_flood() {
        let mut canvas = Canvas::new(9, 9, PaletteMode::Colors24).unwrap();
        canvas
            .paint_circle(4, 4, 2, "蓝色", true, BrushSize::One)
            .unwrap();
        let blue = [0x10, 0x54, 0xC0];
        let yellow = [0xFF, 0xE9, 0x53];
        assert_eq!(canvas.pixels[4 * 9 + 4], blue);
        canvas.flood_fill(0, 0, "黄色").unwrap();
        assert_eq!(canvas.pixels[0], yellow);
        assert_eq!(canvas.pixels[4 * 9 + 4], blue);

        let mut ellipse = Canvas::new(9, 9, PaletteMode::Colors24).unwrap();
        ellipse
            .paint_ellipse(4, 4, 3, 1, "黑色", true, BrushSize::One)
            .unwrap();
        assert_eq!(ellipse.pixels[4 * 9 + 4], [0, 0, 0]);
        assert_eq!(ellipse.pixels[4 * 9 + 1], [0, 0, 0]);
        assert_eq!(
            ellipse.pixels[0 * 9 + 4],
            PaletteMode::Colors24.white_rgb()
        );
    }

    #[test]
    fn shape_unknown_color_rejects_whole_op() {
        let mut canvas = Canvas::new(4, 4, PaletteMode::Colors24).unwrap();
        let err = canvas
            .paint_line(0, 0, 3, 3, "彩虹", BrushSize::One)
            .unwrap_err();
        assert!(matches!(err, CanvasError::UnknownColor { .. }));
        assert_eq!(canvas.pixels[0], PaletteMode::Colors24.white_rgb());
    }

    #[test]
    fn brush_four_and_eight_align_and_overflow() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors221).unwrap();
        assert_eq!(
            canvas
                .paint(&[stamp(4, 0, "正红")], BrushSize::Four)
                .unwrap(),
            16
        );
        let err = canvas
            .paint(&[stamp(2, 0, "纯黑")], BrushSize::Four)
            .unwrap_err();
        assert!(matches!(err, CanvasError::Unaligned { brush: 4, .. }));

        let overflow = canvas
            .paint(&[stamp(8, 0, "纯黑")], BrushSize::Eight)
            .unwrap_err();
        assert!(matches!(overflow, CanvasError::OutOfBounds { x: 8, .. }));

        let mut small = Canvas::new(8, 8, PaletteMode::Colors221).unwrap();
        assert_eq!(
            small
                .paint(&[stamp(0, 0, "正红")], BrushSize::Eight)
                .unwrap(),
            64
        );
        let mut tight = Canvas::new(5, 5, PaletteMode::Colors221).unwrap();
        let err = tight
            .paint(&[stamp(4, 0, "正红")], BrushSize::Four)
            .unwrap_err();
        assert!(matches!(err, CanvasError::OutOfBounds { x: 4, .. }));
        assert_eq!(tight.pixels[0], PaletteMode::Colors221.white_rgb());
    }
}
