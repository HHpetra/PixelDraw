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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateCanvasRequest {
    /// 图纸宽度（像素），范围 1..=128
    pub width: u32,
    /// 图纸高度（像素），范围 1..=128
    pub height: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PixelSpec {
    /// 像素 X 坐标，从左向右，0 为最左列
    pub x: u32,
    /// 像素 Y 坐标，从上向下，0 为最上行
    pub y: u32,
    /// 中文颜色名，例如「红色」「黑色」
    pub color: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DrawPixelsRequest {
    /// 要绘制的像素列表；任一非法则整批拒绝
    pub pixels: Vec<PixelSpec>,
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
        description = "创建指定尺寸的空白像素图纸，用白色铺满。宽和高必须在 1 到 128 之间。(0,0) 为左上角。再次调用会覆盖当前图纸。返回放大 8 倍后的 PNG。"
    )]
    async fn create_canvas(
        &self,
        Parameters(CreateCanvasRequest { width, height }): Parameters<CreateCanvasRequest>,
    ) -> Result<CallToolResult, McpError> {
        let canvas = match Canvas::new(width, height) {
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
            format!("已创建 {width}x{height} 像素图纸，底色为白色。请用 draw_pixels 按坐标填色。"),
            png,
        )
    }

    #[tool(
        description = "在当前像素图纸上按坐标填色，可多次调用以增量绘制。坐标 (0,0) 为左上角，x 向右、y 向下。颜色必须使用中文名：白色、浅灰、灰色、黑色、黄色、橙色、深橙、红色、亮红、粉色、肤色、浅肤、棕色、浅棕、绿色、深绿、亮绿、天蓝、蓝色、深蓝、青色、紫色、品红、暗红。未创建图纸、坐标越界或颜色非法时整批拒绝。返回当前图纸放大 8 倍后的 PNG。"
    )]
    async fn draw_pixels(
        &self,
        Parameters(DrawPixelsRequest { pixels }): Parameters<DrawPixelsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let updates: Vec<PixelUpdate> = pixels
            .into_iter()
            .map(|pixel| PixelUpdate {
                x: pixel.x,
                y: pixel.y,
                color: pixel.color,
            })
            .collect();

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
                "PixelDraw：用强约束像素指令绘图，不要直接文生图。先 create_canvas(width, height)，再多次 draw_pixels([{x,y,color}])，完成后用 save_image 落盘。颜色只用中文名。绘制与保存都会返回 8 倍放大 PNG。",
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
    fn writes_png_to_output_dir() {
        let canvas = Canvas::new(2, 2).unwrap();
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
