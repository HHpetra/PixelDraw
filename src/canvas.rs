use std::io::Cursor;

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba, imageops::FilterType};

use crate::palette::{self, Color, PaletteMode};
use crate::shapes;

pub const MAX_SIZE: u32 = 128;
pub const SCALE: u32 = 8;
/// Radii larger than this cannot cover the canvas after clipping, so they are
/// capped before rasterization (avoids unbounded point buffers and i64 overflow).
pub const MAX_RADIUS: u32 = 256;

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
    EmptyUndo,
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
            Self::EmptyUndo => {
                write!(f, "没有可撤回的绘制步骤。")
            }
        }
    }
}

impl std::error::Error for CanvasError {}

/// Maximum number of undo snapshots kept per canvas.
pub const MAX_UNDO: usize = 64;

#[derive(Debug, Clone)]
struct Snapshot {
    pixels: Vec<[u8; 3]>,
    codes: Vec<Option<&'static str>>,
}

#[derive(Debug, Clone)]
pub struct Canvas {
    width: u32,
    height: u32,
    mode: PaletteMode,
    pixels: Vec<[u8; 3]>,
    codes: Vec<Option<&'static str>>,
    undo: Vec<Snapshot>,
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
            codes: vec![Some("H2"); len],
            undo: Vec::new(),
        })
    }

    pub fn new_empty(width: u32, height: u32, mode: PaletteMode) -> Result<Self, CanvasError> {
        let mut canvas = Self::new(width, height, mode)?;
        canvas.codes.fill(None);
        Ok(canvas)
    }

    pub fn codes(&self) -> &[Option<&'static str>] {
        &self.codes
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

    fn push_undo(&mut self) {
        self.undo.push(Snapshot {
            pixels: self.pixels.clone(),
            codes: self.codes.clone(),
        });
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    /// Number of undo steps currently available.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Mark the current undo depth so a multi-op tool can collapse into one step.
    pub fn undo_mark(&self) -> usize {
        self.undo.len()
    }

    /// Collapse undo entries added after `mark` into a single step that restores
    /// the pre-call state. No-op when at most one entry was added.
    pub fn collapse_undo_to(&mut self, mark: usize) {
        if self.undo.len() > mark + 1 {
            let oldest = self.undo[mark].clone();
            self.undo.truncate(mark);
            self.undo.push(oldest);
        }
    }

    /// Restore the previous canvas state. Returns remaining undo steps.
    pub fn undo(&mut self, steps: usize) -> Result<usize, CanvasError> {
        if self.undo.is_empty() {
            return Err(CanvasError::EmptyUndo);
        }
        let steps = steps.min(self.undo.len());
        for _ in 0..steps {
            let Some(snap) = self.undo.pop() else {
                break;
            };
            self.pixels = snap.pixels;
            self.codes = snap.codes;
        }
        Ok(self.undo.len())
    }

    /// Validate the whole batch, then paint. Any invalid stamp rejects everything.
    /// Returns how many distinct canvas cells were written.
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
            let color = if update.color.trim() == "-" {
                None
            } else {
                Some(
                    palette::resolve_color(self.mode, &update.color).ok_or_else(|| {
                        CanvasError::UnknownColor {
                            color: update.color.clone(),
                            mode: self.mode,
                        }
                    })?,
                )
            };
            resolved.push((update.x, update.y, color));
        }
        if resolved.is_empty() {
            return Ok(0);
        }
        self.push_undo();
        let mut touched = vec![false; self.pixels.len()];
        let mut written = 0usize;
        for (x, y, color) in resolved {
            for dy in 0..n {
                for dx in 0..n {
                    let index = ((y + dy) * self.width + (x + dx)) as usize;
                    self.write_cell(index, color);
                    if !touched[index] {
                        touched[index] = true;
                        written += 1;
                    }
                }
            }
        }
        Ok(written)
    }

    /// Keep display color, canonical bead code and occupancy in sync.
    fn write_cell(&mut self, index: usize, color: Option<&Color>) {
        self.pixels[index] = color.map_or(self.mode.white_rgb(), |c| c.rgb);
        self.codes[index] = color.map(|c| c.code);
    }

    /// Stamp a brush-sized square with top-left at `(x, y)`, clipping at the edges.
    /// Same footprint as `draw_pixels` at the same coordinates. Marks `touched`
    /// and returns how many previously untouched cells were written.
    fn stamp_block(
        &mut self,
        x: i32,
        y: i32,
        color: Option<&Color>,
        n: u32,
        touched: &mut [bool],
    ) -> usize {
        let mut written = 0;
        for dy in 0..n as i32 {
            for dx in 0..n as i32 {
                let px = x + dx;
                let py = y + dy;
                if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
                    continue;
                }
                let index = (py as u32 * self.width + px as u32) as usize;
                self.write_cell(index, color);
                if !touched[index] {
                    touched[index] = true;
                    written += 1;
                }
            }
        }
        written
    }

    /// Paint pre-rasterized shape points with a brush stamp on each point.
    /// Coordinates may fall outside the canvas; those stamps are clipped.
    /// Records one undo step only when at least one cell is written.
    pub fn paint_shape_points(
        &mut self,
        points: &[(i32, i32)],
        color: &str,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        let color =
            palette::resolve_color(self.mode, color).ok_or_else(|| CanvasError::UnknownColor {
                color: color.to_string(),
                mode: self.mode,
            })?;
        if points.is_empty() {
            return Ok(0);
        }
        self.push_undo();
        let n = brush.size();
        let mut touched = vec![false; self.pixels.len()];
        let mut written = 0;
        for &(x, y) in points {
            written += self.stamp_block(x, y, Some(color), n, &mut touched);
        }
        if written == 0 {
            self.undo.pop();
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

    #[allow(clippy::too_many_arguments)] // Coordinates mirror the drawing tool parameters.
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

    #[allow(clippy::too_many_arguments)] // Coordinates mirror the drawing tool parameters.
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
                x0 as i32, y0 as i32, x1 as i32, y1 as i32, x2 as i32, y2 as i32,
            )
        } else {
            shapes::triangle_stroke_points(
                x0 as i32, y0 as i32, x1 as i32, y1 as i32, x2 as i32, y2 as i32,
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
        let r = radius.min(MAX_RADIUS) as i32;
        let points = if fill {
            shapes::circle_fill_points(cx as i32, cy as i32, r)
        } else {
            shapes::circle_stroke_points(cx as i32, cy as i32, r)
        };
        let brush = if fill { BrushSize::One } else { brush };
        self.paint_shape_points(&points, color, brush)
    }

    #[allow(clippy::too_many_arguments)] // Coordinates mirror the drawing tool parameters.
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
        let rx = rx.min(MAX_RADIUS) as i32;
        let ry = ry.min(MAX_RADIUS) as i32;
        let points = if fill {
            shapes::ellipse_fill_points(cx as i32, cy as i32, rx, ry)
        } else {
            shapes::ellipse_stroke_points(cx as i32, cy as i32, rx, ry)
        };
        let brush = if fill { BrushSize::One } else { brush };
        self.paint_shape_points(&points, color, brush)
    }

    /// Smooth curve through waypoints:
    /// 2 points draw a line, 3 points a quadratic Bezier, 4 points a cubic Bezier,
    /// and 5+ points a Catmull-Rom spline through every waypoint.
    pub fn paint_curve(
        &mut self,
        points: &[(u32, u32)],
        color: &str,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        if points.len() < 2 {
            return Err(CanvasError::InvalidOp {
                message: "曲线至少需要 2 个路径点。".into(),
            });
        }
        for &(x, y) in points {
            self.require_point(x, y)?;
        }
        let raster: Vec<(i32, i32)> = match points.len() {
            2 => shapes::line_points(
                points[0].0 as i32,
                points[0].1 as i32,
                points[1].0 as i32,
                points[1].1 as i32,
            ),
            3 => shapes::quadratic_bezier_points(
                points[0].0 as i32,
                points[0].1 as i32,
                points[1].0 as i32,
                points[1].1 as i32,
                points[2].0 as i32,
                points[2].1 as i32,
            ),
            4 => shapes::cubic_bezier_points(
                (points[0].0 as i32, points[0].1 as i32),
                (points[1].0 as i32, points[1].1 as i32),
                (points[2].0 as i32, points[2].1 as i32),
                (points[3].0 as i32, points[3].1 as i32),
            ),
            _ => shapes::catmull_rom_points(
                &points
                    .iter()
                    .map(|&(x, y)| (x as i32, y as i32))
                    .collect::<Vec<_>>(),
            ),
        };
        self.paint_shape_points(&raster, color, brush)
    }

    /// 4-connected flood fill replacing the seed bead code (or empty cells).
    /// Returns pixels changed; empty cells are distinct from white beads.
    /// `color` may be `-` to clear the region back to empty cells.
    pub fn flood_fill(&mut self, x: u32, y: u32, color: &str) -> Result<usize, CanvasError> {
        self.require_point(x, y)?;
        let fill = if color.trim() == "-" {
            None
        } else {
            Some(palette::resolve_color(self.mode, color).ok_or_else(|| {
                CanvasError::UnknownColor {
                    color: color.to_string(),
                    mode: self.mode,
                }
            })?)
        };
        let seed_index = (y * self.width + x) as usize;
        // Compare by bead code: empty cells and H2 share the same RGB.
        let target = self.codes[seed_index];
        let new_code = fill.map(|c| c.code);
        if target == new_code {
            return Ok(0);
        }
        self.push_undo();
        let mut stack = vec![(x, y)];
        let mut written = 0;
        while let Some((px, py)) = stack.pop() {
            let index = (py * self.width + px) as usize;
            if self.codes[index] != target {
                continue;
            }
            self.write_cell(index, fill);
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
        if written == 0 {
            self.undo.pop();
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
            let [r, g, b] = self.pixels[index];
            Rgba([r, g, b, if self.codes[index].is_some() { 255 } else { 0 }])
        });
        let scaled = image::imageops::resize(
            &img,
            self.width * SCALE,
            self.height * SCALE,
            FilterType::Nearest,
        );
        let mut buf = Vec::new();
        DynamicImage::ImageRgba8(scaled).write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    // Compare all observable state with the established draw_pixels path.
    fn assert_drawing_state(draw: impl Fn(&mut Canvas, &str, BrushSize)) {
        for mode in [
            PaletteMode::Colors24,
            PaletteMode::Colors144,
            PaletteMode::Colors221,
        ] {
            let red = palette::resolve_color(mode, "F5").unwrap();
            for empty in [true, false] {
                for brush in [
                    BrushSize::One,
                    BrushSize::Two,
                    BrushSize::Four,
                    BrushSize::Eight,
                ] {
                    let mut canvas = if empty {
                        Canvas::new_empty(9, 9, mode).unwrap()
                    } else {
                        Canvas::new(9, 9, mode).unwrap()
                    };
                    let mut expected = canvas.clone();
                    draw(&mut canvas, red.name, brush);
                    let updates: Vec<_> = canvas
                        .pixels
                        .iter()
                        .enumerate()
                        .filter(|(_, rgb)| **rgb == red.rgb)
                        .map(|(i, _)| stamp(i as u32 % 9, i as u32 / 9, "f5"))
                        .collect();
                    assert!(!updates.is_empty());
                    expected.paint(&updates, BrushSize::One).unwrap();
                    assert_eq!(
                        canvas.codes(),
                        expected.codes(),
                        "{mode:?}, empty={empty}, {brush:?}"
                    );
                    assert_eq!(
                        canvas.encode_png_8x().unwrap(),
                        expected.encode_png_8x().unwrap()
                    );
                    assert_eq!(
                        crate::pattern::counts(&canvas),
                        crate::pattern::counts(&expected)
                    );
                    assert_eq!(
                        crate::pattern::encode_png(&canvas).unwrap(),
                        crate::pattern::encode_png(&expected).unwrap()
                    );
                }
            }
        }
    }

    #[test]
    fn line_keeps_bead_state() {
        assert_drawing_state(|c, color, brush| {
            c.paint_line(0, 0, 5, 1, color, brush).unwrap();
        });
    }

    #[test]
    fn rect_keeps_bead_state() {
        for fill in [false, true] {
            assert_drawing_state(|c, color, brush| {
                c.paint_rect(0, 0, 4, 3, color, fill, brush).unwrap();
            });
        }
    }

    #[test]
    fn triangle_keeps_bead_state() {
        for fill in [false, true] {
            assert_drawing_state(|c, color, brush| {
                c.paint_triangle(0, 0, 4, 0, 0, 4, color, fill, brush)
                    .unwrap();
            });
        }
    }

    #[test]
    fn circle_keeps_bead_state() {
        for fill in [false, true] {
            assert_drawing_state(|c, color, brush| {
                c.paint_circle(1, 1, 3, color, fill, brush).unwrap();
            });
        }
    }

    #[test]
    fn ellipse_keeps_bead_state() {
        for fill in [false, true] {
            assert_drawing_state(|c, color, brush| {
                c.paint_ellipse(1, 1, 3, 2, color, fill, brush).unwrap();
            });
        }
    }

    #[test]
    fn flood_fill_keeps_bead_state() {
        assert_drawing_state(|c, color, _| {
            assert_eq!(c.flood_fill(0, 0, color).unwrap(), 81);
        });
    }

    #[test]
    fn flood_fill_distinguishes_empty_cells_from_white_beads() {
        for mode in [
            PaletteMode::Colors24,
            PaletteMode::Colors144,
            PaletteMode::Colors221,
        ] {
            let mut c = Canvas::new_empty(3, 1, mode).unwrap();
            c.paint(&[stamp(1, 0, "H2")], BrushSize::One).unwrap();
            assert_eq!(c.flood_fill(0, 0, "h2").unwrap(), 1);
            assert_eq!(c.codes(), &[Some("H2"), Some("H2"), None]);
            assert_eq!(c.flood_fill(0, 0, "H2").unwrap(), 0);
            assert_eq!(c.flood_fill(1, 0, "F5").unwrap(), 2);
            assert_eq!(c.codes(), &[Some("F5"), Some("F5"), None]);
            assert_eq!(c.flood_fill(2, 0, "F5").unwrap(), 1);
            assert_eq!(crate::pattern::counts(&c).get("F5"), Some(&3));
        }
    }

    #[test]
    fn shape_and_fill_errors_leave_bead_state_unchanged() {
        let mut c = Canvas::new_empty(4, 4, PaletteMode::Colors221).unwrap();
        c.paint(&[stamp(0, 0, "F5")], BrushSize::One).unwrap();
        let before = c.clone();
        assert!(c.paint_line(0, 0, 3, 3, "invalid", BrushSize::One).is_err());
        assert!(c.flood_fill(0, 0, "invalid").is_err());
        assert!(c.flood_fill(4, 0, "H2").is_err());
        assert_eq!(c.codes(), before.codes());
        assert_eq!(c.pixels, before.pixels);
    }

    #[test]
    fn empty_white_and_canonical_identity_survive_painting() {
        for mode in [
            PaletteMode::Colors24,
            PaletteMode::Colors144,
            PaletteMode::Colors221,
        ] {
            let mut c = Canvas::new_empty(4, 2, mode).unwrap();
            assert!(c.codes().iter().all(Option::is_none));
            c.paint(
                &[stamp(0, 0, "h2"), stamp(1, 0, mode.colors()[0].name)],
                BrushSize::One,
            )
            .unwrap();
            assert_eq!(c.codes()[0], Some("H2"));
            assert_eq!(c.codes()[1], Some(mode.colors()[0].code));
            let rgba = image::load_from_memory(&c.encode_png_8x().unwrap())
                .unwrap()
                .to_rgba8();
            assert_eq!(rgba.get_pixel(0, 0).0[3], 255);
            assert_eq!(rgba.get_pixel(16, 0).0[3], 0);
            let before = c.codes().to_vec();
            assert!(
                c.paint(&[stamp(0, 0, "-"), stamp(2, 0, "invalid")], BrushSize::One)
                    .is_err()
            );
            assert_eq!(c.codes(), before);
            c.paint(&[stamp(0, 0, "-")], BrushSize::Two).unwrap();
            assert!(c.codes()[..2].iter().all(Option::is_none));
            assert_eq!(c.codes()[4], None);
        }
    }

    #[test]
    fn shapes_and_flood_record_codes_on_empty_canvas() {
        let mut c = Canvas::new_empty(4, 4, PaletteMode::Colors221).unwrap();
        c.paint_rect(0, 0, 1, 1, "H7", true, BrushSize::One)
            .unwrap();
        assert_eq!(c.codes()[0], Some("H7"));
        assert_eq!(c.codes()[5], Some("H7"));
        c.flood_fill(3, 3, "H2").unwrap();
        assert_eq!(c.codes()[15], Some("H2"));
        assert_eq!(c.codes()[0], Some("H7"));
        assert!(c.codes().iter().all(Option::is_some));
    }

    #[test]
    fn shapes_sync_codes_for_pattern_on_white_canvas() {
        let mut c = Canvas::new(8, 8, PaletteMode::Colors221).unwrap();
        let written = c.paint_line(0, 0, 7, 0, "F5", BrushSize::One).unwrap();
        assert_eq!(written, 8);
        assert_eq!(c.codes()[..8], [Some("F5"); 8][..]);
        assert_eq!(c.codes()[8], Some("H2"));
        let f5 = c.codes().iter().filter(|code| **code == Some("F5")).count();
        let h2 = c.codes().iter().filter(|code| **code == Some("H2")).count();
        assert_eq!((f5, h2), (8, 56));
    }

    #[test]
    fn distinct_codes_are_not_recovered_from_rgb() {
        let mut c = Canvas::new_empty(128, 2, PaletteMode::Colors221).unwrap();
        for (i, color) in PaletteMode::Colors221.colors().iter().enumerate() {
            c.paint(
                &[stamp(i as u32 % 128, i as u32 / 128, color.code)],
                BrushSize::One,
            )
            .unwrap();
            assert_eq!(c.codes()[i], Some(color.code));
        }
    }

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
                    color: "中国红".into(),
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
                    color: "纯黑".into(),
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
                        color: "纯黑".into(),
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
                        color: "纯黑".into(),
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
        assert_eq!(canvas.pixels[0], [0xFC, 0x3D, 0x46]);
        canvas
            .paint(
                &[PixelUpdate {
                    x: 1,
                    y: 1,
                    color: "纯白".into(),
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
                    color: "纯黑".into(),
                }],
                BrushSize::One,
            )
            .unwrap();
        let png = canvas.encode_png_8x().unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(7, 7), &Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(8, 0), &Rgb([254, 255, 255]));
    }

    fn stamp(x: u32, y: u32, color: &str) -> PixelUpdate {
        PixelUpdate {
            x,
            y,
            color: color.into(),
        }
    }

    #[test]
    fn undo_restores_previous_states() {
        let mut canvas = Canvas::new(4, 4, PaletteMode::Colors221).unwrap();
        assert_eq!(canvas.undo_len(), 0);
        assert!(matches!(canvas.undo(1), Err(CanvasError::EmptyUndo)));

        canvas
            .paint(&[stamp(0, 0, "正红")], BrushSize::One)
            .unwrap();
        canvas
            .paint(&[stamp(1, 0, "纯黑")], BrushSize::One)
            .unwrap();
        assert_eq!(canvas.undo_len(), 2);
        assert_eq!(canvas.codes()[0], Some("F2"));
        assert_eq!(canvas.codes()[1], Some("H7"));

        let remaining = canvas.undo(1).unwrap();
        assert_eq!(remaining, 1);
        assert_eq!(canvas.codes()[0], Some("F2"));
        assert_eq!(canvas.codes()[1], Some("H2"));
        canvas.undo(1).unwrap();
        assert_eq!(canvas.codes()[0], Some("H2"));
        assert!(matches!(canvas.undo(1), Err(CanvasError::EmptyUndo)));
    }

    #[test]
    fn undo_caps_history_and_collapse_merges_batch() {
        let mut canvas = Canvas::new(16, 16, PaletteMode::Colors221).unwrap();
        for i in 0..(MAX_UNDO + 5) {
            let x = (i % 16) as u32;
            let y = ((i / 16) % 16) as u32;
            canvas
                .paint(&[stamp(x, y, "正红")], BrushSize::One)
                .unwrap();
        }
        assert_eq!(canvas.undo_len(), MAX_UNDO);

        let mut batch = Canvas::new(8, 8, PaletteMode::Colors221).unwrap();
        let mark = batch.undo_mark();
        batch.paint(&[stamp(0, 0, "正红")], BrushSize::One).unwrap();
        batch.paint(&[stamp(1, 0, "纯黑")], BrushSize::One).unwrap();
        batch.paint(&[stamp(2, 0, "明黄")], BrushSize::One).unwrap();
        assert_eq!(batch.undo_len(), 3);
        batch.collapse_undo_to(mark);
        assert_eq!(batch.undo_len(), 1);
        batch.undo(1).unwrap();
        assert_eq!(batch.codes()[0], Some("H2"));
        assert_eq!(batch.codes()[1], Some("H2"));
        assert_eq!(batch.codes()[2], Some("H2"));
    }

    #[test]
    fn curve_paints_through_control_points() {
        let mut canvas = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        let written = canvas
            .paint_curve(&[(0, 8), (4, 0), (8, 8), (12, 0)], "纯黑", BrushSize::One)
            .unwrap();
        assert!(written > 10);
        // Cubic Bezier: start P0 (0,8), end P1 (12,0).
        assert_eq!(canvas.codes()[8 * 16], Some("H7"));
        assert_eq!(canvas.codes()[12], Some("H7"));
        assert!(canvas.undo_len() == 1);

        let err = canvas
            .paint_curve(&[(0, 0)], "纯黑", BrushSize::One)
            .unwrap_err();
        assert!(matches!(err, CanvasError::InvalidOp { .. }));
        let err = canvas
            .paint_curve(&[(0, 0), (99, 0)], "纯黑", BrushSize::One)
            .unwrap_err();
        assert!(matches!(err, CanvasError::OutOfBounds { .. }));
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
        let red = [0xFC, 0x3D, 0x46];
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
            .paint_line(1, 1, 5, 1, "纯黑", BrushSize::Two)
            .unwrap();
        // Horizontal centerline y=1, brush 2 → rows 1..=2 (top-left of each stamp).
        assert!(written >= 10);
        let black = [0, 0, 0];
        let white = PaletteMode::Colors24.white_rgb();
        assert_eq!(canvas.pixels[9], black); // (1,1)
        assert_eq!(canvas.pixels[10], black); // (2,1)
        assert_eq!(canvas.pixels[8 + 2], black); // (2,2) below centerline
        assert_eq!(canvas.pixels[1], white); // (1,0) not covered
    }

    #[test]
    fn brush_footprint_matches_between_pixels_and_stroke() {
        let mut via_pixels = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        via_pixels
            .paint(&[stamp(2, 2, "纯黑")], BrushSize::Two)
            .unwrap();
        let mut via_stroke = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        via_stroke
            .paint_line(2, 2, 2, 2, "纯黑", BrushSize::Two)
            .unwrap();
        assert_eq!(via_pixels.pixels, via_stroke.pixels);
        assert_eq!(via_pixels.codes, via_stroke.codes);

        let mut via_pixels4 = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        via_pixels4
            .paint(&[stamp(0, 0, "纯黑")], BrushSize::Four)
            .unwrap();
        let mut via_stroke4 = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        via_stroke4
            .paint_line(0, 0, 0, 0, "纯黑", BrushSize::Four)
            .unwrap();
        assert_eq!(via_pixels4.pixels, via_stroke4.pixels);
    }

    #[test]
    fn huge_radius_is_capped_without_hanging() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        let before = canvas.undo_len();
        let written = canvas
            .paint_circle(4, 4, u32::MAX, "纯黑", true, BrushSize::One)
            .unwrap();
        assert!(written > 0);
        assert_eq!(canvas.pixels[4 * 8 + 4], [0, 0, 0]);
        assert_eq!(canvas.undo_len(), before + 1);

        let mut stroke = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        let w = stroke
            .paint_ellipse(4, 4, 3_000_000_000, 3_000_000_000, "纯黑", true, BrushSize::One)
            .unwrap();
        assert!(w > 0);
        assert_eq!(stroke.pixels[4 * 8 + 4], [0, 0, 0]);
    }

    #[test]
    fn fully_clipped_stroke_records_no_undo() {
        let mut canvas = Canvas::new(4, 4, PaletteMode::Colors24).unwrap();
        let before = canvas.undo_len();
        let written = canvas
            .paint_circle(1, 1, 50, "纯黑", false, BrushSize::One)
            .unwrap();
        assert_eq!(written, 0);
        assert_eq!(canvas.undo_len(), before);
    }

    #[test]
    fn flood_fill_dash_clears_to_empty() {
        let mut canvas = Canvas::new(6, 6, PaletteMode::Colors24).unwrap();
        canvas
            .paint_rect(1, 1, 4, 4, "纯黑", true, BrushSize::One)
            .unwrap();
        assert_eq!(canvas.codes[2 * 6 + 2], Some("H7"));
        let cleared = canvas.flood_fill(2, 2, "-").unwrap();
        assert_eq!(cleared, 16);
        assert_eq!(canvas.codes[2 * 6 + 2], None);
        assert_eq!(canvas.codes[0], Some("H2"));
        // White beads are a distinct region: clearing them empties that region.
        assert_eq!(canvas.flood_fill(0, 0, "-").unwrap(), 20);
        assert_eq!(canvas.codes[0], None);
        // Clearing already-empty is a no-op and records no undo.
        let undo_before = canvas.undo_len();
        assert_eq!(canvas.flood_fill(0, 0, "-").unwrap(), 0);
        assert_eq!(canvas.undo_len(), undo_before);
    }

    #[test]
    fn rect_fill_and_stroke_differ() {
        let mut canvas = Canvas::new(6, 6, PaletteMode::Colors24).unwrap();
        canvas
            .paint_rect(1, 1, 3, 3, "中国红", true, BrushSize::One)
            .unwrap();
        let red = [0xE7, 0x00, 0x2F];
        let white = PaletteMode::Colors24.white_rgb();
        assert_eq!(canvas.pixels[7], red);
        assert_eq!(canvas.pixels[2 * 6 + 2], red);
        assert_eq!(canvas.pixels[0], white);

        let mut stroke = Canvas::new(6, 6, PaletteMode::Colors24).unwrap();
        stroke
            .paint_rect(1, 1, 3, 3, "纯黑", false, BrushSize::One)
            .unwrap();
        assert_eq!(stroke.pixels[7], [0, 0, 0]);
        assert_eq!(stroke.pixels[2 * 6 + 2], white);
        assert_eq!(stroke.pixels[9], [0, 0, 0]);
    }

    #[test]
    fn triangle_fill_and_stroke_differ() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        canvas
            .paint_triangle(1, 1, 6, 1, 1, 6, "中国红", true, BrushSize::One)
            .unwrap();
        let red = [0xE7, 0x00, 0x2F];
        let white = PaletteMode::Colors24.white_rgb();
        assert_eq!(canvas.pixels[9], red);
        assert_eq!(canvas.pixels[2 * 8 + 2], red);
        assert_eq!(canvas.pixels[7 * 8 + 7], white);

        let mut stroke = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        stroke
            .paint_triangle(1, 1, 6, 1, 1, 6, "纯黑", false, BrushSize::One)
            .unwrap();
        assert_eq!(stroke.pixels[9], [0, 0, 0]);
        assert_eq!(stroke.pixels[14], [0, 0, 0]);
        assert_eq!(stroke.pixels[6 * 8 + 1], [0, 0, 0]);
        assert_eq!(stroke.pixels[3 * 8 + 3], white);
    }

    #[test]
    fn circle_and_ellipse_fill_and_flood() {
        let mut canvas = Canvas::new(9, 9, PaletteMode::Colors24).unwrap();
        canvas
            .paint_circle(4, 4, 2, "深蓝", true, BrushSize::One)
            .unwrap();
        let blue = [0x0F, 0x54, 0xC0];
        let yellow = [0xFB, 0xED, 0x56];
        assert_eq!(canvas.pixels[4 * 9 + 4], blue);
        canvas.flood_fill(0, 0, "明黄").unwrap();
        assert_eq!(canvas.pixels[0], yellow);
        assert_eq!(canvas.pixels[4 * 9 + 4], blue);

        let mut ellipse = Canvas::new(9, 9, PaletteMode::Colors24).unwrap();
        ellipse
            .paint_ellipse(4, 4, 3, 1, "纯黑", true, BrushSize::One)
            .unwrap();
        assert_eq!(ellipse.pixels[4 * 9 + 4], [0, 0, 0]);
        assert_eq!(ellipse.pixels[4 * 9 + 1], [0, 0, 0]);
        assert_eq!(ellipse.pixels[4], PaletteMode::Colors24.white_rgb());
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

    // --- Drawing quality on the canvas: gaps and unnecessary bulk ---

    use std::collections::HashSet;

    fn black() -> [u8; 3] {
        [0, 0, 0]
    }

    fn cells_with(canvas: &Canvas, rgb: [u8; 3]) -> HashSet<(u32, u32)> {
        canvas
            .pixels
            .iter()
            .enumerate()
            .filter(|(_, p)| **p == rgb)
            .map(|(i, _)| ((i as u32) % canvas.width(), (i as u32) / canvas.width()))
            .collect()
    }

    fn is_8_connected_region(cells: &HashSet<(u32, u32)>) -> bool {
        let Some(&start) = cells.iter().next() else {
            return true;
        };
        let mut seen = HashSet::from([start]);
        let mut stack = vec![start];
        while let Some((x, y)) = stack.pop() {
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 {
                        continue;
                    }
                    let next = (nx as u32, ny as u32);
                    if cells.contains(&next) && seen.insert(next) {
                        stack.push(next);
                    }
                }
            }
        }
        seen.len() == cells.len()
    }

    /// Paint a line with brush 1 and assert it is a thin connected path:
    /// exact cell count, 8-connected, and no two cells sharing a major coordinate.
    fn assert_line_paints_thin(canvas_size: u32, x0: u32, y0: u32, x1: u32, y1: u32) {
        let mut canvas = Canvas::new(canvas_size, canvas_size, PaletteMode::Colors24).unwrap();
        let written = canvas
            .paint_line(x0, y0, x1, y1, "纯黑", BrushSize::One)
            .unwrap();
        let cells = cells_with(&canvas, black());
        let dx = x1.abs_diff(x0);
        let dy = y1.abs_diff(y0);
        assert_eq!(
            written,
            dx.max(dy) as usize + 1,
            "line {x0},{y0}->{x1},{y1} painted extra cells"
        );
        assert_eq!(cells.len(), written);
        assert!(cells.contains(&(x0, y0)) && cells.contains(&(x1, y1)));
        assert!(
            is_8_connected_region(&cells),
            "line {x0},{y0}->{x1},{y1} has a gap: {cells:?}"
        );
        if dx >= dy {
            let mut xs: Vec<u32> = cells.iter().map(|c| c.0).collect();
            xs.sort_unstable();
            let n = xs.len();
            xs.dedup();
            assert_eq!(xs.len(), n, "line {x0},{y0}->{x1},{y1} is thicker than 1px in y");
        } else {
            let mut ys: Vec<u32> = cells.iter().map(|c| c.1).collect();
            ys.sort_unstable();
            let n = ys.len();
            ys.dedup();
            assert_eq!(ys.len(), n, "line {x0},{y0}->{x1},{y1} is thicker than 1px in x");
        }
    }

    #[test]
    fn lines_in_all_directions_paint_without_gaps_or_bulk() {
        // Horizontal / vertical / both diagonals / shallow / steep, both directions.
        let segments = [
            (0, 0, 15, 0),
            (15, 0, 0, 0),
            (0, 15, 0, 0),
            (0, 0, 0, 15),
            (0, 0, 15, 15),
            (15, 15, 0, 0),
            (15, 0, 0, 15),
            (0, 15, 15, 0),
            (0, 0, 15, 2),
            (15, 2, 0, 0),
            (0, 15, 2, 0),
            (2, 0, 0, 15),
            (0, 0, 15, 3),
            (0, 0, 3, 15),
            (1, 1, 14, 7),
            (14, 1, 1, 7),
            (1, 14, 7, 1),
            (1, 1, 7, 14),
            (4, 4, 12, 4),
            (4, 4, 4, 12),
        ];
        for &(x0, y0, x1, y1) in &segments {
            assert_line_paints_thin(16, x0, y0, x1, y1);
        }
    }

    #[test]
    fn horizontal_line_does_not_bleed_into_neighbor_rows() {
        let mut canvas = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        canvas
            .paint_line(2, 8, 12, 8, "纯黑", BrushSize::One)
            .unwrap();
        let cells = cells_with(&canvas, black());
        assert_eq!(cells.len(), 11);
        assert!(cells.iter().all(|&(_, y)| y == 8), "vertical bleed: {cells:?}");
    }

    #[test]
    fn vertical_line_does_not_bleed_into_neighbor_columns() {
        let mut canvas = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        canvas
            .paint_line(6, 1, 6, 11, "纯黑", BrushSize::One)
            .unwrap();
        let cells = cells_with(&canvas, black());
        assert_eq!(cells.len(), 11);
        assert!(cells.iter().all(|&(x, _)| x == 6), "horizontal bleed: {cells:?}");
    }

    #[test]
    fn brush_one_stamps_exactly_one_cell() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        canvas
            .paint(&[stamp(3, 4, "纯黑")], BrushSize::One)
            .unwrap();
        let cells = cells_with(&canvas, black());
        assert_eq!(cells, HashSet::from([(3, 4)]));
    }

    #[test]
    fn stroke_shapes_leave_interior_unpainted() {
        let mut rect = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        rect.paint_rect(2, 2, 12, 10, "纯黑", false, BrushSize::One)
            .unwrap();
        let rect_cells = cells_with(&rect, black());
        assert!(!rect_cells.contains(&(5, 5)), "rect stroke filled interior");
        assert!(rect_cells.contains(&(2, 2)) && rect_cells.contains(&(12, 10)));
        assert!(is_8_connected_region(&rect_cells));

        let mut circle = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        circle
            .paint_circle(8, 8, 5, "纯黑", false, BrushSize::One)
            .unwrap();
        let circle_cells = cells_with(&circle, black());
        assert!(!circle_cells.contains(&(8, 8)), "circle stroke filled center");
        assert!(!circle_cells.contains(&(8, 9)), "circle stroke filled interior");
        assert!(is_8_connected_region(&circle_cells), "circle ring has gaps");

        let mut ellipse = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        ellipse
            .paint_ellipse(8, 8, 6, 3, "纯黑", false, BrushSize::One)
            .unwrap();
        let ellipse_cells = cells_with(&ellipse, black());
        assert!(!ellipse_cells.contains(&(8, 8)), "ellipse stroke filled center");
        assert!(is_8_connected_region(&ellipse_cells));

        let mut triangle = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        triangle
            .paint_triangle(2, 2, 13, 2, 2, 13, "纯黑", false, BrushSize::One)
            .unwrap();
        let tri_cells = cells_with(&triangle, black());
        assert!(!tri_cells.contains(&(4, 4)), "triangle stroke filled interior");
        assert!(is_8_connected_region(&tri_cells));
    }

    #[test]
    fn fills_cover_their_strokes_without_out_of_bounds() {
        let mut canvas = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        canvas
            .paint_rect(3, 3, 10, 9, "纯黑", true, BrushSize::One)
            .unwrap();
        let cells = cells_with(&canvas, black());
        assert_eq!(cells.len(), 8 * 7);
        assert!(cells.iter().all(|&(x, y)| (3..=10).contains(&x) && (3..=9).contains(&y)));

        let mut circle = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        circle
            .paint_circle(8, 8, 4, "纯黑", true, BrushSize::One)
            .unwrap();
        let disk = cells_with(&circle, black());
        assert!(disk.contains(&(8, 8)));
        assert!(disk.len() > 20);
        assert!(disk.iter().all(|&(x, y)| x.abs_diff(8) <= 4 && y.abs_diff(8) <= 4));
    }

    #[test]
    fn curves_paint_contiguous_paths() {
        let cases: [&[(u32, u32)]; 3] = [
            &[(1, 12), (8, 1), (15, 12)],
            &[(1, 15), (1, 1), (15, 1), (15, 15)],
            &[(1, 8), (5, 1), (11, 15), (15, 8), (12, 2)],
        ];
        for points in cases {
            let mut canvas = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
            canvas
                .paint_curve(points, "纯黑", BrushSize::One)
                .unwrap();
            let cells = cells_with(&canvas, black());
            assert!(cells.contains(&points[0]), "curve missing start");
            assert!(cells.contains(&points[points.len() - 1]), "curve missing end");
            assert!(
                is_8_connected_region(&cells),
                "curve {points:?} has a gap ({ } cells)",
                cells.len()
            );
        }
    }

    #[test]
    fn flood_fill_stays_inside_closed_boundaries() {
        let mut canvas = Canvas::new(12, 12, PaletteMode::Colors24).unwrap();
        // Hollow rectangle border in black on white background.
        canvas
            .paint_rect(2, 2, 9, 9, "纯黑", false, BrushSize::One)
            .unwrap();
        let filled = canvas.flood_fill(5, 5, "正红").unwrap();
        let red = palette::resolve_color(PaletteMode::Colors24, "正红").unwrap();
        let red_cells = cells_with(&canvas, red.rgb);
        // Interior of 8×8 border is 6×6 = 36 cells.
        assert_eq!(filled, 36);
        assert_eq!(red_cells.len(), 36);
        assert!(red_cells.iter().all(|&(x, y)| (3..=8).contains(&x) && (3..=8).contains(&y)));
        // Exterior white and the black border stay untouched.
        assert_eq!(canvas.codes()[0], Some("H2"));
        assert_eq!(canvas.codes()[2 * 12 + 2], Some("H7"));
    }
}
