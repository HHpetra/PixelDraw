use std::io::Cursor;

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb, imageops::FilterType};

use crate::palette::{self, PaletteMode};

pub const MAX_SIZE: u32 = 128;
pub const SCALE: u32 = 8;

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
    OutOfBounds {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    UnknownColor {
        color: String,
        mode: PaletteMode,
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
            Self::UnknownColor { color, mode } => {
                write!(
                    f,
                    "未知颜色「{color}」。当前为 {} 色模式，请使用该模式中的中文名或 MARD 色号。调用 list_colors 查看完整列表。",
                    mode.as_str()
                )
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

    /// Validate the whole batch, then paint. Any invalid pixel rejects everything.
    pub fn paint(&mut self, updates: &[PixelUpdate]) -> Result<usize, CanvasError> {
        let mut resolved = Vec::with_capacity(updates.len());
        for update in updates {
            if update.x >= self.width || update.y >= self.height {
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
            let index = (y * self.width + x) as usize;
            self.pixels[index] = rgb;
        }
        Ok(updates.len())
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
            .paint(&[PixelUpdate {
                x: 1,
                y: 2,
                color: "红色".into(),
            }])
            .unwrap();
        assert_eq!(ok, 1);

        let err = canvas
            .paint(&[PixelUpdate {
                x: 4,
                y: 0,
                color: "黑色".into(),
            }])
            .unwrap_err();
        assert!(matches!(err, CanvasError::OutOfBounds { x: 4, y: 0, .. }));
    }

    #[test]
    fn rejects_unknown_color_without_painting() {
        let mut canvas = Canvas::new(2, 2, PaletteMode::Colors24).unwrap();
        let err = canvas
            .paint(&[
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
            ])
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
            .paint(&[
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
            ])
            .unwrap_err();
        assert!(matches!(err, CanvasError::UnknownColor { .. }));
        assert_eq!(canvas.pixels[0], PaletteMode::Colors24.white_rgb());
    }

    #[test]
    fn default_221_mode_rejects_24_only_name() {
        let mut canvas = Canvas::new(2, 2, PaletteMode::default()).unwrap();
        assert_eq!(canvas.mode(), PaletteMode::Colors221);
        let err = canvas
            .paint(&[PixelUpdate {
                x: 0,
                y: 0,
                color: "红色".into(),
            }])
            .unwrap_err();
        assert!(matches!(
            err,
            CanvasError::UnknownColor {
                mode: PaletteMode::Colors221,
                ..
            }
        ));
        canvas
            .paint(&[PixelUpdate {
                x: 0,
                y: 0,
                color: "正红".into(),
            }])
            .unwrap();
        assert_eq!(canvas.pixels[0], [0xE7, 0x00, 0x2F]);
        canvas
            .paint(&[PixelUpdate {
                x: 1,
                y: 1,
                color: "雪白".into(),
            }])
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
            .paint(&[PixelUpdate {
                x: 0,
                y: 0,
                color: "黑色".into(),
            }])
            .unwrap();
        let png = canvas.encode_png_8x().unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(7, 7), &Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(8, 0), &Rgb([255, 255, 255]));
    }
}
