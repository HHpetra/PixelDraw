use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::canvas::{Canvas, PixelUpdate};
use crate::palette::{self, PaletteMode};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateCanvasRequest {
    /// 图纸宽度（像素），范围 1..=128
    pub width: u32,
    /// 图纸高度（像素），范围 1..=128
    pub height: u32,
    /// 色表模式：`24`、`144` 或 `221`。省略则默认 `221`。创建后锁定，绘制中途不可更改。
    #[serde(default)]
    pub palette: Option<String>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct ListColorsRequest {
    /// 未创建图纸时指定要列出的色表：`24`、`144` 或 `221`，默认 `221`。已有图纸时忽略此参数，列出当前锁定色表。
    #[serde(default)]
    pub palette: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DrawPixelsRequest {
    /// 全部像素写成一个字符串：`x y 颜色` 三元组，可用空格或换行分隔。例如 `0 0 正红 0 1 纯黑`。任一非法则整批拒绝。
    pub pixels: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaveImageRequest {
    /// 可选文件名，例如 `cat.png`。不填则按时间戳自动命名。只使用文件名，忽略路径。
    pub filename: Option<String>,
}

#[derive(Clone)]
pub struct PixelDraw {
    canvas: Arc<Mutex<Option<Canvas>>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl PixelDraw {
    pub fn new() -> Self {
        Self {
            canvas: Arc::new(Mutex::new(None)),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "创建指定尺寸的空白像素图纸，用白色铺满。宽和高必须在 1 到 128 之间。(0,0) 为左上角。palette 为色表模式：\"24\"、\"144\" 或 \"221\"，默认 \"221\"。色表在创建时锁定，绘制中途不可更改；要换色表请重新 create_canvas（会覆盖当前图纸）。返回放大 8 倍后的 PNG。"
    )]
    async fn create_canvas(
        &self,
        Parameters(CreateCanvasRequest {
            width,
            height,
            palette,
        }): Parameters<CreateCanvasRequest>,
    ) -> Result<CallToolResult, McpError> {
        let mode = match PaletteMode::parse(palette.as_deref()) {
            Ok(mode) => mode,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(err)]));
            }
        };
        let canvas = match Canvas::new(width, height, mode) {
            Ok(canvas) => canvas,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    err.to_string(),
                )]));
            }
        };
        let png = encode_or_internal(&canvas)?;
        *self.canvas.lock().await = Some(canvas);
        image_result(
            format!(
                "已创建 {width}x{height} 像素图纸，底色为白色，已锁定 {} 色模式。请用 list_colors 查看可用颜色，用 draw_pixels 按坐标填色。绘制中途不能切换色表。",
                mode.as_str()
            ),
            png,
        )
    }

    #[tool(
        description = "列出可用颜色（中文名与 MARD 色号）。已创建图纸时列出该图纸锁定的色表；未创建时可传 palette（\"24\"、\"144\" 或 \"221\"），默认 221。"
    )]
    async fn list_colors(
        &self,
        Parameters(ListColorsRequest { palette }): Parameters<ListColorsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (mode, locked) = {
            let guard = self.canvas.lock().await;
            match guard.as_ref() {
                Some(canvas) => (canvas.mode(), true),
                None => match PaletteMode::parse(palette.as_deref()) {
                    Ok(mode) => (mode, false),
                    Err(err) => {
                        return Ok(CallToolResult::error(vec![ContentBlock::text(err)]));
                    }
                },
            }
        };
        let mut text = palette::format_list(mode);
        if locked {
            text = format!("当前图纸已锁定以下色表。\n{text}");
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    #[tool(
        description = "在当前像素图纸上按坐标填色，可多次调用以增量绘制。pixels 是一整段字符串，格式为「x y 颜色」三元组，可用空格或换行分隔，例如「0 0 正红\\n0 1 纯黑」。坐标 (0,0) 为左上角，x 向右、y 向下。颜色必须使用创建时锁定色表中的中文名或 MARD 色号；完整列表请调用 list_colors。格式错误、坐标越界或颜色非法时整批拒绝。返回当前图纸放大 8 倍后的 PNG。"
    )]
    async fn draw_pixels(
        &self,
        Parameters(DrawPixelsRequest { pixels }): Parameters<DrawPixelsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let updates = match parse_pixels(&pixels) {
            Ok(updates) => updates,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(err)]));
            }
        };

        let mut guard = self.canvas.lock().await;
        let Some(canvas) = guard.as_mut() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "尚未创建图纸。请先调用 create_canvas。",
            )]));
        };

        match canvas.paint(&updates) {
            Ok(count) => {
                let png = encode_or_internal(canvas)?;
                let width = canvas.width();
                let height = canvas.height();
                image_result(
                    format!("已绘制 {count} 个像素，当前图纸 {width}x{height}。"),
                    png,
                )
            }
            Err(err) => Ok(CallToolResult::error(vec![ContentBlock::text(
                err.to_string(),
            )])),
        }
    }

    #[tool(
        description = "将当前像素图纸的 8 倍放大 PNG 保存到项目 output 目录。未创建图纸时不可调用。可选 filename 仅接受文件名（如 cat.png），不填则自动命名。返回保存路径和预览图。"
    )]
    async fn save_image(
        &self,
        Parameters(SaveImageRequest { filename }): Parameters<SaveImageRequest>,
    ) -> Result<CallToolResult, McpError> {
        let guard = self.canvas.lock().await;
        let Some(canvas) = guard.as_ref() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "尚未创建图纸。请先调用 create_canvas。",
            )]));
        };

        let path = match resolve_output_path(filename.as_deref()) {
            Ok(path) => path,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(err)]));
            }
        };
        let png = encode_or_internal(canvas)?;
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "无法创建目录 {}: {err}",
                    parent.display()
                ))]));
            }
        }
        if let Err(err) = fs::write(&path, &png) {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "保存失败 {}: {err}",
                path.display()
            ))]));
        }
        image_result(
            format!(
                "已保存 {}x{} 像素图（8 倍）到 {}",
                canvas.width(),
                canvas.height(),
                path.display()
            ),
            png,
        )
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for PixelDraw {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "pixeldraw",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "PixelDraw：用强约束像素指令绘图，不要直接文生图。先 create_canvas(width, height, palette?) 选定 24、144 或 221 色（默认 221，创建后锁定），用 list_colors 查看颜色，再多次 draw_pixels({pixels:\"x y 颜色 ...\"})（可用换行），完成后用 save_image 落盘。颜色只用当前模式的中文名或 MARD 色号。绘制与保存都会返回 8 倍放大 PNG。",
            )
    }
}

fn encode_or_internal(canvas: &Canvas) -> Result<Vec<u8>, McpError> {
    canvas
        .encode_png_8x()
        .map_err(|err| McpError::internal_error(format!("PNG 编码失败: {err}"), None))
}

fn image_result(text: impl Into<String>, png: Vec<u8>) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![
        ContentBlock::text(text.into()),
        ContentBlock::image(STANDARD.encode(png), "image/png"),
    ]))
}

fn output_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output")
}

fn sanitize_filename(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("文件名不能为空".into());
    }
    let name = Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "文件名无效".to_string())?;
    if name.is_empty() || name == "." || name == ".." {
        return Err("文件名无效".into());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err("文件名只能包含字母、数字、点、下划线和连字符".into());
    }
    if name.to_ascii_lowercase().ends_with(".png") {
        Ok(name.to_string())
    } else {
        Ok(format!("{name}.png"))
    }
}

fn timestamp_filename() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("pixeldraw-{secs}.png")
}

/// Parse `x y color` triplets separated by any whitespace, including newlines.
fn parse_pixels(raw: &str) -> Result<Vec<PixelUpdate>, String> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    if tokens.len() % 3 != 0 {
        return Err(format!(
            "像素字符串格式无效：应为「x y 颜色」三元组（可用空格或换行分隔），当前有 {} 个词，不是 3 的倍数。",
            tokens.len()
        ));
    }
    let mut updates = Vec::with_capacity(tokens.len() / 3);
    for chunk in tokens.chunks_exact(3) {
        let x = chunk[0]
            .parse::<u32>()
            .map_err(|_| format!("无效的 X 坐标「{}」，必须是非负整数。", chunk[0]))?;
        let y = chunk[1]
            .parse::<u32>()
            .map_err(|_| format!("无效的 Y 坐标「{}」，必须是非负整数。", chunk[1]))?;
        updates.push(PixelUpdate {
            x,
            y,
            color: chunk[2].to_string(),
        });
    }
    Ok(updates)
}

fn resolve_output_path(filename: Option<&str>) -> Result<PathBuf, String> {
    let name = match filename {
        Some(raw) if !raw.trim().is_empty() => sanitize_filename(raw)?,
        _ => timestamp_filename(),
    };
    Ok(output_dir().join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_filename_and_strips_path() {
        assert_eq!(sanitize_filename("cat.png").unwrap(), "cat.png");
        assert_eq!(sanitize_filename("cat").unwrap(), "cat.png");
        assert_eq!(sanitize_filename(r"..\evil.png").unwrap(), "evil.png");
        assert!(sanitize_filename(" ").is_err());
        assert!(sanitize_filename("a b.png").is_err());
    }

    #[test]
    fn default_path_is_under_output_dir() {
        let path = resolve_output_path(Some("demo")).unwrap();
        assert_eq!(path.file_name().unwrap(), "demo.png");
        assert_eq!(path.parent().unwrap(), output_dir());
    }

    #[test]
    fn parse_pixels_accepts_spaces_and_newlines() {
        let one_line = parse_pixels("0 0 正红 0 1 纯黑 2 0 正红").unwrap();
        let multiline = parse_pixels("0 0 正红\n0 1 纯黑\r\n2 0 正红").unwrap();
        let mixed = parse_pixels("0 0 正红\n0 1 纯黑 2 0 正红").unwrap();
        let expected = vec![
            PixelUpdate {
                x: 0,
                y: 0,
                color: "正红".into(),
            },
            PixelUpdate {
                x: 0,
                y: 1,
                color: "纯黑".into(),
            },
            PixelUpdate {
                x: 2,
                y: 0,
                color: "正红".into(),
            },
        ];
        assert_eq!(one_line, expected);
        assert_eq!(multiline, expected);
        assert_eq!(mixed, expected);
        assert!(parse_pixels("").unwrap().is_empty());
        assert!(parse_pixels("  \n\t ").unwrap().is_empty());
    }

    #[test]
    fn parse_pixels_rejects_incomplete_or_invalid_coords() {
        assert!(parse_pixels("0 0 正红 1").is_err());
        assert!(parse_pixels("a 0 正红").is_err());
        assert!(parse_pixels("0 -1 正红").is_err());
    }

    #[test]
    fn parsed_invalid_color_rejects_whole_batch() {
        let mut canvas = Canvas::new(4, 4, PaletteMode::Colors144).unwrap();
        let updates = parse_pixels("0 0 正红 1 1 彩虹").unwrap();
        let err = canvas.paint(&updates).unwrap_err();
        assert!(err.to_string().contains("未知颜色"));

        let png = canvas.encode_png_8x().unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(
            img.get_pixel(0, 0),
            &image::Rgb(PaletteMode::Colors144.white_rgb())
        );
    }

    #[test]
    fn parsed_string_paints_pixels() {
        let mut canvas = Canvas::new(4, 4, PaletteMode::Colors144).unwrap();
        let updates = parse_pixels("0 0 正红\n1 2 纯黑").unwrap();
        assert_eq!(canvas.paint(&updates).unwrap(), 2);
        let png = canvas.encode_png_8x().unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &image::Rgb([0xE7, 0x00, 0x2F]));
        assert_eq!(img.get_pixel(8, 16), &image::Rgb([0, 0, 0]));
    }

    #[test]
    fn writes_png_to_output_dir() {
        let canvas = Canvas::new(2, 2, PaletteMode::default()).unwrap();
        let path = resolve_output_path(Some("save-image-test")).unwrap();
        let png = canvas.encode_png_8x().unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &png).unwrap();
        let written = fs::read(&path).unwrap();
        let img = image::load_from_memory(&written).unwrap();
        assert_eq!(img.width(), 16);
        assert_eq!(img.height(), 16);
        let _ = fs::remove_file(&path);
    }
}
