use std::fs;
use std::io::{ErrorKind, Write};
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
use tempfile::NamedTempFile;
use tokio::sync::Mutex;

use crate::canvas::{BrushSize, Canvas, CanvasError, PixelUpdate};
use crate::palette::{self, PaletteMode};

const MAX_PIXEL_BYTES: usize = 1024 * 1024;
const MAX_STAMPS: usize = 65_536;
const MAX_SAVE_SUFFIX: usize = 100;

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
    /// 全部像素写成一个字符串：`x y 颜色` 三元组，可用空格或换行分隔。例如 `0 0 正红 0 1 纯黑`。最多 1 MiB（1048576 字节）、65536 个落点。任一非法则整批拒绝。
    pub pixels: String,
    /// 笔刷大小：`1`、`2`、`4` 或 `8`，默认 `1`。落点必须对齐到笔刷网格，并一次涂满 size×size 方块。
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DrawLineRequest {
    /// 起点 X，必须在图纸范围内
    pub x0: u32,
    /// 起点 Y
    pub y0: u32,
    /// 终点 X
    pub x1: u32,
    /// 终点 Y
    pub y1: u32,
    /// 颜色（当前色表的中文名或 MARD 色号）
    pub color: String,
    /// 线宽笔刷：1、2、4 或 8，默认 1。按画笔大小加粗，不要求网格对齐。
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DrawRectRequest {
    /// 对角起点 X（可大于 x1，自动归一化）
    pub x0: u32,
    /// 对角起点 Y
    pub y0: u32,
    /// 对角终点 X
    pub x1: u32,
    /// 对角终点 Y
    pub y1: u32,
    /// 颜色
    pub color: String,
    /// true 为填充，false 为描边
    pub fill: bool,
    /// 描边时的线宽笔刷 1/2/4/8，默认 1；填充时忽略
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DrawTriangleRequest {
    /// 顶点 A 的 X，必须在图纸内
    pub x0: u32,
    /// 顶点 A 的 Y
    pub y0: u32,
    /// 顶点 B 的 X
    pub x1: u32,
    /// 顶点 B 的 Y
    pub y1: u32,
    /// 顶点 C 的 X
    pub x2: u32,
    /// 顶点 C 的 Y
    pub y2: u32,
    /// 颜色
    pub color: String,
    /// true 为填充，false 为描边
    pub fill: bool,
    /// 描边线宽笔刷 1/2/4/8，默认 1；填充时忽略
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DrawCircleRequest {
    /// 圆心 X，必须在图纸范围内；半径可以超出图纸，超出部分裁剪
    pub cx: u32,
    /// 圆心 Y
    pub cy: u32,
    /// 半径（像素），0 表示单点
    pub radius: u32,
    /// 颜色
    pub color: String,
    /// true 为填充，false 为描边
    pub fill: bool,
    /// 描边线宽笔刷 1/2/4/8，默认 1；填充时忽略
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DrawEllipseRequest {
    /// 圆心 X
    pub cx: u32,
    /// 圆心 Y
    pub cy: u32,
    /// 水平半径，0 表示退化为竖线或单点
    pub rx: u32,
    /// 垂直半径
    pub ry: u32,
    /// 颜色
    pub color: String,
    /// true 为填充，false 为描边
    pub fill: bool,
    /// 描边线宽笔刷 1/2/4/8，默认 1；填充时忽略
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FloodFillRequest {
    /// 种子点 X，必须在图纸范围内
    pub x: u32,
    /// 种子点 Y
    pub y: u32,
    /// 填充颜色；替换与种子点相连的同色区域（四连通）
    pub color: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaveImageRequest {
    /// 可选文件名，例如 `cat.png`，不填则按时间戳自动命名，重名时最多尝试 100 个数字后缀。不覆盖已有文件。仅允许 ASCII 字母、数字、点、下划线和连字符；禁止路径、空名、尾点及 Windows 设备名，补全 .png 后最多 200 字节。
    pub filename: Option<String>,
}

#[derive(Clone)]
pub struct PixelDraw {
    canvas: Arc<Mutex<Option<Canvas>>>,
    output_dir: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl PixelDraw {
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            canvas: Arc::new(Mutex::new(None)),
            output_dir,
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
        let mut guard = self.canvas.lock().await;
        *guard = Some(canvas);
        let png = encode_or_internal(guard.as_ref().expect("canvas just stored"))?;
        image_result(
            format!(
                "已创建 {width}x{height} 像素图纸，底色为白色，已锁定 {} 色模式。直接用 draw_line / draw_rect / draw_triangle / draw_circle / draw_ellipse / flood_fill / draw_pixels 画出图形；需要查色时再 list_colors。画完可查看返回的预览图，再继续修改和调整。绘制中途不能切换色表。",
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
        description = "在当前像素图纸上按坐标填色。尽量使用像素绘制；其他工具仅用于大面积绘制。pixels 是一整段字符串，格式为「x y 颜色」三元组，可用空格或换行分隔，例如「0 0 正红\\n0 1 纯黑」。每次最多 1 MiB（1048576 字节）、65536 个落点，超限整批拒绝。可选 brush 为 1、2、4 或 8（默认 1）：在 (x,y) 涂满左上对齐的 N×N 方块，且 x、y 必须是 N 的倍数（笔刷 2 只能落在 0,2,4,6…），不对齐不吸附。坐标 (0,0) 为左上角。颜色必须使用创建时锁定色表中的中文名或 MARD 色号；完整列表请调用 list_colors。格式错误、笔刷非法、未对齐、越界或颜色非法时整批拒绝。返回当前图纸放大 8 倍后的 PNG。"
    )]
    async fn draw_pixels(
        &self,
        Parameters(DrawPixelsRequest { pixels, brush }): Parameters<DrawPixelsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let updates = match parse_pixels(&pixels) {
            Ok(updates) => updates,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(err)]));
            }
        };
        let brush = match BrushSize::parse(brush) {
            Ok(brush) => brush,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    err.to_string(),
                )]));
            }
        };

        let mut guard = self.canvas.lock().await;
        let Some(canvas) = guard.as_mut() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "尚未创建图纸。请先调用 create_canvas。",
            )]));
        };

        match canvas.paint(&updates, brush) {
            Ok(count) => {
                let png = encode_or_internal(canvas)?;
                let width = canvas.width();
                let height = canvas.height();
                let n = brush.size();
                let stamps = updates.len();
                image_result(
                    format!(
                        "已绘制 {count} 个像素（笔刷 {n}，{stamps} 个落点），当前图纸 {width}x{height}。"
                    ),
                    png,
                )
            }
            Err(err) => Ok(CallToolResult::error(vec![ContentBlock::text(
                err.to_string(),
            )])),
        }
    }

    #[tool(
        description = "画一条直线（Bresenham）。仅用于大面积绘制，细节请尽量用像素绘制。参数为起点 (x0,y0)、终点 (x1,y1) 和颜色；两端点必须在图纸内。可选 brush 为线宽 1、2、4 或 8（默认 1），按画笔大小加粗，坐标不必对齐网格。返回 8 倍放大 PNG，可据预览继续修改和调整。"
    )]
    async fn draw_line(
        &self,
        Parameters(DrawLineRequest {
            x0,
            y0,
            x1,
            y1,
            color,
            brush,
        }): Parameters<DrawLineRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| {
            let brush = BrushSize::parse(brush)?;
            canvas.paint_line(x0, y0, x1, y1, &color, brush)
        })
        .await
    }

    #[tool(
        description = "画矩形。仅用于大面积绘制，细节请尽量用像素绘制。对角两点 (x0,y0)-(x1,y1) 自动归一化。fill=true 填充整块；fill=false 只描边。描边可用 brush（1/2/4/8，默认 1）控制线宽，坐标不必对齐；填充忽略 brush。两端点必须在图纸内。返回 8 倍放大 PNG，可据预览继续修改和调整。"
    )]
    async fn draw_rect(
        &self,
        Parameters(DrawRectRequest {
            x0,
            y0,
            x1,
            y1,
            color,
            fill,
            brush,
        }): Parameters<DrawRectRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| {
            let brush = BrushSize::parse(brush)?;
            canvas.paint_rect(x0, y0, x1, y1, &color, fill, brush)
        })
        .await
    }

    #[tool(
        description = "画三角形。仅用于大面积绘制，细节请尽量用像素绘制。三个顶点 (x0,y0)、(x1,y1)、(x2,y2) 必须在图纸内。fill=true 填充，fill=false 描边。描边可用 brush（1/2/4/8，默认 1）控制线宽，坐标不必对齐；填充忽略 brush。返回 8 倍放大 PNG，可据预览继续修改和调整。"
    )]
    async fn draw_triangle(
        &self,
        Parameters(DrawTriangleRequest {
            x0,
            y0,
            x1,
            y1,
            x2,
            y2,
            color,
            fill,
            brush,
        }): Parameters<DrawTriangleRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| {
            let brush = BrushSize::parse(brush)?;
            canvas.paint_triangle(x0, y0, x1, y1, x2, y2, &color, fill, brush)
        })
        .await
    }

    #[tool(
        description = "画圆。仅用于大面积绘制，细节请尽量用像素绘制。圆心 (cx,cy) 与半径 radius（0 为单点）；圆心必须在图纸内，半径超出部分自动裁剪。fill=true 填充，fill=false 描边。描边可用 brush（1/2/4/8，默认 1）加粗；填充忽略 brush。返回 8 倍放大 PNG，可据预览继续修改和调整。"
    )]
    async fn draw_circle(
        &self,
        Parameters(DrawCircleRequest {
            cx,
            cy,
            radius,
            color,
            fill,
            brush,
        }): Parameters<DrawCircleRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| {
            let brush = BrushSize::parse(brush)?;
            canvas.paint_circle(cx, cy, radius, &color, fill, brush)
        })
        .await
    }

    #[tool(
        description = "画椭圆。仅用于大面积绘制，细节请尽量用像素绘制。圆心 (cx,cy)，水平半径 rx、垂直半径 ry（可为 0，退化为线或点）；圆心必须在图纸内，越界部分裁剪。fill=true 填充，fill=false 描边。描边可用 brush（1/2/4/8，默认 1）加粗；填充忽略 brush。返回 8 倍放大 PNG，可据预览继续修改和调整。"
    )]
    async fn draw_ellipse(
        &self,
        Parameters(DrawEllipseRequest {
            cx,
            cy,
            rx,
            ry,
            color,
            fill,
            brush,
        }): Parameters<DrawEllipseRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| {
            let brush = BrushSize::parse(brush)?;
            canvas.paint_ellipse(cx, cy, rx, ry, &color, fill, brush)
        })
        .await
    }

    #[tool(
        description = "油漆桶：仅用于大面积填充，细节请尽量用像素绘制。从种子点 (x,y) 四连通填充与该点当前颜色相同的区域，替换为 color。种子点必须在图纸内。若目标色与种子色相同则不改动。返回 8 倍放大 PNG。"
    )]
    async fn flood_fill(
        &self,
        Parameters(FloodFillRequest { x, y, color }): Parameters<FloodFillRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| canvas.flood_fill(x, y, &color)).await
    }

    async fn with_canvas<F>(&self, op: F) -> Result<CallToolResult, McpError>
    where
        F: FnOnce(&mut Canvas) -> Result<usize, CanvasError>,
    {
        let mut guard = self.canvas.lock().await;
        let Some(canvas) = guard.as_mut() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "尚未创建图纸。请先调用 create_canvas。",
            )]));
        };
        match op(canvas) {
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
        description = "将当前像素图纸快照的 8 倍放大 PNG 原子保存到运行时配置的输出目录，不覆盖已有文件。未创建图纸时不可调用。可选 filename（如 cat.png）仅允许 ASCII 字母、数字、点、下划线和连字符；禁止正反斜杠、空名、.、..、尾点及 Windows 设备名（含扩展名），补全 .png 后最多 200 字节。省略则按时间戳命名，重名时最多尝试 100 个数字后缀。保存失败返回工具错误，成功返回保存路径和预览图。"
    )]
    async fn save_image(
        &self,
        Parameters(SaveImageRequest { filename }): Parameters<SaveImageRequest>,
    ) -> Result<CallToolResult, McpError> {
        let snapshot = self.canvas.lock().await.clone();
        let Some(canvas) = snapshot else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "尚未创建图纸。请先调用 create_canvas。",
            )]));
        };

        let name = match filename.as_deref().map(sanitize_filename).transpose() {
            Ok(name) => name.unwrap_or_else(timestamp_filename),
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(err)]));
            }
        };
        let png = match canvas.encode_png_8x() {
            Ok(png) => png,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "PNG 编码失败: {err}"
                ))]));
            }
        };
        let path = match persist_png(&self.output_dir, &name, filename.is_none(), &png) {
            Ok(path) => path,
            Err(err) => return Ok(CallToolResult::error(vec![ContentBlock::text(err)])),
        };
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
                "PixelDraw：用强约束像素指令绘图，不要直接文生图。尽量使用像素绘制；其他工具（draw_line / draw_rect / draw_triangle / draw_circle / draw_ellipse / flood_fill）仅用于大面积绘制。流程：create_canvas(width, height, palette?) 建图并锁定 24/144/221 色（默认 221）→ 用 draw_pixels 精确描点，大面积可再用图元辅助 → 查看返回的 8 倍预览，继续修改和调整 → 满意后 save_image 保存。矩形、三角形、圆、椭圆用 fill=true|false 区分填充/描边；直线与描边可用 brush=1|2|4|8 控制线宽（不要求网格对齐）。draw_pixels 落点须对齐笔刷网格。list_colors 只在需要查色时调用，不是必经步骤。颜色只用当前模式的中文名或 MARD 色号。save_image 不覆盖已有文件；文件名仅限 ASCII 字母、数字、点、下划线和连字符。",
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

fn sanitize_filename(raw: &str) -> Result<String, String> {
    let name = raw;
    if name.is_empty() || name.ends_with('.') || name.len() > 200 {
        return Err("文件名无效".into());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err("文件名只能包含字母、数字、点、下划线和连字符".into());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("文件名不能使用 Windows 设备名".into());
    }
    let name = if name.to_ascii_lowercase().ends_with(".png") {
        name.to_string()
    } else {
        format!("{name}.png")
    };
    if name.len() > 200 {
        return Err("文件名补全 .png 后不能超过 200 字节".into());
    }
    Ok(name)
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
    if raw.len() > MAX_PIXEL_BYTES {
        return Err("像素字符串不能超过 1 MiB（1048576 字节）".into());
    }
    let tokens: Vec<&str> = raw.split_whitespace().take(MAX_STAMPS * 3 + 1).collect();
    if tokens.len() > MAX_STAMPS * 3 {
        return Err("每次最多允许 65536 个落点".into());
    }
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    if !tokens.len().is_multiple_of(3) {
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

fn persist_png(
    output_dir: &Path,
    name: &str,
    autogenerated: bool,
    png: &[u8],
) -> Result<PathBuf, String> {
    fs::create_dir_all(output_dir)
        .map_err(|err| format!("无法创建目录 {}: {err}", output_dir.display()))?;
    let mut temp = NamedTempFile::new_in(output_dir)
        .map_err(|err| format!("无法创建临时文件 {}: {err}", output_dir.display()))?;
    temp.write_all(png)
        .map_err(|err| format!("写入 PNG 失败: {err}"))?;
    let attempts = if autogenerated { MAX_SAVE_SUFFIX } else { 0 };
    for suffix in 0..=attempts {
        let path = output_dir.join(if suffix == 0 {
            name.to_string()
        } else {
            format!("{}-{suffix}.png", name.strip_suffix(".png").unwrap_or(name))
        });
        match temp.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(err) if err.error.kind() == ErrorKind::AlreadyExists && suffix < attempts => {
                temp = err.file;
            }
            Err(err) => return Err(format!("保存失败 {}: {}", path.display(), err.error)),
        }
    }
    unreachable!("bounded save loop always returns on its final attempt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_filename_without_stripping_paths() {
        assert_eq!(sanitize_filename("cat.png").unwrap(), "cat.png");
        assert_eq!(sanitize_filename("cat").unwrap(), "cat.png");
        assert_eq!(sanitize_filename("cat.PNG").unwrap(), "cat.PNG");
        for name in [
            "",
            " ",
            ".",
            "..",
            "cat.",
            "a b.png",
            " cat.png",
            "cat.png ",
            "../evil.png",
            r"..\evil.png",
            "/cat.png",
            r"C:\cat.png",
            "猫.png",
            "con",
            "PrN.png",
            "aux.tar.png",
            "NUL",
            "COM1.png",
            "lpt9.PNG",
        ] {
            assert!(sanitize_filename(name).is_err(), "{name:?}");
        }
        for prefix in ["COM", "LPT"] {
            for digit in 1..=9 {
                assert!(sanitize_filename(&format!("{prefix}{digit}.png")).is_err());
            }
        }
        assert!(sanitize_filename("COM0.png").is_ok());
        assert!(sanitize_filename("LPT10.png").is_ok());
        assert!(sanitize_filename(&"a".repeat(196)).is_ok());
        assert!(sanitize_filename(&"a".repeat(197)).is_err());
        assert!(sanitize_filename(&format!("{}.png", "a".repeat(197))).is_err());
    }

    #[test]
    fn parse_pixels_enforces_byte_and_stamp_limits() {
        assert!(parse_pixels(&" ".repeat(MAX_PIXEL_BYTES)).is_ok());
        assert!(parse_pixels(&" ".repeat(MAX_PIXEL_BYTES + 1)).is_err());
        assert!(parse_pixels(&"　".repeat(MAX_PIXEL_BYTES / 3 + 1)).is_err());
        assert_eq!(
            parse_pixels(&"0 0 H2 ".repeat(MAX_STAMPS)).unwrap().len(),
            MAX_STAMPS
        );
        assert!(parse_pixels(&"0 0 H2 ".repeat(MAX_STAMPS + 1)).is_err());
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
        let err = canvas.paint(&updates, BrushSize::One).unwrap_err();
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
        assert_eq!(canvas.paint(&updates, BrushSize::One).unwrap(), 2);
        let png = canvas.encode_png_8x().unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &image::Rgb([0xE7, 0x00, 0x2F]));
        assert_eq!(img.get_pixel(8, 16), &image::Rgb([0, 0, 0]));
    }

    #[test]
    fn parsed_string_paints_with_brush_two() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors144).unwrap();
        let updates = parse_pixels("0 0 正红\n2 0 纯黑").unwrap();
        assert_eq!(canvas.paint(&updates, BrushSize::Two).unwrap(), 8);
        let png = canvas.encode_png_8x().unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(8, 8), &image::Rgb([0xE7, 0x00, 0x2F]));
        assert_eq!(img.get_pixel(16, 0), &image::Rgb([0, 0, 0]));
        assert_eq!(
            img.get_pixel(32, 0),
            &image::Rgb(PaletteMode::Colors144.white_rgb())
        );
    }

    fn response_png(result: &CallToolResult) -> Vec<u8> {
        assert_ne!(result.is_error, Some(true));
        assert!(result.content[0].as_text().is_some());
        let image = result.content[1].as_image().unwrap();
        assert_eq!(image.mime_type, "image/png");
        STANDARD.decode(&image.data).unwrap()
    }

    async fn create(server: &PixelDraw) -> CallToolResult {
        server
            .create_canvas(Parameters(CreateCanvasRequest {
                width: 4,
                height: 4,
                palette: Some("24".into()),
            }))
            .await
            .unwrap()
    }

    #[test]
    fn auto_collision_retries_and_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let name = timestamp_filename();
        fs::write(dir.path().join(&name), b"original").unwrap();
        let path = persist_png(dir.path(), &name, true, b"snapshot").unwrap();
        let stem = name.strip_suffix(".png").unwrap();
        assert_eq!(path, dir.path().join(format!("{stem}-1.png")));
        assert_eq!(fs::read(&path).unwrap(), b"snapshot");
        assert_eq!(fs::read(dir.path().join(&name)).unwrap(), b"original");
        for suffix in 2..=MAX_SAVE_SUFFIX {
            fs::write(dir.path().join(format!("{stem}-{suffix}.png")), b"original").unwrap();
        }
        assert!(persist_png(dir.path(), &name, true, b"replacement").is_err());
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            MAX_SAVE_SUFFIX + 1
        );
        assert_eq!(fs::read(path).unwrap(), b"snapshot");
    }

    #[tokio::test]
    async fn save_handler_writes_image_and_preserves_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("nested");
        let server = PixelDraw::new(output.clone());
        let expected = response_png(&create(&server).await);
        let result = server
            .save_image(Parameters(SaveImageRequest {
                filename: Some("demo".into()),
            }))
            .await
            .unwrap();
        assert_eq!(response_png(&result), expected);
        assert_eq!(fs::read(output.join("demo.png")).unwrap(), expected);
        fs::write(output.join("demo.png"), b"existing").unwrap();
        let result = server
            .save_image(Parameters(SaveImageRequest {
                filename: Some("demo".into()),
            }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(fs::read(output.join("demo.png")).unwrap(), b"existing");
        assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
        let result = server
            .save_image(Parameters(SaveImageRequest { filename: None }))
            .await
            .unwrap();
        assert_eq!(response_png(&result), expected);
        assert_eq!(fs::read_dir(&output).unwrap().count(), 2);
        for filename in ["", "../escape.png", r"..\escape.png", "NUL.png"] {
            let result = server
                .save_image(Parameters(SaveImageRequest {
                    filename: Some(filename.into()),
                }))
                .await
                .unwrap();
            assert_eq!(result.is_error, Some(true));
        }
    }

    #[tokio::test]
    async fn save_output_failure_is_a_tool_error() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("not-a-directory");
        fs::write(&output, b"existing").unwrap();
        let server = PixelDraw::new(output.clone());
        create(&server).await;
        let result = server
            .save_image(Parameters(SaveImageRequest { filename: None }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(fs::read(output).unwrap(), b"existing");
    }

    #[tokio::test]
    async fn shape_tools_draw_in_one_call() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        create(&server).await;
        let lined = server
            .draw_line(Parameters(DrawLineRequest {
                x0: 0,
                y0: 0,
                x1: 3,
                y1: 0,
                color: "黑色".into(),
                brush: Some(1),
            }))
            .await
            .unwrap();
        assert_ne!(lined.is_error, Some(true));
        let filled = server
            .draw_rect(Parameters(DrawRectRequest {
                x0: 0,
                y0: 1,
                x1: 3,
                y1: 3,
                color: "红色".into(),
                fill: true,
                brush: None,
            }))
            .await
            .unwrap();
        assert_ne!(filled.is_error, Some(true));
        let png = response_png(&filled);
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &image::Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(8, 8), &image::Rgb([0xD8, 0x01, 0x27]));

        let seed = server
            .flood_fill(Parameters(FloodFillRequest {
                x: 0,
                y: 0,
                color: "黄色".into(),
            }))
            .await
            .unwrap();
        assert_ne!(seed.is_error, Some(true));
        let png = response_png(&seed);
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &image::Rgb([0xFF, 0xE9, 0x53]));
    }

    #[tokio::test]
    async fn uncreated_handlers_return_tool_errors() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().join("output"));
        let result = server
            .draw_pixels(Parameters(DrawPixelsRequest {
                pixels: "0 0 H2".into(),
                brush: None,
            }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        let result = server
            .save_image(Parameters(SaveImageRequest { filename: None }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(!server.output_dir.exists());
    }

    #[tokio::test]
    async fn invalid_create_retains_canvas_and_palette() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        let expected = response_png(&create(&server).await);
        for (width, height, palette) in [(0, 4, "144"), (4, 129, "221"), (4, 4, "bad")] {
            let result = server
                .create_canvas(Parameters(CreateCanvasRequest {
                    width,
                    height,
                    palette: Some(palette.into()),
                }))
                .await
                .unwrap();
            assert_eq!(result.is_error, Some(true));
            let guard = server.canvas.lock().await;
            let canvas = guard.as_ref().unwrap();
            assert_eq!(canvas.mode(), PaletteMode::Colors24);
            assert_eq!(canvas.encode_png_8x().unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn invalid_paint_is_atomic_including_brush_and_limits() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        let expected = response_png(&create(&server).await);
        for (pixels, brush) in [
            ("0 0 F5 2 2 invalid".into(), Some(2)),
            ("0 0 F5 4 0 H7".into(), Some(2)),
            ("0 0 F5 1 0 H7".into(), Some(2)),
            ("0 0 F5".into(), Some(3)),
            ("0 0 F5 1".into(), None),
            ("0 0 正红".into(), None),
            (" ".repeat(MAX_PIXEL_BYTES + 1), None),
            ("0 0 F5 ".repeat(MAX_STAMPS + 1), None),
        ] {
            let result = server
                .draw_pixels(Parameters(DrawPixelsRequest { pixels, brush }))
                .await
                .unwrap();
            assert_eq!(result.is_error, Some(true));
            assert_eq!(
                server
                    .canvas
                    .lock()
                    .await
                    .as_ref()
                    .unwrap()
                    .encode_png_8x()
                    .unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn palette_lock_image_response_and_independent_instances() {
        let dir = tempfile::tempdir().unwrap();
        let first = PixelDraw::new(dir.path().join("first"));
        let second = PixelDraw::new(dir.path().join("second"));
        let initial = response_png(&create(&first).await);
        assert_eq!(response_png(&create(&second).await), initial);
        let listed = first
            .list_colors(Parameters(ListColorsRequest {
                palette: Some("invalid".into()),
            }))
            .await
            .unwrap();
        assert_ne!(listed.is_error, Some(true));
        let text = &listed.content[0].as_text().unwrap().text;
        assert_eq!(
            text,
            &format!(
                "当前图纸已锁定以下色表。\n{}",
                palette::format_list(PaletteMode::Colors24)
            )
        );
        let painted = first
            .draw_pixels(Parameters(DrawPixelsRequest {
                pixels: "0 0 H7".into(),
                brush: Some(2),
            }))
            .await
            .unwrap();
        let png = response_png(&painted);
        let image = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(image.dimensions(), (32, 32));
        assert_eq!(image.get_pixel(15, 15), &image::Rgb([0, 0, 0]));
        assert_eq!(
            image.get_pixel(16, 16),
            &image::Rgb(PaletteMode::Colors24.white_rgb())
        );
        assert_ne!(png, initial);
        assert_eq!(
            second
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .encode_png_8x()
                .unwrap(),
            initial
        );
        assert_ne!(first.output_dir, second.output_dir);
    }
}
