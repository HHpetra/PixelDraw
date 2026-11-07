use std::io::Cursor;

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba, imageops::FilterType};

use crate::motif;
use crate::palette::{self, Color, PaletteMode};
use crate::shapes;

pub const MAX_SIZE: u32 = 128;
pub const SCALE: u32 = 8;
/// Diameters and ellipse axes larger than this are capped before rasterization.
/// Same order of magnitude as the old radius cap of 256 (`diameter = 2×radius + 1`).
pub const MAX_AXIS: u32 = 512;
/// Diamond half-axes larger than this are capped before rasterization.
/// Width and height are `2*r+1`, so 255 stays inside the 512 scale (max side 511).
pub const MAX_DIAMOND_HALF_AXIS: u32 = 255;

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

/// Result of [`Canvas::move_region`] or [`Canvas::copy_region`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionOutcome {
    /// False when the transplant would not change any cell.
    pub changed: bool,
    /// Cells whose bead code changed. Zero when `changed` is false.
    pub written: usize,
    /// Normalized inclusive source bounds.
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
    /// Inclusive destination bounds.
    pub nx0: u32,
    pub ny0: u32,
    pub nx1: u32,
    pub ny1: u32,
}

/// Maximum number of undo snapshots kept per canvas.
pub const MAX_UNDO: usize = 64;

#[derive(Debug, Clone)]
struct Snapshot {
    pixels: Vec<[u8; 3]>,
    codes: Vec<Option<&'static str>>,
    /// Remembered background bead code. `None` is an empty canvas (no beads).
    background: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct Canvas {
    width: u32,
    height: u32,
    mode: PaletteMode,
    pixels: Vec<[u8; 3]>,
    codes: Vec<Option<&'static str>>,
    /// Bead code last chosen as the canvas background.
    ///
    /// `Some("H2")` for the default white canvas. `None` for `background: empty`
    /// until [`Canvas::fill_background`] sets a color. Drawing tools do not change it.
    background: Option<&'static str>,
    undo: Vec<Snapshot>,
    /// Pre-batch state, held while [`Canvas::begin_batch`] is active so the
    /// whole batch collapses into one undo step on [`Canvas::end_batch`].
    pending_batch: Option<Snapshot>,
    /// Whether this batch changed any cell or the remembered background color.
    batch_wrote: bool,
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
            background: Some("H2"),
            undo: Vec::new(),
            pending_batch: None,
            batch_wrote: false,
        })
    }

    pub fn new_empty(width: u32, height: u32, mode: PaletteMode) -> Result<Self, CanvasError> {
        let mut canvas = Self::new(width, height, mode)?;
        canvas.codes.fill(None);
        canvas.background = None;
        Ok(canvas)
    }

    /// Fill a new canvas with one palette color and remember it as the background.
    ///
    /// Size is checked before the color, matching [`Canvas::new`]. An unknown color
    /// does not produce a canvas. The result has an empty undo history.
    pub fn new_with_color(
        width: u32,
        height: u32,
        mode: PaletteMode,
        color: &str,
    ) -> Result<Self, CanvasError> {
        let mut canvas = Self::new(width, height, mode)?;
        let fill =
            palette::resolve_color(mode, color).ok_or_else(|| CanvasError::UnknownColor {
                color: color.to_string(),
                mode,
            })?;
        canvas.pixels.fill(fill.rgb);
        canvas.codes.fill(Some(fill.code));
        canvas.background = Some(fill.code);
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

    /// Remembered background bead code, or `None` when the canvas is empty.
    pub fn background_code(&self) -> Option<&'static str> {
        self.background
    }

    /// `雪白（H2）` for a bead background, or `空格` when none is set.
    pub fn background_label(&self) -> String {
        match self.background_code() {
            None => "空格".to_string(),
            Some(code) => match palette::resolve_color(self.mode, code) {
                Some(color) => format!("{}（{}）", color.name, color.code),
                None => code.to_string(),
            },
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            pixels: self.pixels.clone(),
            codes: self.codes.clone(),
            background: self.background,
        }
    }

    /// Record the pre-write state as one undo step.
    ///
    /// Inside a batch, ops defer to the batch snapshot taken by
    /// [`Canvas::begin_batch`]; the step is pushed once in [`Canvas::end_batch`].
    fn push_undo(&mut self) {
        if self.pending_batch.is_some() {
            return;
        }
        self.push_snapshot(self.snapshot());
    }

    fn push_snapshot(&mut self, snapshot: Snapshot) {
        self.undo.push(snapshot);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    /// Drop the undo step pushed for a write that ended up painting nothing.
    /// Inside a batch the snapshot is committed only at [`Canvas::end_batch`].
    fn discard_undo(&mut self) {
        if self.pending_batch.is_some() {
            return;
        }
        self.undo.pop();
    }

    /// Number of undo steps currently available.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Begin an atomic multi-op drawing step.
    ///
    /// Captures the current state once; the writes that follow join it into a
    /// single undo step when [`Canvas::end_batch`] runs. Taking the snapshot up
    /// front (instead of merging afterwards) means the pre-batch state cannot be
    /// evicted by the batch itself when the history is already full.
    ///
    /// [`Canvas::end_batch`]: Self::end_batch
    pub fn begin_batch(&mut self) {
        debug_assert!(self.pending_batch.is_none(), "nested batches");
        self.pending_batch = Some(self.snapshot());
        self.batch_wrote = false;
    }

    /// End an atomic multi-op drawing step started by [`Canvas::begin_batch`].
    /// Commits one undo step only when the batch changed cells or the remembered
    /// background color.
    ///
    /// [`Canvas::begin_batch`]: Self::begin_batch
    pub fn end_batch(&mut self) {
        let Some(snapshot) = self.pending_batch.take() else {
            return;
        };
        if self.batch_wrote {
            self.push_snapshot(snapshot);
        }
        self.batch_wrote = false;
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
            self.background = snap.background;
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
        self.batch_wrote = true;
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
    /// `-` is not a shape color; motif placement accepts it in [`Self::paint_motif`].
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
        Ok(self.stamp_points(points, Some(color), brush.size()))
    }

    /// Stamp `stamp`×`stamp` blocks on pre-rasterized points.
    ///
    /// `None` clears those cells to empty (not an H2 bead). Blocks that fall
    /// outside the canvas are clipped. One undo step is recorded only when at
    /// least one cell is written. The remembered background color is unchanged.
    /// The returned count is distinct cells, not `points × stamp²`.
    fn stamp_points(&mut self, points: &[(i32, i32)], color: Option<&Color>, stamp: u32) -> usize {
        if points.is_empty() {
            return 0;
        }
        self.push_undo();
        let mut touched = vec![false; self.pixels.len()];
        let mut written = 0;
        for &(x, y) in points {
            written += self.stamp_block(x, y, color, stamp, &mut touched);
        }
        if written == 0 {
            self.discard_undo();
        }
        written
    }

    /// Place one embedded motif at the top-left `(x, y)` of its bounding box.
    ///
    /// `scale` defaults to 1 and must be an integer from 1 to 8. Each occupied
    /// cell becomes a solid `scale`×`scale` block; gaps in the bitmap are not
    /// painted. The anchor must lie on the canvas. Blocks past the edge are
    /// clipped. `color` of `-` clears cells. An unknown name, a bad dice face,
    /// a stray face, a bad scale, an anchor outside the canvas, or an unknown
    /// color rejects the call before any cell is written.
    pub fn paint_motif(
        &mut self,
        name: &str,
        x: u32,
        y: u32,
        color: &str,
        scale: Option<u32>,
        face: Option<u32>,
    ) -> Result<usize, CanvasError> {
        let pattern = motif::motif_points(name, face)
            .map_err(|message| CanvasError::InvalidOp { message })?;
        let scale = match scale {
            None => 1,
            Some(scale) if (1..=8).contains(&scale) => scale,
            Some(scale) => {
                return Err(CanvasError::InvalidOp {
                    message: format!("放大倍数 scale 必须是 1 到 8 的整数，当前为 {scale}。"),
                });
            }
        };
        self.require_point(x, y)?;
        let paint = if color.trim() == "-" {
            None
        } else {
            Some(palette::resolve_color(self.mode, color).ok_or_else(|| {
                CanvasError::UnknownColor {
                    color: color.to_string(),
                    mode: self.mode,
                }
            })?)
        };
        let origin_x = i32::try_from(x).expect("canvas x fits i32");
        let origin_y = i32::try_from(y).expect("canvas y fits i32");
        let step = i32::try_from(scale).expect("motif scale fits i32");
        let points: Vec<(i32, i32)> = pattern
            .cells
            .iter()
            .map(|&(dx, dy)| {
                (
                    origin_x + i32::from(dx) * step,
                    origin_y + i32::from(dy) * step,
                )
            })
            .collect();
        Ok(self.stamp_points(&points, paint, scale))
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

    /// Pixel circle anchored at the top-left `(x, y)` of a `diameter`×`diameter` box.
    /// `diameter` must be >= 1; values above [`MAX_AXIS`] are clamped.
    /// The anchor must lie on the canvas. Cells past the right or bottom edge are clipped.
    pub fn paint_circle(
        &mut self,
        x: u32,
        y: u32,
        diameter: u32,
        color: &str,
        fill: bool,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        self.require_point(x, y)?;
        if diameter == 0 {
            return Err(CanvasError::InvalidOp {
                message: "直径必须是大于等于 1 的整数。".into(),
            });
        }
        let diameter = diameter.min(MAX_AXIS) as i32;
        let points = if fill {
            shapes::circle_fill_points(x as i32, y as i32, diameter)
        } else {
            shapes::circle_stroke_points(x as i32, y as i32, diameter)
        };
        let brush = if fill { BrushSize::One } else { brush };
        self.paint_shape_points(&points, color, brush)
    }

    /// Pixel ellipse anchored at the top-left `(x, y)` of a `width`×`height` box.
    /// `width` and `height` must be >= 1; values above [`MAX_AXIS`] are clamped.
    #[allow(clippy::too_many_arguments)] // Coordinates mirror the drawing tool parameters.
    pub fn paint_ellipse(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        color: &str,
        fill: bool,
        brush: BrushSize,
    ) -> Result<usize, CanvasError> {
        self.require_point(x, y)?;
        if width == 0 || height == 0 {
            return Err(CanvasError::InvalidOp {
                message: "椭圆的宽和高必须是大于等于 1 的整数。".into(),
            });
        }
        let width = width.min(MAX_AXIS) as i32;
        let height = height.min(MAX_AXIS) as i32;
        let points = if fill {
            shapes::ellipse_fill_points(x as i32, y as i32, width, height)
        } else {
            shapes::ellipse_stroke_points(x as i32, y as i32, width, height)
        };
        let brush = if fill { BrushSize::One } else { brush };
        self.paint_shape_points(&points, color, brush)
    }

    /// Pixel diamond centered on cell `(cx, cy)`.
    ///
    /// `rx` and `ry` are diamond half-axes. Values above [`MAX_DIAMOND_HALF_AXIS`]
    /// are clamped. Width and height are `2*rx+1` and `2*ry+1`. The center must
    /// lie on the canvas. Cells past any edge are clipped.
    #[allow(clippy::too_many_arguments)] // Coordinates mirror the drawing tool parameters.
    pub fn paint_diamond(
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
        let rx = rx.min(MAX_DIAMOND_HALF_AXIS) as i32;
        let ry = ry.min(MAX_DIAMOND_HALF_AXIS) as i32;
        let points = if fill {
            shapes::diamond_fill_points(cx as i32, cy as i32, rx, ry)
        } else {
            shapes::diamond_stroke_points(cx as i32, cy as i32, rx, ry)
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
            self.discard_undo();
        }
        Ok(written)
    }

    /// Replace every cell with `color`, overwriting beads and empty cells alike.
    ///
    /// The color is resolved before any cell is written. Unlike [`Self::flood_fill`],
    /// this does not stop at a region of the same bead code. The remembered
    /// background color becomes `color` whenever this fill is applied, including
    /// when every cell was already that color but the remembered color differed.
    /// Records one undo step when any cell changes or the remembered color changes.
    /// Returns how many cells changed. A canvas that is already entirely `color`
    /// and already remembers that color is left unchanged.
    pub fn fill_background(&mut self, color: &str) -> Result<usize, CanvasError> {
        let fill =
            palette::resolve_color(self.mode, color).ok_or_else(|| CanvasError::UnknownColor {
                color: color.to_string(),
                mode: self.mode,
            })?;
        let new_code = Some(fill.code);
        let changed = self.codes.iter().filter(|code| **code != new_code).count();
        let background_changed = self.background != new_code;
        if changed == 0 && !background_changed {
            return Ok(0);
        }
        self.push_undo();
        if changed > 0 {
            for index in 0..self.pixels.len() {
                self.write_cell(index, Some(fill));
            }
        }
        self.background = new_code;
        // A metadata-only update never calls `write_cell`, but the batch still
        // has to keep the pre-change snapshot so undo can restore the old color.
        if background_changed {
            self.batch_wrote = true;
        }
        Ok(changed)
    }

    /// Move the inclusive source rectangle onto a new position.
    ///
    /// The block, including empty cells, is read before the source is filled
    /// with the current background (spaces when none is set) and the block is
    /// written at the destination, so an overlapping move stays correct. The
    /// remembered background color is not changed.
    ///
    /// Pass exactly one address pair: relative `(dx, dy)`, or absolute
    /// destination top-left `(tx, ty)`. A source endpoint or a destination
    /// rectangle outside the canvas is rejected and the canvas is left
    /// unchanged. When the move would not change any cell — a `(0, 0)` offset,
    /// an empty block moved onto empty cells, or a background-colored block
    /// moved onto the same background — nothing is written and no undo step is
    /// recorded.
    #[allow(clippy::too_many_arguments)] // Coordinates mirror the drawing tool parameters.
    pub fn move_region(
        &mut self,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        dx: Option<i32>,
        dy: Option<i32>,
        tx: Option<i32>,
        ty: Option<i32>,
    ) -> Result<RegionOutcome, CanvasError> {
        self.transplant_region(x0, y0, x1, y1, dx, dy, tx, ty, true)
    }

    /// Copy the inclusive source rectangle onto a new position.
    ///
    /// Addressing, bounds, and the zero-change rule match [`Self::move_region`].
    /// The source stays; empty cells in the block overwrite the destination.
    /// The remembered background color is not changed.
    #[allow(clippy::too_many_arguments)] // Coordinates mirror the drawing tool parameters.
    pub fn copy_region(
        &mut self,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        dx: Option<i32>,
        dy: Option<i32>,
        tx: Option<i32>,
        ty: Option<i32>,
    ) -> Result<RegionOutcome, CanvasError> {
        self.transplant_region(x0, y0, x1, y1, dx, dy, tx, ty, false)
    }

    #[allow(clippy::too_many_arguments)] // Coordinates mirror the drawing tool parameters.
    fn transplant_region(
        &mut self,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        dx: Option<i32>,
        dy: Option<i32>,
        tx: Option<i32>,
        ty: Option<i32>,
        erase_source: bool,
    ) -> Result<RegionOutcome, CanvasError> {
        let address = match (dx, dy, tx, ty) {
            (Some(dx), Some(dy), None, None) => RegionAddress::Delta(dx, dy),
            (None, None, Some(tx), Some(ty)) => RegionAddress::Origin(tx, ty),
            _ => {
                return Err(CanvasError::InvalidOp {
                    message: "「dx,dy」与「tx,ty」只能给一组：相对位移填 dx/dy，绝对目标填 tx/ty。"
                        .into(),
                });
            }
        };
        self.require_point(x0, y0)?;
        self.require_point(x1, y1)?;
        let (left, top, right, bottom) =
            shapes::rect_bounds(x0 as i32, y0 as i32, x1 as i32, y1 as i32);
        let sx0 = left as u32;
        let sy0 = top as u32;
        let sx1 = right as u32;
        let sy1 = bottom as u32;
        let w = sx1 - sx0 + 1;
        let h = sy1 - sy0 + 1;
        let (origin_x, origin_y) = match address {
            RegionAddress::Delta(dx, dy) => (
                i64::from(left) + i64::from(dx),
                i64::from(top) + i64::from(dy),
            ),
            RegionAddress::Origin(tx, ty) => (i64::from(tx), i64::from(ty)),
        };
        let fits = origin_x >= 0
            && origin_y >= 0
            && origin_x + i64::from(w) <= i64::from(self.width)
            && origin_y + i64::from(h) <= i64::from(self.height);
        if !fits {
            return Err(CanvasError::InvalidOp {
                message: format!(
                    "目标区域越界：起点 ({origin_x}, {origin_y}) 加尺寸 {w}x{h} 超出 0..{} x 0..{}。请调整 dx/dy 或 tx/ty。",
                    self.width, self.height
                ),
            });
        }
        let nx0 = u32::try_from(origin_x).expect("destination origin is in range");
        let ny0 = u32::try_from(origin_y).expect("destination origin is in range");
        let nx1 = nx0 + w - 1;
        let ny1 = ny0 + h - 1;

        let mut buffer = Vec::with_capacity((w as usize) * (h as usize));
        for y in sy0..=sy1 {
            for x in sx0..=sx1 {
                buffer.push(self.codes[(y * self.width + x) as usize]);
            }
        }
        let place = PlacedRegion {
            sx0,
            sy0,
            sx1,
            sy1,
            tx: nx0,
            ty: ny0,
            w,
            h,
        };
        // Read first, then erase, then write, so a one-cell shift does not
        // sample cells that this same call has already overwritten.
        let mut next = self.codes.clone();
        blit_region(
            &mut next,
            self.width,
            &buffer,
            &place,
            erase_source,
            self.background,
        );
        let outcome = RegionOutcome {
            changed: next != self.codes,
            written: next
                .iter()
                .zip(self.codes.iter())
                .filter(|(after, before)| after != before)
                .count(),
            x0: sx0,
            y0: sy0,
            x1: sx1,
            y1: sy1,
            nx0,
            ny0,
            nx1,
            ny1,
        };
        if !outcome.changed {
            return Ok(outcome);
        }
        self.push_undo();
        for (index, code) in next.into_iter().enumerate() {
            self.write_code(index, code);
        }
        Ok(outcome)
    }

    /// Write one cell from a stored bead code. Identical codes are left alone
    /// so a no-op transplant does not mark the open batch as written.
    fn write_code(&mut self, index: usize, code: Option<&'static str>) {
        if self.codes[index] == code {
            return;
        }
        let color = code.map(|code| {
            palette::resolve_color(self.mode, code).expect("canvas stores canonical palette codes")
        });
        self.write_cell(index, color);
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

enum RegionAddress {
    Delta(i32, i32),
    Origin(i32, i32),
}

struct PlacedRegion {
    sx0: u32,
    sy0: u32,
    sx1: u32,
    sy1: u32,
    tx: u32,
    ty: u32,
    w: u32,
    h: u32,
}

/// Erase `place`'s source (move only), then stamp the buffer onto the destination.
fn blit_region(
    codes: &mut [Option<&'static str>],
    width: u32,
    buffer: &[Option<&'static str>],
    place: &PlacedRegion,
    erase_source: bool,
    background: Option<&'static str>,
) {
    if erase_source {
        for y in place.sy0..=place.sy1 {
            for x in place.sx0..=place.sx1 {
                codes[(y * width + x) as usize] = background;
            }
        }
    }
    for row in 0..place.h {
        for col in 0..place.w {
            codes[((place.ty + row) * width + (place.tx + col)) as usize] =
                buffer[(row * place.w + col) as usize];
        }
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
    fn diamond_keeps_bead_state() {
        for fill in [false, true] {
            assert_drawing_state(|c, color, brush| {
                c.paint_diamond(4, 4, 3, 2, color, fill, brush).unwrap();
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
    fn undo_caps_history_and_batches_merge_into_one_step() {
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
        let blank = batch.codes().to_vec();
        batch.begin_batch();
        batch.paint(&[stamp(0, 0, "正红")], BrushSize::One).unwrap();
        batch.paint(&[stamp(1, 0, "纯黑")], BrushSize::One).unwrap();
        batch.paint(&[stamp(2, 0, "明黄")], BrushSize::One).unwrap();
        batch.end_batch();
        assert_eq!(batch.undo_len(), 1);
        batch.undo(1).unwrap();
        assert_eq!(batch.codes(), &blank[..]);
    }

    #[test]
    fn batch_undo_survives_a_full_history() {
        // The batch snapshot is taken up front, so a batch issued while the
        // history is full must still revert as a single step. Merging after the
        // fact used to lose the pre-batch state to eviction.
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors221).unwrap();
        for i in 0..MAX_UNDO {
            canvas
                .paint(
                    &[stamp((i % 8) as u32, (i / 8) as u32, "H7")],
                    BrushSize::One,
                )
                .unwrap();
        }
        assert_eq!(canvas.undo_len(), MAX_UNDO);
        let before = canvas.codes().to_vec();

        canvas.begin_batch();
        for y in 4..8 {
            canvas.paint_line(0, y, 7, y, "F5", BrushSize::One).unwrap();
        }
        canvas.end_batch();
        assert_eq!(canvas.undo_len(), MAX_UNDO);
        assert_eq!(canvas.codes()[4 * 8], Some("F5"));

        canvas.undo(1).unwrap();
        assert_eq!(
            canvas.codes(),
            &before[..],
            "one undo must revert the batch"
        );
    }

    #[test]
    fn empty_batch_records_no_undo_step() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors221).unwrap();
        canvas.begin_batch();
        // Filling the seed with the color already there writes nothing.
        canvas.flood_fill(0, 0, "H2").unwrap();
        canvas.end_batch();
        assert_eq!(canvas.undo_len(), 0);

        canvas.begin_batch();
        canvas.paint_line(0, 0, 7, 0, "H7", BrushSize::One).unwrap();
        canvas.flood_fill(0, 0, "H7").unwrap();
        canvas.end_batch();
        assert_eq!(canvas.undo_len(), 1);
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
    fn huge_diameter_is_capped_without_hanging() {
        let mut canvas = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        let before = canvas.undo_len();
        let written = canvas
            .paint_circle(0, 0, u32::MAX, "纯黑", true, BrushSize::One)
            .unwrap();
        assert!(written > 0);
        assert_eq!(canvas.undo_len(), before + 1);
        let mut capped = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        capped
            .paint_circle(0, 0, MAX_AXIS, "纯黑", true, BrushSize::One)
            .unwrap();
        assert_eq!(canvas.pixels, capped.pixels);
        let mut also = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        also.paint_circle(0, 0, MAX_AXIS + 1, "纯黑", true, BrushSize::One)
            .unwrap();
        assert_eq!(canvas.pixels, also.pixels);
        let mut smaller = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        smaller
            .paint_circle(0, 0, MAX_AXIS - 1, "纯黑", true, BrushSize::One)
            .unwrap();
        assert_ne!(canvas.pixels, smaller.pixels);

        let mut ellipse = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        ellipse
            .paint_ellipse(0, 0, 3_000_000_000, 4, "纯黑", true, BrushSize::One)
            .unwrap();
        let mut ellipse_cap = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        ellipse_cap
            .paint_ellipse(0, 0, MAX_AXIS, 4, "纯黑", true, BrushSize::One)
            .unwrap();
        assert_eq!(ellipse.pixels, ellipse_cap.pixels);
        assert!(ellipse.pixels.contains(&[0, 0, 0]));
    }

    #[test]
    fn fully_clipped_circle_records_no_undo() {
        // A diameter-512 disk anchored at the origin does not reach an 8×8 corner.
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        let before = canvas.undo_len();
        let written = canvas
            .paint_circle(0, 0, 512, "纯黑", false, BrushSize::One)
            .unwrap();
        assert_eq!(written, 0);
        assert_eq!(canvas.undo_len(), before);
        assert!(
            canvas
                .pixels
                .iter()
                .all(|pixel| *pixel == PaletteMode::Colors24.white_rgb())
        );
        let filled = canvas
            .paint_circle(0, 0, MAX_AXIS, "纯黑", true, BrushSize::One)
            .unwrap();
        assert_eq!(filled, 0);
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
    fn fill_background_overwrites_every_cell_and_undoes_once() {
        for mode in [
            PaletteMode::Colors24,
            PaletteMode::Colors144,
            PaletteMode::Colors221,
        ] {
            let mut canvas = Canvas::new(4, 3, mode).unwrap();
            assert_eq!(canvas.background_code(), Some("H2"));
            assert_eq!(canvas.background_label(), "雪白（H2）");
            canvas
                .paint(
                    &[stamp(0, 0, "纯黑"), stamp(1, 0, "正红"), stamp(2, 1, "H2")],
                    BrushSize::One,
                )
                .unwrap();
            assert_eq!(canvas.background_code(), Some("H2"));
            let before_codes = canvas.codes().to_vec();
            let before_pixels = canvas.pixels.clone();
            let undo_before = canvas.undo_len();

            let err = canvas.fill_background("彩虹").unwrap_err();
            assert!(matches!(err, CanvasError::UnknownColor { .. }));
            // `-` clears a region in flood_fill; it is not a background color.
            assert!(canvas.fill_background("-").is_err());
            assert!(canvas.fill_background("  ").is_err());
            if mode == PaletteMode::Colors24 {
                assert!(canvas.fill_background("淡鹅黄").is_err());
                assert!(canvas.fill_background("A1").is_err());
            }
            assert_eq!(canvas.codes(), &before_codes[..]);
            assert_eq!(canvas.pixels, before_pixels);
            assert_eq!(canvas.undo_len(), undo_before);
            assert_eq!(canvas.background_code(), Some("H2"));

            let red = palette::resolve_color(mode, "正红").unwrap();
            let area = (canvas.width() * canvas.height()) as usize;
            // One cell is already 正红; the rest, including white beads, change.
            assert_eq!(canvas.fill_background("  f2 ").unwrap(), area - 1);
            assert!(canvas.codes().iter().all(|code| *code == Some(red.code)));
            assert!(canvas.pixels.iter().all(|rgb| *rgb == red.rgb));
            assert_eq!(canvas.background_code(), Some("F2"));
            assert_eq!(canvas.background_label(), "正红（F2）");
            assert_eq!(canvas.undo_len(), undo_before + 1);
            assert_eq!(crate::pattern::counts(&canvas).get("F2"), Some(&area));

            assert_eq!(canvas.fill_background("正红").unwrap(), 0);
            assert_eq!(canvas.fill_background("F2").unwrap(), 0);
            assert_eq!(canvas.undo_len(), undo_before + 1);
            assert_eq!(canvas.background_code(), Some("F2"));

            canvas.undo(1).unwrap();
            assert_eq!(canvas.codes(), &before_codes[..]);
            assert_eq!(canvas.pixels, before_pixels);
            assert_eq!(canvas.background_code(), Some("H2"));
            assert_eq!(canvas.undo_len(), undo_before);

            let mut empty = Canvas::new_empty(3, 2, mode).unwrap();
            assert!(empty.codes().iter().all(Option::is_none));
            assert_eq!(empty.background_code(), None);
            assert_eq!(empty.background_label(), "空格");
            let white = palette::resolve_color(mode, "H2").unwrap();
            assert_eq!(empty.fill_background("雪白").unwrap(), 6);
            assert!(empty.codes().iter().all(|code| *code == Some("H2")));
            assert_eq!(empty.background_code(), Some("H2"));
            assert!(empty.pixels.iter().all(|rgb| *rgb == white.rgb));
            let png = image::load_from_memory(&empty.encode_png_8x().unwrap())
                .unwrap()
                .to_rgba8();
            assert_eq!((png.width(), png.height()), (24, 16));
            assert!(
                png.pixels()
                    .all(|pixel| pixel.0[3] == 255 && white.rgb == pixel.0[..3])
            );
            assert_eq!(empty.undo_len(), 1);
            empty.undo(1).unwrap();
            assert!(empty.codes().iter().all(Option::is_none));
            assert_eq!(empty.background_code(), None);
            let cleared = image::load_from_memory(&empty.encode_png_8x().unwrap())
                .unwrap()
                .to_rgba8();
            assert!(cleared.pixels().all(|pixel| pixel.0[3] == 0));

            let mut white_canvas = Canvas::new(2, 2, mode).unwrap();
            assert_eq!(white_canvas.fill_background("h2").unwrap(), 0);
            assert_eq!(white_canvas.undo_len(), 0);
            assert_eq!(white_canvas.background_code(), Some("H2"));
            assert_eq!(white_canvas.fill_background("中国红").unwrap(), 4);
            assert!(white_canvas.codes().iter().all(|code| *code == Some("F5")));
            assert_eq!(white_canvas.background_code(), Some("F5"));

            let mut batched = Canvas::new(2, 2, mode).unwrap();
            batched
                .paint(&[stamp(0, 0, "纯黑")], BrushSize::One)
                .unwrap();
            let prior = batched.codes().to_vec();
            batched.begin_batch();
            assert!(batched.fill_background("nope").is_err());
            batched.end_batch();
            assert_eq!(batched.codes(), &prior[..]);
            assert_eq!(batched.background_code(), Some("H2"));
            assert_eq!(batched.undo_len(), 1);

            batched.begin_batch();
            assert_eq!(batched.fill_background("明黄").unwrap(), 4);
            assert_eq!(batched.background_code(), Some("A4"));
            batched
                .paint(&[stamp(1, 1, "纯黑")], BrushSize::One)
                .unwrap();
            batched.end_batch();
            assert_eq!(batched.undo_len(), 2);
            assert_eq!(
                batched
                    .codes()
                    .iter()
                    .filter(|code| **code == Some("A4"))
                    .count(),
                3
            );
            assert_eq!(batched.codes()[3], Some("H7"));
            batched.undo(1).unwrap();
            assert_eq!(batched.codes(), &prior[..]);
            assert_eq!(batched.background_code(), Some("H2"));
        }
    }

    #[test]
    fn background_color_is_remembered_and_updated_by_refill() {
        let mode = PaletteMode::Colors24;
        let err = Canvas::new_with_color(0, 4, mode, "正红").unwrap_err();
        assert!(matches!(err, CanvasError::InvalidSize { .. }));
        let err = Canvas::new_with_color(2, 2, mode, "淡鹅黄").unwrap_err();
        assert!(matches!(err, CanvasError::UnknownColor { .. }));
        assert!(Canvas::new_with_color(2, 2, mode, "-").is_err());

        let mut canvas = Canvas::new_with_color(2, 2, mode, "  正红 ").unwrap();
        assert_eq!(canvas.undo_len(), 0);
        assert_eq!(canvas.background_code(), Some("F2"));
        assert!(canvas.codes().iter().all(|code| *code == Some("F2")));
        assert!(canvas.pixels.iter().all(|rgb| *rgb == [0xFC, 0x3D, 0x46]));
        // Painting does not retarget the remembered background.
        canvas
            .paint(&[stamp(0, 0, "纯黑")], BrushSize::One)
            .unwrap();
        assert_eq!(canvas.background_code(), Some("F2"));

        // Every cell is already 中国红 only after a full overwrite. First paint the
        // remaining cells the same color without fill_background, then refill.
        canvas
            .paint(
                &[
                    stamp(1, 0, "中国红"),
                    stamp(0, 1, "中国红"),
                    stamp(1, 1, "中国红"),
                    stamp(0, 0, "中国红"),
                ],
                BrushSize::One,
            )
            .unwrap();
        assert!(canvas.codes().iter().all(|code| *code == Some("F5")));
        assert_eq!(canvas.background_code(), Some("F2"));
        let undo_before = canvas.undo_len();
        assert_eq!(canvas.fill_background("F5").unwrap(), 0);
        assert_eq!(canvas.background_code(), Some("F5"));
        assert_eq!(canvas.undo_len(), undo_before + 1);
        canvas.undo(1).unwrap();
        assert_eq!(canvas.background_code(), Some("F2"));
        assert!(canvas.codes().iter().all(|code| *code == Some("F5")));

        canvas.begin_batch();
        assert_eq!(canvas.fill_background("纯黑").unwrap(), 4);
        assert_eq!(canvas.background_code(), Some("H7"));
        canvas.end_batch();
        assert_eq!(canvas.undo_len(), undo_before + 1);
        canvas.undo(1).unwrap();
        assert_eq!(canvas.background_code(), Some("F2"));
        assert!(canvas.codes().iter().all(|code| *code == Some("F5")));

        // Cells already match the new color, so only the remembered color changes,
        // and a batch still records that as one undo step.
        canvas.begin_batch();
        assert_eq!(canvas.fill_background("中国红").unwrap(), 0);
        canvas.end_batch();
        assert_eq!(canvas.background_code(), Some("F5"));
        assert_eq!(canvas.undo_len(), undo_before + 1);
        canvas.undo(1).unwrap();
        assert_eq!(canvas.background_code(), Some("F2"));
        assert!(canvas.codes().iter().all(|code| *code == Some("F5")));
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
            .paint_circle(2, 2, 5, "深蓝", true, BrushSize::One)
            .unwrap();
        let blue = [0x0F, 0x54, 0xC0];
        let yellow = [0xFB, 0xED, 0x56];
        let white = PaletteMode::Colors24.white_rgb();
        // Center cell of the diameter-5 disk. The box corner is outside it.
        assert_eq!(canvas.pixels[4 * 9 + 4], blue);
        assert_eq!(canvas.pixels[2 * 9 + 2], white);
        canvas.flood_fill(0, 0, "明黄").unwrap();
        assert_eq!(canvas.pixels[0], yellow);
        assert_eq!(canvas.pixels[4 * 9 + 4], blue);

        let mut ellipse = Canvas::new(9, 9, PaletteMode::Colors24).unwrap();
        ellipse
            .paint_ellipse(1, 3, 7, 3, "纯黑", true, BrushSize::One)
            .unwrap();
        // Equator of a 7×3 ellipse is the middle row and spans the full width.
        assert_eq!(ellipse.pixels[4 * 9 + 1], [0, 0, 0]);
        assert_eq!(ellipse.pixels[4 * 9 + 7], [0, 0, 0]);
        assert_eq!(ellipse.pixels[4 * 9 + 4], [0, 0, 0]);
        assert_eq!(ellipse.pixels[2 * 9 + 4], white);
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
            assert_eq!(
                xs.len(),
                n,
                "line {x0},{y0}->{x1},{y1} is thicker than 1px in y"
            );
        } else {
            let mut ys: Vec<u32> = cells.iter().map(|c| c.1).collect();
            ys.sort_unstable();
            let n = ys.len();
            ys.dedup();
            assert_eq!(
                ys.len(),
                n,
                "line {x0},{y0}->{x1},{y1} is thicker than 1px in x"
            );
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
        assert!(
            cells.iter().all(|&(_, y)| y == 8),
            "vertical bleed: {cells:?}"
        );
    }

    #[test]
    fn vertical_line_does_not_bleed_into_neighbor_columns() {
        let mut canvas = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        canvas
            .paint_line(6, 1, 6, 11, "纯黑", BrushSize::One)
            .unwrap();
        let cells = cells_with(&canvas, black());
        assert_eq!(cells.len(), 11);
        assert!(
            cells.iter().all(|&(x, _)| x == 6),
            "horizontal bleed: {cells:?}"
        );
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
            .paint_circle(2, 2, 11, "纯黑", false, BrushSize::One)
            .unwrap();
        let circle_cells = cells_with(&circle, black());
        let circle_fill: HashSet<(u32, u32)> = shapes::circle_fill_points(2, 2, 11)
            .into_iter()
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
        assert!(circle_fill.contains(&(7, 7)));
        assert!(
            !circle_cells.contains(&(7, 7)),
            "circle stroke filled center"
        );
        assert!(circle_cells.is_subset(&circle_fill));
        assert!(is_8_connected_region(&circle_cells), "circle ring has gaps");

        let mut ellipse = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        ellipse
            .paint_ellipse(2, 4, 12, 8, "纯黑", false, BrushSize::One)
            .unwrap();
        let ellipse_cells = cells_with(&ellipse, black());
        let ellipse_fill: HashSet<(u32, u32)> = shapes::ellipse_fill_points(2, 4, 12, 8)
            .into_iter()
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
        assert!(ellipse_fill.contains(&(8, 8)));
        assert!(
            !ellipse_cells.contains(&(8, 8)),
            "ellipse stroke filled center"
        );
        assert!(ellipse_cells.is_subset(&ellipse_fill));
        assert!(is_8_connected_region(&ellipse_cells));

        let mut triangle = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        triangle
            .paint_triangle(2, 2, 13, 2, 2, 13, "纯黑", false, BrushSize::One)
            .unwrap();
        let tri_cells = cells_with(&triangle, black());
        assert!(
            !tri_cells.contains(&(4, 4)),
            "triangle stroke filled interior"
        );
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
        assert!(
            cells
                .iter()
                .all(|&(x, y)| (3..=10).contains(&x) && (3..=9).contains(&y))
        );

        let mut circle = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        circle
            .paint_circle(3, 3, 9, "纯黑", true, BrushSize::One)
            .unwrap();
        let disk = cells_with(&circle, black());
        let expected: HashSet<(u32, u32)> = shapes::circle_fill_points(3, 3, 9)
            .into_iter()
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
        assert_eq!(disk, expected);
        assert!(disk.contains(&(7, 7)));
        assert!(
            disk.iter()
                .all(|&(x, y)| (3..12).contains(&x) && (3..12).contains(&y))
        );
    }

    #[test]
    fn circle_anchor_out_of_bounds_errors_and_overflow_clips() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        let err = canvas
            .paint_circle(8, 0, 4, "纯黑", true, BrushSize::One)
            .unwrap_err();
        assert!(matches!(err, CanvasError::OutOfBounds { x: 8, y: 0, .. }));
        let err = canvas
            .paint_ellipse(0, 8, 4, 4, "纯黑", true, BrushSize::One)
            .unwrap_err();
        assert!(matches!(err, CanvasError::OutOfBounds { x: 0, y: 8, .. }));
        assert_eq!(canvas.undo_len(), 0);
        assert!(
            canvas
                .pixels
                .iter()
                .all(|pixel| *pixel == PaletteMode::Colors24.white_rgb())
        );

        let zero = canvas
            .paint_circle(0, 0, 0, "纯黑", true, BrushSize::One)
            .unwrap_err();
        assert!(zero.to_string().contains("直径"));
        let zero_ellipse = canvas
            .paint_ellipse(0, 0, 4, 0, "纯黑", true, BrushSize::One)
            .unwrap_err();
        assert!(zero_ellipse.to_string().contains("宽和高"));
        assert_eq!(canvas.undo_len(), 0);

        let written = canvas
            .paint_circle(6, 5, 5, "纯黑", true, BrushSize::One)
            .unwrap();
        let cells = cells_with(&canvas, black());
        let expected: HashSet<(u32, u32)> = shapes::circle_fill_points(6, 5, 5)
            .into_iter()
            .filter(|&(x, y)| (0..8).contains(&x) && (0..8).contains(&y))
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
        assert_eq!(cells, expected);
        assert_eq!(written, expected.len());
        assert!(written > 0);
        assert!(
            shapes::circle_fill_points(6, 5, 5)
                .iter()
                .any(|&(x, y)| x >= 8 || y >= 8),
            "fixture should actually overflow"
        );
    }

    #[test]
    fn circle_and_ellipse_each_record_one_undo_step() {
        let mut canvas = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        canvas
            .paint_circle(1, 1, 9, "纯黑", true, BrushSize::One)
            .unwrap();
        assert_eq!(canvas.undo_len(), 1);
        let after_circle = canvas.pixels.clone();
        canvas
            .paint_ellipse(0, 8, 6, 4, "正红", false, BrushSize::One)
            .unwrap();
        assert_eq!(canvas.undo_len(), 2);
        canvas.undo(1).unwrap();
        assert_eq!(canvas.pixels, after_circle);
        assert_eq!(canvas.undo_len(), 1);
        canvas.undo(1).unwrap();
        assert!(
            canvas
                .pixels
                .iter()
                .all(|pixel| *pixel == PaletteMode::Colors24.white_rgb())
        );
    }

    #[test]
    fn circle_fill_ignores_brush_and_stroke_brush_stays_hollow() {
        let mut thin_fill = Canvas::new(32, 32, PaletteMode::Colors24).unwrap();
        thin_fill
            .paint_circle(0, 0, 16, "纯黑", true, BrushSize::One)
            .unwrap();
        let mut thick_fill = Canvas::new(32, 32, PaletteMode::Colors24).unwrap();
        thick_fill
            .paint_circle(0, 0, 16, "纯黑", true, BrushSize::Eight)
            .unwrap();
        assert_eq!(thin_fill.pixels, thick_fill.pixels);

        let mut thin = Canvas::new(32, 32, PaletteMode::Colors24).unwrap();
        thin.paint_circle(0, 0, 16, "纯黑", false, BrushSize::One)
            .unwrap();
        let mut thick = Canvas::new(32, 32, PaletteMode::Colors24).unwrap();
        thick
            .paint_circle(0, 0, 16, "纯黑", false, BrushSize::Two)
            .unwrap();
        let thin_cells = cells_with(&thin, black());
        let thick_cells = cells_with(&thick, black());
        assert!(thin_cells.is_subset(&thick_cells));
        assert!(thick_cells.len() > thin_cells.len());
        // The four cells around the center of an even diameter stay empty.
        for cell in [(7, 7), (8, 7), (7, 8), (8, 8)] {
            assert!(!thick_cells.contains(&cell), "brush 2 filled {cell:?}");
        }
        assert!(is_8_connected_region(&thick_cells));

        let mut ellipse = Canvas::new(32, 32, PaletteMode::Colors24).unwrap();
        ellipse
            .paint_ellipse(1, 1, 16, 16, "纯黑", false, BrushSize::Two)
            .unwrap();
        let cells = cells_with(&ellipse, black());
        for cell in [(8, 8), (9, 8), (8, 9), (9, 9)] {
            assert!(!cells.contains(&cell), "ellipse brush filled {cell:?}");
        }
    }

    #[test]
    fn diamond_matches_pixels_and_records_one_undo() {
        let mut canvas = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        let before = canvas.clone();
        let err = canvas
            .paint_diamond(4, 4, 3, 2, "彩虹", true, BrushSize::One)
            .unwrap_err();
        assert!(matches!(err, CanvasError::UnknownColor { .. }));
        assert_eq!(canvas.pixels, before.pixels);
        assert_eq!(canvas.codes(), before.codes());
        assert_eq!(canvas.undo_len(), 0);

        let oob = canvas
            .paint_diamond(16, 0, 2, 2, "纯黑", true, BrushSize::One)
            .unwrap_err();
        assert!(matches!(oob, CanvasError::OutOfBounds { x: 16, y: 0, .. }));
        assert_eq!(canvas.undo_len(), 0);
        assert_eq!(canvas.pixels, before.pixels);

        canvas
            .paint_diamond(6, 5, 3, 2, "纯黑", true, BrushSize::Eight)
            .unwrap();
        assert_eq!(canvas.undo_len(), 1);
        let cells = cells_with(&canvas, black());
        let mut via_pixels = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        let updates: Vec<_> = cells.iter().map(|&(x, y)| stamp(x, y, "H7")).collect();
        via_pixels.paint(&updates, BrushSize::One).unwrap();
        assert_eq!(canvas.codes(), via_pixels.codes());
        assert_eq!(canvas.pixels, via_pixels.pixels);
        assert_eq!(
            canvas.encode_png_8x().unwrap(),
            via_pixels.encode_png_8x().unwrap()
        );
        assert_eq!(
            crate::pattern::counts(&canvas),
            crate::pattern::counts(&via_pixels)
        );

        let expected: HashSet<(u32, u32)> = shapes::diamond_fill_points(6, 5, 3, 2)
            .into_iter()
            .filter(|&(x, y)| (0..16).contains(&x) && (0..16).contains(&y))
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
        assert_eq!(cells, expected);
        // Brush is ignored for fills: the same diamond with brush 1 matches.
        let mut thin = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        thin.paint_diamond(6, 5, 3, 2, "纯黑", true, BrushSize::One)
            .unwrap();
        assert_eq!(canvas.pixels, thin.pixels);

        let after = canvas.pixels.clone();
        canvas
            .paint_diamond(2, 2, 4, 4, "正红", false, BrushSize::One)
            .unwrap();
        assert_eq!(canvas.undo_len(), 2);
        canvas.undo(1).unwrap();
        assert_eq!(canvas.pixels, after);
        assert_eq!(canvas.undo_len(), 1);
    }

    #[test]
    fn diamond_clips_past_the_edge_and_caps_half_axes() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        let written = canvas
            .paint_diamond(0, 0, 3, 2, "纯黑", true, BrushSize::One)
            .unwrap();
        let cells = cells_with(&canvas, black());
        let expected: HashSet<(u32, u32)> = shapes::diamond_fill_points(0, 0, 3, 2)
            .into_iter()
            .filter(|&(x, y)| (0..8).contains(&x) && (0..8).contains(&y))
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
        assert_eq!(cells, expected);
        assert_eq!(written, expected.len());
        assert!(
            shapes::diamond_fill_points(0, 0, 3, 2)
                .iter()
                .any(|&(x, y)| x < 0 || y < 0),
            "fixture should extend past the top-left"
        );
        assert!(cells.contains(&(0, 0)));
        assert!(!cells.contains(&(3, 2)));

        let mut right = Canvas::new(8, 8, PaletteMode::Colors24).unwrap();
        right
            .paint_diamond(7, 3, 3, 2, "纯黑", true, BrushSize::One)
            .unwrap();
        let right_cells = cells_with(&right, black());
        let right_expected: HashSet<(u32, u32)> = shapes::diamond_fill_points(7, 3, 3, 2)
            .into_iter()
            .filter(|&(x, y)| (0..8).contains(&x) && (0..8).contains(&y))
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
        assert_eq!(right_cells, right_expected);
        assert!(
            shapes::diamond_fill_points(7, 3, 3, 2)
                .iter()
                .any(|&(x, _)| x >= 8)
        );

        let mut huge = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        huge.paint_diamond(0, 0, u32::MAX, 200, "纯黑", true, BrushSize::One)
            .unwrap();
        let mut capped = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        capped
            .paint_diamond(
                0,
                0,
                MAX_DIAMOND_HALF_AXIS,
                200,
                "纯黑",
                true,
                BrushSize::One,
            )
            .unwrap();
        assert_eq!(huge.pixels, capped.pixels);
        let mut also = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        also.paint_diamond(
            0,
            0,
            MAX_DIAMOND_HALF_AXIS + 1,
            200,
            "纯黑",
            true,
            BrushSize::One,
        )
        .unwrap();
        assert_eq!(huge.pixels, also.pixels);
        let mut smaller = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        smaller
            .paint_diamond(
                0,
                0,
                MAX_DIAMOND_HALF_AXIS - 1,
                200,
                "纯黑",
                true,
                BrushSize::One,
            )
            .unwrap();
        assert_ne!(huge.pixels, smaller.pixels);

        let mut tall = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        tall.paint_diamond(0, 0, 200, u32::MAX, "纯黑", true, BrushSize::One)
            .unwrap();
        let mut tall_cap = Canvas::new(128, 128, PaletteMode::Colors24).unwrap();
        tall_cap
            .paint_diamond(
                0,
                0,
                200,
                MAX_DIAMOND_HALF_AXIS,
                "纯黑",
                true,
                BrushSize::One,
            )
            .unwrap();
        assert_eq!(tall.pixels, tall_cap.pixels);
    }

    #[test]
    fn diamond_stroke_stays_hollow_and_brush_thickens_it() {
        let mut thin = Canvas::new(24, 24, PaletteMode::Colors24).unwrap();
        thin.paint_diamond(10, 10, 6, 5, "纯黑", false, BrushSize::One)
            .unwrap();
        let thin_cells = cells_with(&thin, black());
        let fill: HashSet<(u32, u32)> = shapes::diamond_fill_points(10, 10, 6, 5)
            .into_iter()
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
        assert!(fill.contains(&(10, 10)));
        assert!(
            !thin_cells.contains(&(10, 10)),
            "diamond stroke filled center"
        );
        assert!(thin_cells.is_subset(&fill));
        assert!(is_8_connected_region(&thin_cells));
        assert!(thin_cells.len() * 2 < fill.len());

        let mut thick = Canvas::new(24, 24, PaletteMode::Colors24).unwrap();
        thick
            .paint_diamond(10, 10, 6, 5, "纯黑", false, BrushSize::Two)
            .unwrap();
        let thick_cells = cells_with(&thick, black());
        assert!(thin_cells.is_subset(&thick_cells));
        assert!(thick_cells.len() > thin_cells.len());
        assert!(
            !thick_cells.contains(&(10, 10)),
            "brush 2 filled the center"
        );
        assert!(is_8_connected_region(&thick_cells));
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
            canvas.paint_curve(points, "纯黑", BrushSize::One).unwrap();
            let cells = cells_with(&canvas, black());
            assert!(cells.contains(&points[0]), "curve missing start");
            assert!(
                cells.contains(&points[points.len() - 1]),
                "curve missing end"
            );
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
        assert!(
            red_cells
                .iter()
                .all(|&(x, y)| (3..=8).contains(&x) && (3..=8).contains(&y))
        );
        // Exterior white and the black border stay untouched.
        assert_eq!(canvas.codes()[0], Some("H2"));
        assert_eq!(canvas.codes()[2 * 12 + 2], Some("H7"));
    }

    #[test]
    fn reversed_lines_paint_the_same_cells() {
        // Shallow slopes used to differ when drawn end-to-start.
        let pairs = [
            ((0, 0), (15, 2)),
            ((0, 0), (8, 1)),
            ((1, 12), (14, 3)),
            ((15, 15), (0, 7)),
        ];
        for &(a, b) in &pairs {
            let mut forward = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
            forward
                .paint_line(a.0, a.1, b.0, b.1, "纯黑", BrushSize::One)
                .unwrap();
            let mut backward = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
            backward
                .paint_line(b.0, b.1, a.0, a.1, "纯黑", BrushSize::One)
                .unwrap();
            assert_eq!(
                forward.pixels, backward.pixels,
                "direction changed the painted cells for {a:?}->{b:?}"
            );
            assert_eq!(forward.codes(), backward.codes());
        }
    }

    fn cell(canvas: &Canvas, x: u32, y: u32) -> Option<&'static str> {
        canvas.codes()[(y * canvas.width() + x) as usize]
    }

    #[test]
    fn move_region_writes_source_codes_at_dest_and_background_at_source() {
        let mut canvas = Canvas::new_with_color(4, 2, PaletteMode::Colors24, "正红").unwrap();
        assert_eq!(canvas.background_code(), Some("F2"));
        canvas
            .paint(&[stamp(0, 0, "纯黑"), stamp(1, 0, "明黄")], BrushSize::One)
            .unwrap();
        let undo_before = canvas.undo_len();
        let outcome = canvas
            .move_region(0, 0, 1, 0, Some(2), Some(0), None, None)
            .unwrap();
        assert!(outcome.changed);
        assert_eq!(
            (outcome.x0, outcome.y0, outcome.x1, outcome.y1),
            (0, 0, 1, 0)
        );
        assert_eq!(
            (outcome.nx0, outcome.ny0, outcome.nx1, outcome.ny1),
            (2, 0, 3, 0)
        );
        // Codes, not RGB: source becomes the remembered background bead.
        assert_eq!(cell(&canvas, 0, 0), Some("F2"));
        assert_eq!(cell(&canvas, 1, 0), Some("F2"));
        assert_eq!(cell(&canvas, 2, 0), Some("H7"));
        assert_eq!(cell(&canvas, 3, 0), Some("A4"));
        assert_eq!(cell(&canvas, 0, 1), Some("F2"));
        assert_eq!(canvas.background_code(), Some("F2"));
        assert_eq!(canvas.undo_len(), undo_before + 1);
        canvas.undo(1).unwrap();
        assert_eq!(cell(&canvas, 0, 0), Some("H7"));
        assert_eq!(cell(&canvas, 1, 0), Some("A4"));
        assert_eq!(cell(&canvas, 2, 0), Some("F2"));
        assert_eq!(canvas.background_code(), Some("F2"));
    }

    #[test]
    fn move_region_on_empty_background_leaves_spaces() {
        let mut canvas = Canvas::new_empty(3, 1, PaletteMode::Colors24).unwrap();
        assert_eq!(canvas.background_code(), None);
        canvas
            .paint(&[stamp(0, 0, "纯黑")], BrushSize::One)
            .unwrap();
        canvas
            .move_region(0, 0, 0, 0, Some(2), Some(0), None, None)
            .unwrap();
        assert_eq!(canvas.codes(), &[None, None, Some("H7")]);
        assert_eq!(canvas.background_code(), None);
        let png = image::load_from_memory(&canvas.encode_png_8x().unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!((png.width(), png.height()), (24, 8));
        assert_eq!(png.get_pixel(0, 0).0[3], 0);
        assert_eq!(png.get_pixel(16, 0).0, [0, 0, 0, 255]);
    }

    #[test]
    fn move_region_normalizes_reversed_endpoints() {
        let mut forward = Canvas::new(4, 3, PaletteMode::Colors221).unwrap();
        let mut reversed = Canvas::new(4, 3, PaletteMode::Colors221).unwrap();
        for canvas in [&mut forward, &mut reversed] {
            canvas
                .paint(
                    &[
                        stamp(0, 0, "正红"),
                        stamp(1, 0, "纯黑"),
                        stamp(0, 1, "明黄"),
                        stamp(1, 1, "-"),
                    ],
                    BrushSize::One,
                )
                .unwrap();
        }
        let by_delta = forward
            .move_region(0, 0, 1, 1, Some(2), Some(1), None, None)
            .unwrap();
        let by_origin = reversed
            .move_region(1, 1, 0, 0, None, None, Some(2), Some(1))
            .unwrap();
        assert_eq!(
            (by_origin.x0, by_origin.y0, by_origin.x1, by_origin.y1),
            (0, 0, 1, 1)
        );
        assert_eq!(
            (by_origin.nx0, by_origin.ny0, by_origin.nx1, by_origin.ny1),
            (by_delta.nx0, by_delta.ny0, by_delta.nx1, by_delta.ny1)
        );
        assert_eq!(forward.codes(), reversed.codes());
        assert_eq!(cell(&forward, 0, 0), Some("H2"));
        assert_eq!(cell(&forward, 1, 1), Some("H2"));
        assert_eq!(cell(&forward, 2, 1), Some("F2"));
        assert_eq!(cell(&forward, 3, 1), Some("H7"));
        assert_eq!(cell(&forward, 2, 2), Some("A4"));
        assert_eq!(cell(&forward, 3, 2), None);
        assert_eq!(forward.background_code(), Some("H2"));
    }

    #[test]
    fn move_region_overlap_reads_source_before_erasing() {
        let mut canvas = Canvas::new(4, 1, PaletteMode::Colors24).unwrap();
        canvas
            .paint(
                &[
                    stamp(0, 0, "正红"),
                    stamp(1, 0, "纯黑"),
                    stamp(2, 0, "明黄"),
                    stamp(3, 0, "碧绿"),
                ],
                BrushSize::One,
            )
            .unwrap();
        canvas
            .move_region(0, 0, 1, 0, Some(1), Some(0), None, None)
            .unwrap();
        // (2,0) is overwritten by the original (1,0), not by the erased cell.
        // (3,0) sits outside the destination and stays put.
        assert_eq!(
            canvas.codes(),
            &[Some("H2"), Some("F2"), Some("H7"), Some("B5")]
        );

        // A solid block shifted by one still clears the uncovered source cell.
        let mut solid = Canvas::new(4, 1, PaletteMode::Colors24).unwrap();
        solid
            .paint(
                &[
                    stamp(0, 0, "正红"),
                    stamp(1, 0, "正红"),
                    stamp(2, 0, "正红"),
                    stamp(3, 0, "正红"),
                ],
                BrushSize::One,
            )
            .unwrap();
        assert_eq!(solid.background_code(), Some("H2"));
        solid
            .move_region(0, 0, 2, 0, Some(1), Some(0), None, None)
            .unwrap();
        assert_eq!(
            solid.codes(),
            &[Some("H2"), Some("F2"), Some("F2"), Some("F2")]
        );
    }

    #[test]
    fn region_transplant_rejects_out_of_bounds_without_undo() {
        let mut canvas = Canvas::new(4, 3, PaletteMode::Colors144).unwrap();
        canvas
            .paint(&[stamp(0, 0, "正红")], BrushSize::One)
            .unwrap();
        let codes = canvas.codes().to_vec();
        let undo = canvas.undo_len();
        let background = canvas.background_code();

        let err = canvas
            .move_region(0, 0, 4, 0, Some(0), Some(0), None, None)
            .unwrap_err();
        assert_eq!(err.to_string(), "坐标越界：(4, 0) 不在 0..4 x 0..3 范围内");

        let err = canvas
            .move_region(0, 0, 1, 0, Some(3), Some(0), None, None)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "目标区域越界：起点 (3, 0) 加尺寸 2x1 超出 0..4 x 0..3。请调整 dx/dy 或 tx/ty。"
        );
        let err = canvas
            .copy_region(0, 0, 0, 0, None, None, Some(-1), Some(0))
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "目标区域越界：起点 (-1, 0) 加尺寸 1x1 超出 0..4 x 0..3。请调整 dx/dy 或 tx/ty。"
        );
        assert!(
            canvas
                .move_region(0, 0, 0, 0, Some(i32::MIN), Some(i32::MAX), None, None)
                .is_err()
        );
        assert!(
            canvas
                .copy_region(0, 0, 0, 0, None, None, Some(i32::MAX), Some(i32::MIN))
                .is_err()
        );

        canvas.begin_batch();
        assert!(
            canvas
                .move_region(0, 0, 9, 0, Some(1), Some(0), None, None)
                .is_err()
        );
        canvas.end_batch();

        assert_eq!(canvas.codes(), &codes[..]);
        assert_eq!(canvas.undo_len(), undo);
        assert_eq!(canvas.background_code(), background);
    }

    #[test]
    fn region_transplant_skips_undo_when_nothing_changes() {
        let mut canvas = Canvas::new(4, 2, PaletteMode::Colors24).unwrap();
        let codes = canvas.codes().to_vec();
        let same_place = canvas
            .move_region(0, 0, 1, 1, Some(0), Some(0), None, None)
            .unwrap();
        assert!(!same_place.changed);
        assert_eq!(same_place.written, 0);
        assert_eq!(canvas.undo_len(), 0);
        let onto_background = canvas
            .copy_region(0, 0, 1, 0, Some(2), Some(0), None, None)
            .unwrap();
        assert!(!onto_background.changed);
        assert_eq!(canvas.codes(), &codes[..]);
        assert_eq!(canvas.undo_len(), 0);

        let mut empty = Canvas::new_empty(4, 2, PaletteMode::Colors221).unwrap();
        assert!(
            !empty
                .move_region(0, 0, 1, 0, Some(2), Some(1), None, None)
                .unwrap()
                .changed
        );
        assert_eq!(empty.undo_len(), 0);
        assert!(empty.codes().iter().all(Option::is_none));

        let mut red = Canvas::new_with_color(4, 2, PaletteMode::Colors24, "正红").unwrap();
        assert!(
            !red.move_region(0, 0, 1, 0, None, None, Some(2), Some(0))
                .unwrap()
                .changed
        );
        assert_eq!(red.undo_len(), 0);
        assert_eq!(red.background_code(), Some("F2"));
        assert!(red.codes().iter().all(|code| *code == Some("F2")));

        canvas
            .paint(&[stamp(0, 0, "纯黑")], BrushSize::One)
            .unwrap();
        let undo = canvas.undo_len();
        assert!(
            !canvas
                .move_region(0, 0, 0, 0, Some(0), Some(0), None, None)
                .unwrap()
                .changed
        );
        assert_eq!(canvas.undo_len(), undo);
        assert_eq!(cell(&canvas, 0, 0), Some("H7"));

        canvas.begin_batch();
        assert!(
            !canvas
                .move_region(1, 0, 1, 0, Some(0), Some(0), None, None)
                .unwrap()
                .changed
        );
        canvas.end_batch();
        assert_eq!(canvas.undo_len(), undo);
    }

    #[test]
    fn copy_region_keeps_source_and_empty_block_erases_destination() {
        let mut canvas = Canvas::new(4, 1, PaletteMode::Colors24).unwrap();
        canvas
            .paint(&[stamp(0, 0, "正红"), stamp(1, 0, "纯黑")], BrushSize::One)
            .unwrap();
        let outcome = canvas
            .copy_region(0, 0, 1, 0, Some(2), Some(0), None, None)
            .unwrap();
        assert!(outcome.changed);
        assert_eq!(
            canvas.codes(),
            &[Some("F2"), Some("H7"), Some("F2"), Some("H7")]
        );
        assert_eq!(canvas.background_code(), Some("H2"));
        assert_eq!(canvas.undo_len(), 2);
        canvas.undo(1).unwrap();
        assert_eq!(
            canvas.codes(),
            &[Some("F2"), Some("H7"), Some("H2"), Some("H2")]
        );

        let mut empty_source = Canvas::new_empty(4, 1, PaletteMode::Colors144).unwrap();
        empty_source
            .paint(&[stamp(2, 0, "正红"), stamp(3, 0, "纯黑")], BrushSize::One)
            .unwrap();
        empty_source
            .copy_region(0, 0, 1, 0, None, None, Some(2), Some(0))
            .unwrap();
        assert_eq!(empty_source.codes(), &[None, None, None, None]);
        assert_eq!(empty_source.background_code(), None);
        assert_eq!(empty_source.undo_len(), 2);
    }

    fn paint_cells(canvas: &mut Canvas, cells: &[(u32, u32)], color: &str) {
        let updates: Vec<_> = cells.iter().map(|&(x, y)| stamp(x, y, color)).collect();
        canvas.paint(&updates, BrushSize::One).unwrap();
    }

    fn motif_block(name: &str, face: Option<u32>, x: u32, y: u32, scale: u32) -> Vec<(u32, u32)> {
        let pattern = motif::motif_points(name, face).unwrap();
        let mut cells = Vec::new();
        for &(dx, dy) in pattern.cells {
            for sy in 0..scale {
                for sx in 0..scale {
                    cells.push((
                        x + u32::from(dx) * scale + sx,
                        y + u32::from(dy) * scale + sy,
                    ));
                }
            }
        }
        cells
    }

    #[test]
    fn motif_matches_pixels_and_records_one_undo() {
        let mut canvas = Canvas::new(16, 12, PaletteMode::Colors24).unwrap();
        let before = canvas.clone();
        let background = canvas.background_code();
        let err = canvas
            .paint_motif("十字", 2, 3, "彩虹", None, None)
            .unwrap_err();
        assert!(matches!(err, CanvasError::UnknownColor { .. }));
        assert_eq!(canvas.pixels, before.pixels);
        assert_eq!(canvas.codes(), before.codes());
        assert_eq!(canvas.undo_len(), 0);
        assert_eq!(canvas.background_code(), background);

        for (name, scale, face) in [
            ("没有这种", Some(1), None),
            ("骰子面", None, None),
            ("骰子面", None, Some(0)),
            ("骰子面", None, Some(7)),
            ("十字", None, Some(1)),
            ("十字", Some(0), None),
            ("十字", Some(9), None),
        ] {
            let err = canvas.paint_motif(name, 1, 1, "纯黑", scale, face);
            assert!(err.is_err(), "{name} scale={scale:?} face={face:?}");
            assert_eq!(canvas.pixels, before.pixels);
            assert_eq!(canvas.undo_len(), 0);
            assert_eq!(canvas.background_code(), background);
        }
        let oob = canvas
            .paint_motif("十字", 16, 0, "纯黑", None, None)
            .unwrap_err();
        assert!(matches!(oob, CanvasError::OutOfBounds { x: 16, y: 0, .. }));
        assert_eq!(canvas.undo_len(), 0);

        let written = canvas
            .paint_motif("十字", 2, 3, "纯黑", None, None)
            .unwrap();
        assert_eq!(written, 5);
        assert_eq!(canvas.undo_len(), 1);
        assert_eq!(canvas.background_code(), background);
        let cells = cells_with(&canvas, black());
        let expected: HashSet<(u32, u32)> =
            motif_block("十字", None, 2, 3, 1).into_iter().collect();
        assert_eq!(cells, expected);
        // Gaps in the 3×3 box stay background beads.
        assert_eq!(cell(&canvas, 2, 3), Some("H2"));
        assert_eq!(cell(&canvas, 3, 3), Some("H7"));

        let mut via_pixels = Canvas::new(16, 12, PaletteMode::Colors24).unwrap();
        paint_cells(
            &mut via_pixels,
            &expected.iter().copied().collect::<Vec<_>>(),
            "H7",
        );
        assert_eq!(canvas.codes(), via_pixels.codes());
        assert_eq!(canvas.pixels, via_pixels.pixels);
        assert_eq!(
            canvas.encode_png_8x().unwrap(),
            via_pixels.encode_png_8x().unwrap()
        );
        assert_eq!(
            crate::pattern::counts(&canvas),
            crate::pattern::counts(&via_pixels)
        );

        let scaled = canvas
            .paint_motif("四点", 1, 1, "正红", Some(2), None)
            .unwrap();
        assert_eq!(scaled, 16);
        assert_eq!(canvas.undo_len(), 2);
        let red = palette::resolve_color(PaletteMode::Colors24, "正红").unwrap();
        let red_cells = cells_with(&canvas, red.rgb);
        let expected_red: HashSet<(u32, u32)> =
            motif_block("四点", None, 1, 1, 2).into_iter().collect();
        assert_eq!(red_cells, expected_red);
        let mut via_scale = Canvas::new(16, 12, PaletteMode::Colors24).unwrap();
        paint_cells(
            &mut via_scale,
            &expected_red.iter().copied().collect::<Vec<_>>(),
            "F2",
        );
        // The cross underneath is still there; compare only the new red cells.
        for &(x, y) in &expected_red {
            assert_eq!(cell(&canvas, x, y), Some("F2"));
            assert_eq!(cell(&via_scale, x, y), Some("F2"));
        }
        // A gap of the unscaled 四点, expanded, stays the earlier color.
        assert_eq!(cell(&canvas, 1, 3), Some("H2"));

        canvas.undo(1).unwrap();
        assert_eq!(canvas.undo_len(), 1);
        assert_eq!(cells_with(&canvas, black()), cells);
        assert!(cells_with(&canvas, red.rgb).is_empty());
        canvas.undo(1).unwrap();
        assert_eq!(canvas.pixels, before.pixels);
        assert_eq!(canvas.undo_len(), 0);
    }

    #[test]
    fn motif_clips_occupied_cells_and_ignores_gaps() {
        let mut canvas = Canvas::new(4, 4, PaletteMode::Colors24).unwrap();
        canvas
            .paint_rect(0, 0, 3, 3, "正红", true, BrushSize::One)
            .unwrap();
        let written = canvas
            .paint_motif("十字", 2, 2, "纯黑", Some(2), None)
            .unwrap();
        // Relative occupied cells of 十字 at scale 2 from (2, 2):
        // (1,0) -> (4,2) out, (0,1) -> (2,4) out, (1,1) -> (4,4) out,
        // (2,1) -> (6,4) out, (1,2) -> (4,6) out. Nothing lands.
        assert_eq!(written, 0);
        assert_eq!(canvas.undo_len(), 1, "the red fill is the only step");
        assert!(canvas.codes().iter().all(|code| *code == Some("F2")));

        let written = canvas
            .paint_motif("斜线-短", 3, 3, "纯黑", Some(2), None)
            .unwrap();
        // (0,0) block starts at (3,3): only (3,3) is on the canvas.
        // (1,1) block starts at (5,5) and is clipped entirely.
        assert_eq!(written, 1);
        assert_eq!(cell(&canvas, 3, 3), Some("H7"));
        assert_eq!(cell(&canvas, 2, 3), Some("F2"));
        assert_eq!(cell(&canvas, 3, 2), Some("F2"));
        assert_eq!(canvas.background_code(), Some("H2"));

        let mut edge = Canvas::new(8, 8, PaletteMode::Colors221).unwrap();
        let written = edge
            .paint_motif("箭头-右", 6, 6, "纯黑", None, None)
            .unwrap();
        let on_canvas: HashSet<(u32, u32)> = motif_block("箭头-右", None, 6, 6, 1)
            .into_iter()
            .filter(|&(x, y)| x < 8 && y < 8)
            .collect();
        assert!(
            motif_block("箭头-右", None, 6, 6, 1)
                .iter()
                .any(|&(x, y)| x >= 8 || y >= 8)
        );
        assert_eq!(cells_with(&edge, black()), on_canvas);
        assert_eq!(written, on_canvas.len());
        assert_eq!(edge.undo_len(), 1);
    }

    #[test]
    fn motif_clear_is_not_h2_and_background_stays() {
        let mut cleared = Canvas::new(5, 5, PaletteMode::Colors24).unwrap();
        let before_counts = crate::pattern::counts(&cleared);
        assert_eq!(before_counts["H2"], 25);
        let written = cleared.paint_motif("十字", 1, 1, "-", None, None).unwrap();
        assert_eq!(written, 5);
        assert_eq!(cleared.background_code(), Some("H2"));
        assert_eq!(cleared.undo_len(), 1);
        for (x, y) in motif_block("十字", None, 1, 1, 1) {
            assert_eq!(cell(&cleared, x, y), None, "({x},{y}) should be empty");
        }
        assert_eq!(
            cell(&cleared, 1, 1),
            Some("H2"),
            "the gap stays a white bead"
        );
        let counts = crate::pattern::counts(&cleared);
        assert_eq!(counts["H2"], 20);
        assert_eq!(counts.len(), 1);
        let png = image::load_from_memory(&cleared.encode_png_8x().unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(png.get_pixel(8, 8).0[3], 255);
        assert_eq!(png.get_pixel(16, 8).0[3], 0);

        let mut beads = Canvas::new_empty(5, 5, PaletteMode::Colors24).unwrap();
        beads
            .paint_motif("十字", 1, 1, "H2", Some(1), None)
            .unwrap();
        assert_eq!(beads.background_code(), None);
        for (x, y) in motif_block("十字", None, 1, 1, 1) {
            assert_eq!(cell(&beads, x, y), Some("H2"));
        }
        assert_eq!(cell(&beads, 1, 1), None);
        assert_eq!(crate::pattern::counts(&beads)["H2"], 5);
        let bead_png = image::load_from_memory(&beads.encode_png_8x().unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(bead_png.get_pixel(16, 8).0, [254, 255, 255, 255]);
        assert_eq!(bead_png.get_pixel(8, 8).0[3], 0);
        assert_ne!(cleared.codes(), beads.codes());

        let mut colored = Canvas::new_with_color(6, 3, PaletteMode::Colors144, "正红").unwrap();
        colored
            .paint_motif("品字-缺左上", 0, 0, "纯黑", None, None)
            .unwrap();
        assert_eq!(colored.background_code(), Some("F2"));
        colored
            .move_region(0, 0, 1, 1, Some(3), Some(0), None, None)
            .unwrap();
        assert_eq!(cell(&colored, 0, 0), Some("F2"));
        assert_eq!(cell(&colored, 1, 0), Some("F2"));
        assert_eq!(cell(&colored, 0, 1), Some("F2"));
        assert_eq!(cell(&colored, 3, 0), Some("F2"));
        assert_eq!(cell(&colored, 4, 0), Some("H7"));
        assert_eq!(cell(&colored, 3, 1), Some("H7"));
        assert_eq!(cell(&colored, 4, 1), Some("H7"));
        assert_eq!(colored.background_code(), Some("F2"));

        let mut empty_bg = Canvas::new_empty(3, 2, PaletteMode::Colors24).unwrap();
        empty_bg
            .paint_motif("斜线-短", 0, 0, "纯黑", None, None)
            .unwrap();
        assert_eq!(empty_bg.background_code(), None);
        assert_eq!(cell(&empty_bg, 0, 0), Some("H7"));
        assert_eq!(cell(&empty_bg, 1, 1), Some("H7"));
        empty_bg
            .move_region(0, 0, 0, 0, Some(2), Some(0), None, None)
            .unwrap();
        assert_eq!(cell(&empty_bg, 0, 0), None);
        assert_eq!(cell(&empty_bg, 2, 0), Some("H7"));
        assert_eq!(cell(&empty_bg, 1, 1), Some("H7"));
        assert_eq!(empty_bg.background_code(), None);
    }

    #[test]
    fn motif_batch_is_one_undo_and_scale_counts_unique_cells() {
        let mut canvas = Canvas::new(16, 16, PaletteMode::Colors24).unwrap();
        canvas.begin_batch();
        let cross = canvas
            .paint_motif("十字", 0, 0, "纯黑", Some(3), None)
            .unwrap();
        assert_eq!(cross, 5 * 9);
        canvas
            .paint_line(0, 15, 4, 15, "明黄", BrushSize::One)
            .unwrap();
        canvas.end_batch();
        assert_eq!(canvas.undo_len(), 1);
        assert_eq!(canvas.background_code(), Some("H2"));
        canvas.undo(1).unwrap();
        assert!(
            canvas
                .pixels
                .iter()
                .all(|pixel| *pixel == PaletteMode::Colors24.white_rgb())
        );
        assert_eq!(canvas.undo_len(), 0);

        canvas.begin_batch();
        let none = canvas
            .paint_motif("十字", 15, 15, "纯黑", None, None)
            .unwrap();
        assert_eq!(none, 0);
        canvas.end_batch();
        assert_eq!(canvas.undo_len(), 0);
        assert!(
            canvas
                .pixels
                .iter()
                .all(|pixel| *pixel == PaletteMode::Colors24.white_rgb())
        );

        let dice = canvas
            .paint_motif("骰子面", 0, 0, "中国红", None, Some(5))
            .unwrap();
        assert_eq!(dice, 5);
        assert_eq!(cell(&canvas, 0, 0), Some("F5"));
        assert_eq!(cell(&canvas, 1, 0), Some("H2"));
        assert_eq!(cell(&canvas, 1, 1), Some("F5"));
        assert_eq!(canvas.undo_len(), 1);
    }
}
