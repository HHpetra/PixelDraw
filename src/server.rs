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

use crate::canvas::{BrushSize, Canvas, CanvasError, PixelUpdate, RegionOutcome};
use crate::palette::{self, PaletteMode};
use crate::pattern;

const MAX_PIXEL_BYTES: usize = 1024 * 1024;
const MAX_STAMPS: usize = 65_536;
const MAX_SAVE_SUFFIX: usize = 100;

#[derive(Debug, Default, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    #[default]
    White,
    Empty,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportOutput {
    #[default]
    Pixel,
    Pattern,
    Both,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateCanvasRequest {
    /// 图纸宽度（像素），范围 1..=128
    pub width: u32,
    /// 图纸高度（像素），范围 1..=128
    pub height: u32,
    /// 色表模式：`24`、`144` 或 `221`。省略则默认 `221`。创建后锁定，绘制中途不可更改。
    #[serde(default)]
    pub palette: Option<String>,
    /// 背景：white（默认，H2 白豆）或 empty（空格，不放豆）。
    #[serde(default)]
    pub background: Background,
    /// 可选背景颜色（当前色表的中文名或 MARD 色号）。省略则沿用 background。指定后整张铺该色豆，并记为当前背景色。不能与 background 为 empty 同时使用。
    #[serde(default)]
    pub background_color: Option<String>,
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
#[serde(deny_unknown_fields)]
pub struct DrawCircleRequest {
    /// 占据方形的左上角 X，必须在图纸内；右侧超出部分裁剪
    pub x: u32,
    /// 占据方形的左上角 Y，必须在图纸内；下方超出部分裁剪
    pub y: u32,
    /// 直径（格子数），必须 ≥ 1；超过 512 按 512。圆占据 diameter×diameter 格
    pub diameter: u32,
    /// 颜色
    pub color: String,
    /// true 为填充，false 为该填充的 1 格轮廓
    pub fill: bool,
    /// 描边线宽笔刷 1/2/4/8，默认 1；填充时忽略
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawEllipseRequest {
    /// 占据矩形的左上角 X，必须在图纸内；右侧超出部分裁剪
    pub x: u32,
    /// 占据矩形的左上角 Y，必须在图纸内；下方超出部分裁剪
    pub y: u32,
    /// 宽度（格子数），必须 ≥ 1；超过 512 按 512
    pub width: u32,
    /// 高度（格子数），必须 ≥ 1；超过 512 按 512。宽高相等时与同直径的圆逐格相同
    pub height: u32,
    /// 颜色
    pub color: String,
    /// true 为填充，false 为该填充的 1 格轮廓
    pub fill: bool,
    /// 描边线宽笔刷 1/2/4/8，默认 1；填充时忽略
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawDiamondRequest {
    /// 中心格 X，必须在图纸内。不是左上角锚点；越过边缘的部分裁剪
    pub cx: u32,
    /// 中心格 Y，必须在图纸内。不是左上角锚点；越过边缘的部分裁剪
    pub cy: u32,
    /// 水平菱形半轴（从中心格向一侧的格数）。宽度为 2*rx+1。超过 255 按 255。不是已废止的椭圆 rx
    pub rx: u32,
    /// 垂直菱形半轴（从中心格向一侧的格数）。高度为 2*ry+1。超过 255 按 255。不是已废止的椭圆 ry
    pub ry: u32,
    /// 颜色
    pub color: String,
    /// true 为填充，false 为该填充的 1 格轮廓
    pub fill: bool,
    /// 描边线宽笔刷 1/2/4/8，默认 1；填充时忽略
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DrawCurveRequest {
    /// 路径点列表，每项为 [x, y]，至少 2 个点，且都必须在图纸内。
    /// 2 点画直线；3 点为二次贝塞尔（中点为控制点）；4 点为三次贝塞尔（中间两点为控制点）；5 点及以上为过点平滑样条（Catmull-Rom，曲线经过每个点）。
    pub points: Vec<[u32; 2]>,
    /// 颜色
    pub color: String,
    /// 线宽笔刷 1/2/4/8，默认 1。从落点向右下扩展 N×N（与 draw_pixels 一致），坐标不必对齐网格。
    #[serde(default)]
    pub brush: Option<u32>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct UndoRequest {
    /// 撤回步数，默认 1。每次成功的绘制工具调用记为一步；draw_batch 整批记为一步。
    #[serde(default)]
    pub steps: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FloodFillRequest {
    /// 种子点 X，必须在图纸范围内
    pub x: u32,
    /// 种子点 Y
    pub y: u32,
    /// 填充颜色；替换与种子点相连的同色号区域（四连通），空格与白豆分开处理；可用 - 清为空格
    pub color: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FillBackgroundRequest {
    /// 背景颜色（当前色表的中文名或 MARD 色号）。覆盖整张图纸的每一个格子。
    pub color: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RegionRequest {
    /// 源区域起点 X，含端点，可大于 x1，自动归一化
    pub x0: u32,
    /// 源区域起点 Y
    pub y0: u32,
    /// 源区域终点 X
    pub x1: u32,
    /// 源区域终点 Y
    pub y1: u32,
    /// 相对位移 X，可负。与 tx/ty 互斥，必须且只能给一组。
    #[serde(default)]
    pub dx: Option<i32>,
    /// 相对位移 Y，可负。与 tx/ty 互斥。
    #[serde(default)]
    pub dy: Option<i32>,
    /// 目标区域左上角绝对 X。与 dx/dy 互斥。
    #[serde(default)]
    pub tx: Option<i32>,
    /// 目标区域左上角绝对 Y。与 dx/dy 互斥。
    #[serde(default)]
    pub ty: Option<i32>,
}

/// One drawing step inside `draw_batch`. Discriminated by `type`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DrawOp {
    /// 直线
    Line {
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        color: String,
        #[serde(default)]
        brush: Option<u32>,
    },
    /// 矩形，fill 为 true 填充、false 描边
    Rect {
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        color: String,
        fill: bool,
        #[serde(default)]
        brush: Option<u32>,
    },
    /// 三角形
    Triangle {
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        color: String,
        fill: bool,
        #[serde(default)]
        brush: Option<u32>,
    },
    /// 圆。左上角 (x,y) 与直径 diameter（≥1，超过 512 按 512）
    Circle {
        x: u32,
        y: u32,
        diameter: u32,
        color: String,
        fill: bool,
        #[serde(default)]
        brush: Option<u32>,
    },
    /// 椭圆。左上角 (x,y)，宽 width、高 height（≥1，超过 512 按 512）
    Ellipse {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        color: String,
        fill: bool,
        #[serde(default)]
        brush: Option<u32>,
    },
    /// 像素菱形。中心格 (cx, cy) 与菱形半轴 rx、ry（宽高为 2*r+1，超过 255 按 255）
    Diamond {
        cx: u32,
        cy: u32,
        rx: u32,
        ry: u32,
        color: String,
        fill: bool,
        #[serde(default)]
        brush: Option<u32>,
    },
    /// 曲线：points 为 [x,y] 列表；2 点直线、3 点二次贝塞尔、4 点三次贝塞尔、≥5 点过点平滑样条
    Curve {
        points: Vec<[u32; 2]>,
        color: String,
        #[serde(default)]
        brush: Option<u32>,
    },
    /// 油漆桶
    FloodFill { x: u32, y: u32, color: String },
    /// 填充背景：用 color 覆盖每一个格子，强行替换已有内容
    FillBackground { color: String },
    /// 移动区域：源矩形整块搬到目标，源区域改回当前背景色
    MoveRegion {
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        #[serde(default)]
        dx: Option<i32>,
        #[serde(default)]
        dy: Option<i32>,
        #[serde(default)]
        tx: Option<i32>,
        #[serde(default)]
        ty: Option<i32>,
    },
    /// 复制区域：源矩形整块覆盖目标，源区域保留
    CopyRegion {
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        #[serde(default)]
        dx: Option<i32>,
        #[serde(default)]
        dy: Option<i32>,
        #[serde(default)]
        tx: Option<i32>,
        #[serde(default)]
        ty: Option<i32>,
    },
    /// 与 draw_pixels 相同的点阵/方块笔刷落点
    Pixels {
        pixels: String,
        #[serde(default)]
        brush: Option<u32>,
    },
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DrawBatchRequest {
    /// 有序绘制操作；按数组顺序执行。任一步失败则整批拒绝、画布不变。最多 256 步。
    pub ops: Vec<DrawOp>,
}

const MAX_BATCH_OPS: usize = 256;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaveImageRequest {
    /// 可选文件名，例如 `cat.png`，不填则按时间戳自动命名，重名时最多尝试 100 个数字后缀。不覆盖已有文件。仅允许 ASCII 字母、数字、点、下划线和连字符；禁止路径、空名、尾点及 Windows 设备名，补全 .png 后最多 200 字节。
    pub filename: Option<String>,
    /// pixel（默认）：8 倍像素画；pattern：四边序号、逐格色号、计数线与色号数量图例的 PNG；both：两者。
    #[serde(default)]
    pub output: ExportOutput,
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
        description = "创建指定尺寸的像素图纸。background 可选 white（默认，H2 白豆）或 empty（透明空格，不计豆数）。可选 background_color 为当前色表的中文名或 MARD 色号：整张铺该色豆，并记为当前背景色；省略时与只传 background 相同。background 为 empty 时不能同时传 background_color。之后 fill_background 会把当前背景色改成新颜色。宽和高必须在 1 到 128 之间。(0,0) 为左上角。palette 为色表模式：\"24\"、\"144\" 或 \"221\"，默认 \"221\"。色表在创建时锁定，绘制中途不可更改；要换色表请重新 create_canvas（会覆盖当前图纸）。颜色非法时不创建、不覆盖已有图纸。返回放大 8 倍后的 PNG。"
    )]
    async fn create_canvas(
        &self,
        Parameters(CreateCanvasRequest {
            width,
            height,
            palette,
            background,
            background_color,
        }): Parameters<CreateCanvasRequest>,
    ) -> Result<CallToolResult, McpError> {
        let mode = match PaletteMode::parse(palette.as_deref()) {
            Ok(mode) => mode,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(err)]));
            }
        };
        let background_color = background_color
            .as_deref()
            .map(str::trim)
            .filter(|color| !color.is_empty());
        if matches!(background, Background::Empty) && background_color.is_some() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "background 为 empty 时不能同时指定 background_color。empty 表示不放豆；要设置背景颜色请传入 background_color，并省略 background 或设为 white。",
            )]));
        }
        let canvas = match background_color {
            Some(color) => Canvas::new_with_color(width, height, mode, color),
            None => match background {
                Background::White => Canvas::new(width, height, mode),
                Background::Empty => Canvas::new_empty(width, height, mode),
            },
        };
        let canvas = match canvas {
            Ok(canvas) => canvas,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    err.to_string(),
                )]));
            }
        };
        let label = canvas.background_label();
        let mut guard = self.canvas.lock().await;
        *guard = Some(canvas);
        let png = encode_or_internal(guard.as_ref().expect("canvas just stored"))?;
        image_result(
            format!(
                "已创建 {width}x{height} 像素图纸，背景为 {label}，已锁定 {} 色模式。直接用 draw_line / draw_rect / draw_triangle / draw_circle / draw_ellipse / draw_diamond / draw_curve / flood_fill / fill_background / draw_pixels / move_region / copy_region 画出图形；需要查色时再 list_colors。画完可查看返回的预览图，再继续修改和调整；画错可用 undo 撤回。绘制中途不能切换色表。fill_background 会把当前背景色更新为新的颜色。",
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
        description = "在当前像素图纸上按坐标填色。尽量使用像素绘制；其他工具仅用于大面积绘制。pixels 是一整段字符串，格式为「x y 颜色」三元组，可用空格或换行分隔，例如「0 0 正红\\n0 1 纯黑」。每次最多 1 MiB（1048576 字节）、65536 个落点，超限整批拒绝。可选 brush 为 1、2、4 或 8（默认 1）：在 (x,y) 涂满左上对齐的 N×N 方块，且 x、y 必须是 N 的倍数（笔刷 2 只能落在 0,2,4,6…），不对齐不吸附。坐标 (0,0) 为左上角。颜色必须使用创建时锁定色表中的中文名或 MARD 色号；用 - 清除为空格（不是 H2 白豆）；完整列表请调用 list_colors。格式错误、笔刷非法、未对齐、越界或颜色非法时整批拒绝。返回当前图纸放大 8 倍后的 PNG。"
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
        description = "画一条直线（Bresenham）。仅用于大面积绘制，细节请尽量用像素绘制。参数为起点 (x0,y0)、终点 (x1,y1) 和颜色；两端点必须在图纸内。可选 brush 为线宽 1、2、4 或 8（默认 1），从落点向右下扩展 N×N（与 draw_pixels 足迹一致），坐标不必对齐网格。返回 8 倍放大 PNG，可据预览继续修改和调整。"
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
        description = "画圆。仅用于大面积绘制，细节请尽量用像素绘制。左上角 (x,y) 为占据方形的锚点，直径 diameter 为 ≥1 的整数（超过 512 按 512）；圆占 diameter×diameter 格，格心落在圆内才计入。锚点必须在图纸内，右侧和下方超出部分裁剪。fill=true 填充，fill=false 取该填充的 1 格轮廓。描边可用 brush（1/2/4/8，默认 1）向右下加粗；填充忽略 brush。旧参数 cx、cy、radius 会报错。返回 8 倍放大 PNG，可据预览继续修改和调整。"
    )]
    async fn draw_circle(
        &self,
        Parameters(DrawCircleRequest {
            x,
            y,
            diameter,
            color,
            fill,
            brush,
        }): Parameters<DrawCircleRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| {
            let brush = BrushSize::parse(brush)?;
            canvas.paint_circle(x, y, diameter, &color, fill, brush)
        })
        .await
    }

    #[tool(
        description = "画椭圆。仅用于大面积绘制，细节请尽量用像素绘制。左上角 (x,y) 为占据矩形的锚点，宽 width、高 height 为 ≥1 的整数（超过 512 按 512）；椭圆占 width×height 格，格心落在椭圆内才计入。宽高相等时与同直径的圆逐格相同。锚点必须在图纸内，右侧和下方超出部分裁剪。fill=true 填充，fill=false 取该填充的 1 格轮廓。描边可用 brush（1/2/4/8，默认 1）向右下加粗；填充忽略 brush。旧参数 cx、cy、rx、ry 会报错。返回 8 倍放大 PNG，可据预览继续修改和调整。"
    )]
    async fn draw_ellipse(
        &self,
        Parameters(DrawEllipseRequest {
            x,
            y,
            width,
            height,
            color,
            fill,
            brush,
        }): Parameters<DrawEllipseRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| {
            let brush = BrushSize::parse(brush)?;
            canvas.paint_ellipse(x, y, width, height, &color, fill, brush)
        })
        .await
    }

    #[tool(
        description = "画像素菱形。仅用于大面积绘制，细节请尽量用像素绘制。(cx, cy) 是中心格，不是左上角锚定；rx、ry 是菱形半轴（从中心格沿水平或垂直方向延伸到边的格数，非负整数），宽高分别是 2*rx+1 与 2*ry+1，因此总是奇数。这里的 rx、ry 是菱形半轴，不是已废止的椭圆 rx、ry（椭圆已改用宽高，且可以是偶数）。格心落在菱形内才计入。半轴超过 255 按 255。中心格必须在图纸内，越过边缘的部分裁剪。fill=true 填充，fill=false 取该填充的 1 格轮廓。描边可用 brush（1/2/4/8，默认 1）向右下加粗；填充忽略 brush。返回 8 倍放大 PNG，可据预览继续修改和调整。"
    )]
    async fn draw_diamond(
        &self,
        Parameters(DrawDiamondRequest {
            cx,
            cy,
            rx,
            ry,
            color,
            fill,
            brush,
        }): Parameters<DrawDiamondRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| {
            let brush = BrushSize::parse(brush)?;
            canvas.paint_diamond(cx, cy, rx, ry, &color, fill, brush)
        })
        .await
    }

    #[tool(
        description = "画曲线。仅用于大面积绘制，细节请尽量用像素绘制。points 为 [x,y] 路径点列表，至少 2 个点且都必须在图纸内：2 点画直线；3 点为二次贝塞尔（中间点是控制点）；4 点为三次贝塞尔（中间两点是控制点）；5 点及以上为过点平滑样条（Catmull-Rom，曲线经过每一个点）。可选 brush 为线宽 1/2/4/8（默认 1），从落点向右下扩展 N×N（与 draw_pixels 一致），坐标不必对齐网格。返回 8 倍放大 PNG，可据预览继续修改和调整。"
    )]
    async fn draw_curve(
        &self,
        Parameters(DrawCurveRequest {
            points,
            color,
            brush,
        }): Parameters<DrawCurveRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| {
            let brush = BrushSize::parse(brush)?;
            let path: Vec<(u32, u32)> = points.iter().map(|[x, y]| (*x, *y)).collect();
            canvas.paint_curve(&path, &color, brush)
        })
        .await
    }

    #[tool(
        description = "撤回最近的绘制步骤。每次成功的绘制工具调用记为一步；draw_batch 整批只记一步；create_canvas 会清空历史且不可撤回。可选 steps 指定一次撤回几步，默认 1。返回撤回后的 8 倍预览图。"
    )]
    async fn undo(
        &self,
        Parameters(UndoRequest { steps }): Parameters<UndoRequest>,
    ) -> Result<CallToolResult, McpError> {
        let steps = steps.unwrap_or(1).max(1) as usize;
        let mut guard = self.canvas.lock().await;
        let Some(canvas) = guard.as_mut() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "尚未创建图纸。请先调用 create_canvas。",
            )]));
        };
        if canvas.undo_len() == 0 {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "没有可撤回的绘制步骤。",
            )]));
        }
        let available = canvas.undo_len().min(steps);
        match canvas.undo(steps) {
            Ok(remaining) => {
                let png = encode_or_internal(canvas)?;
                image_result(
                    format!(
                        "已撤回 {available} 步，剩余可撤回 {remaining} 步，当前图纸 {}x{}。",
                        canvas.width(),
                        canvas.height()
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
        description = "油漆桶：仅用于大面积填充，细节请尽量用像素绘制。从种子点 (x,y) 四连通填充与该点当前色号相同的区域，替换为 color（可用 - 清为空格）。空格与 H2 白豆分开处理。种子点必须在图纸内。若目标色号与种子色号相同则不改动。返回 8 倍放大 PNG。"
    )]
    async fn flood_fill(
        &self,
        Parameters(FloodFillRequest { x, y, color }): Parameters<FloodFillRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.with_canvas(|canvas| canvas.flood_fill(x, y, &color))
            .await
    }

    #[tool(
        description = "填充背景颜色：用 color 强行覆盖当前图纸的每一个格子，已有画面、空格和其他颜色都会被替换，并把已记录的当前背景色改为该颜色。需要已创建图纸。color 为当前锁定色表的中文名或 MARD 色号；不接受 -。颜色非法时画布不变，背景色也不变。与 flood_fill 不同，本工具不看种子点，也不停在色块边界。有格子变化，或当前背景色与新颜色不同时，记一次 undo。格子和背景色都已是该颜色时不改动、不记步。undo 会一并恢复之前的背景色。返回 8 倍放大 PNG。"
    )]
    async fn fill_background(
        &self,
        Parameters(FillBackgroundRequest { color }): Parameters<FillBackgroundRequest>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = self.canvas.lock().await;
        let Some(canvas) = guard.as_mut() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "尚未创建图纸。请先调用 create_canvas。",
            )]));
        };
        match canvas.fill_background(&color) {
            Ok(count) => {
                let label = canvas.background_label();
                let png = encode_or_internal(canvas)?;
                let width = canvas.width();
                let height = canvas.height();
                let area = width * height;
                image_result(
                    format!(
                        "已用背景色覆盖全部 {area} 个格子（改写 {count} 个），当前图纸 {width}x{height}。当前背景色为 {label}。"
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
        description = "移动一块矩形区域。源区域 (x0,y0)-(x1,y1) 含端点，反序会自动归一化。整块读取（含空格）后再把源区域改回当前背景色并写入目标，因此重叠搬运（如右移 1 格）结果正确；背景为空格时源区域留空格。不改变已记录的背景色。寻址必须且只能给一组：相对位移 dx、dy（可负），或目标左上角绝对坐标 tx、ty。源端点越界或目标区域越界时整次拒绝，画布不变、不记 undo。内容与搬运结果逐格相同时不改画布、不记 undo。返回 8 倍放大 PNG。"
    )]
    async fn move_region(
        &self,
        Parameters(request): Parameters<RegionRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.apply_region(request, true).await
    }

    #[tool(
        description = "复制一块矩形区域。参数与 move_region 相同：源区域含端点，反序自动归一化；整块读取（含空格）后覆盖目标，源区域保留。空格会覆盖目标原有内容。寻址必须且只能给一组：相对位移 dx、dy（可负），或目标左上角绝对坐标 tx、ty。源端点越界或目标区域越界时整次拒绝，画布不变、不记 undo。内容与搬运结果逐格相同时不改画布、不记 undo。不改变已记录的背景色。返回 8 倍放大 PNG。"
    )]
    async fn copy_region(
        &self,
        Parameters(request): Parameters<RegionRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.apply_region(request, false).await
    }

    async fn apply_region(
        &self,
        RegionRequest {
            x0,
            y0,
            x1,
            y1,
            dx,
            dy,
            tx,
            ty,
        }: RegionRequest,
        erase_source: bool,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = self.canvas.lock().await;
        let Some(canvas) = guard.as_mut() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "尚未创建图纸。请先调用 create_canvas。",
            )]));
        };
        let outcome = if erase_source {
            canvas.move_region(x0, y0, x1, y1, dx, dy, tx, ty)
        } else {
            canvas.copy_region(x0, y0, x1, y1, dx, dy, tx, ty)
        };
        match outcome {
            Ok(outcome) => {
                let png = encode_or_internal(canvas)?;
                image_result(region_message(&outcome, !erase_source), png)
            }
            Err(err) => Ok(CallToolResult::error(vec![ContentBlock::text(
                err.to_string(),
            )])),
        }
    }

    #[tool(
        description = "批量绘制：将多条有序操作一次提交、原子执行，只返回最终一张预览。适合需要多笔时一次画完，避免并行调用多个绘制工具造成顺序混乱。ops 数组按顺序执行，每项用 type 区分：line / rect / triangle / circle / ellipse / diamond / curve / flood_fill / fill_background / pixels / move_region / copy_region（参数与对应单笔工具相同）。任一步非法则整批拒绝且画布不变。成功时整批只记一次 undo。最多 256 步。尽量使用像素绘制；大面积再用本工具的图元。"
    )]
    async fn draw_batch(
        &self,
        Parameters(DrawBatchRequest { ops }): Parameters<DrawBatchRequest>,
    ) -> Result<CallToolResult, McpError> {
        if ops.len() > MAX_BATCH_OPS {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "批量操作不能超过 {MAX_BATCH_OPS} 步，当前 {} 步。",
                ops.len()
            ))]));
        }
        if ops.is_empty() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "ops 不能为空。",
            )]));
        }
        let mut guard = self.canvas.lock().await;
        let Some(canvas) = guard.as_mut() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "尚未创建图纸。请先调用 create_canvas。",
            )]));
        };
        let backup = canvas.clone();
        canvas.begin_batch();
        let mut total = 0usize;
        for (index, op) in ops.iter().enumerate() {
            match apply_draw_op(canvas, op) {
                Ok(count) => total += count,
                Err(err) => {
                    *canvas = backup;
                    return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                        "批量第 {} 步失败，整批已拒绝：{err}",
                        index + 1
                    ))]));
                }
            }
        }
        canvas.end_batch();
        let png = encode_or_internal(canvas)?;
        let width = canvas.width();
        let height = canvas.height();
        let steps = ops.len();
        image_result(
            format!("批量已执行 {steps} 步，共绘制 {total} 个像素，当前图纸 {width}x{height}。"),
            png,
        )
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
        description = "导出当前快照：output 为 pixel（默认，8 倍 PNG）、pattern（四边序号、逐格 MARD 色号、自中心每 10 格计数线，以及色号与数量图例的 PNG）或 both（两者）。空格留白、不计数；H2 是白豆。both 使用 filename 保存像素画，并在扩展名前加 -pattern 保存图纸。每个文件原子保存且不覆盖；多文件并非事务，如遇中途磁盘故障会返回已保存路径。未创建图纸时不可调用。可选 filename（如 cat.png）仅允许 ASCII 字母、数字、点、下划线和连字符；禁止正反斜杠、空名、.、..、尾点及 Windows 设备名（含扩展名），补全 .png 后最多 200 字节。省略则按时间戳命名，重名时最多尝试 100 个数字后缀。保存失败返回工具错误，成功返回保存路径和预览图。"
    )]
    async fn save_image(
        &self,
        Parameters(SaveImageRequest { filename, output }): Parameters<SaveImageRequest>,
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
        let mut exports = Vec::new();
        if matches!(output, ExportOutput::Pixel | ExportOutput::Both) {
            exports.push((name.clone(), encode_or_internal(&canvas)?));
        }
        if matches!(output, ExportOutput::Pattern | ExportOutput::Both) {
            let pattern_name = if matches!(output, ExportOutput::Both) {
                match sanitize_filename(&format!("{}-pattern.png", &name[..name.len() - 4])) {
                    Ok(name) => name,
                    Err(err) => return Ok(CallToolResult::error(vec![ContentBlock::text(err)])),
                }
            } else {
                name
            };
            let png = pattern::encode_png(&canvas).map_err(|err| {
                McpError::internal_error(format!("图纸 PNG 编码失败: {err}"), None)
            })?;
            exports.push((pattern_name, png));
        }
        let paths = match persist_exports(&self.output_dir, &exports, filename.is_none()) {
            Ok(paths) => paths,
            Err(err) => return Ok(CallToolResult::error(vec![ContentBlock::text(err)])),
        };
        let mut content = vec![ContentBlock::text(format!(
            "已保存 {}x{}，output={output:?}：{}",
            canvas.width(),
            canvas.height(),
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))];
        for (_, png) in exports {
            content.push(ContentBlock::image(STANDARD.encode(png), "image/png"));
        }
        Ok(CallToolResult::success(content))
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
                "PixelDraw：用强约束像素指令绘图，不要直接文生图。尽量使用像素绘制；其他图元仅用于大面积绘制。多笔绘制请一次调用 draw_batch（ops 有序数组，原子执行，只回最终预览）；单笔可用 draw_line / draw_rect / draw_triangle / draw_circle / draw_ellipse / draw_diamond / draw_curve / flood_fill / fill_background / draw_pixels / move_region / copy_region。禁止并行调用多个绘制工具，会打乱顺序。流程：create_canvas(width, height, palette?, background?, background_color?) 建图并锁定色表 → draw_batch 或单笔工具作画 → 看 8 倍预览继续修改调整（画错可用 undo 撤回）→ save_image 保存。矩形、三角形、圆、椭圆、菱形用 fill=true|false 区分填充/描边；直线、曲线与描边可用 brush=1|2|4|8 控制线宽（从落点向右下扩展 N×N，与 draw_pixels 足迹一致）。圆用左上角 (x,y) 与直径 diameter，椭圆用左上角 (x,y)、宽 width 与高 height（均为 ≥1 的整数，超过 512 按 512；锚点越界报错，右侧和下方裁剪；旧的 cx/cy/radius/rx/ry 会报错）。像素菱形 draw_diamond 用中心格 (cx,cy) 与菱形半轴 rx、ry（宽高为 2*rx+1 与 2*ry+1，总是奇数；超过 255 按 255；不是左上角锚定，也不是已废止的椭圆 rx/ry）。中心格必须在图纸内，越过边缘的部分裁剪。draw_curve 的 points 为 [x,y] 列表：2 点直线、3 点二次贝塞尔、4 点三次贝塞尔、≥5 点过点平滑样条。fill_background 用一种颜色覆盖整张图纸的每一个格子，并把当前背景色改为该颜色。move_region 把矩形区域（含空格）搬到 dx/dy 或 tx/ty 指定的位置，源区域改回当前背景色；copy_region 同样搬运但保留源区域。两组寻址只能给一组，源端点或目标越界整次拒绝。draw_pixels 与 flood_fill 可用 - 清除为空格（不是 H2 白豆）；background 可选 white（默认）或 empty；background_color 可选，用色表颜色铺满并记为当前背景色。list_colors 只在需要查色时调用。颜色只用当前模式的中文名或 MARD 色号。save_image 的 output 可选 pixel（默认 8 倍 PNG）、pattern（拼豆图纸）或 both；不覆盖已有文件。",
            )
    }
}

fn apply_draw_op(canvas: &mut Canvas, op: &DrawOp) -> Result<usize, CanvasError> {
    match op {
        DrawOp::Line {
            x0,
            y0,
            x1,
            y1,
            color,
            brush,
        } => {
            let brush = BrushSize::parse(*brush)?;
            canvas.paint_line(*x0, *y0, *x1, *y1, color, brush)
        }
        DrawOp::Rect {
            x0,
            y0,
            x1,
            y1,
            color,
            fill,
            brush,
        } => {
            let brush = BrushSize::parse(*brush)?;
            canvas.paint_rect(*x0, *y0, *x1, *y1, color, *fill, brush)
        }
        DrawOp::Triangle {
            x0,
            y0,
            x1,
            y1,
            x2,
            y2,
            color,
            fill,
            brush,
        } => {
            let brush = BrushSize::parse(*brush)?;
            canvas.paint_triangle(*x0, *y0, *x1, *y1, *x2, *y2, color, *fill, brush)
        }
        DrawOp::Circle {
            x,
            y,
            diameter,
            color,
            fill,
            brush,
        } => {
            let brush = BrushSize::parse(*brush)?;
            canvas.paint_circle(*x, *y, *diameter, color, *fill, brush)
        }
        DrawOp::Ellipse {
            x,
            y,
            width,
            height,
            color,
            fill,
            brush,
        } => {
            let brush = BrushSize::parse(*brush)?;
            canvas.paint_ellipse(*x, *y, *width, *height, color, *fill, brush)
        }
        DrawOp::Diamond {
            cx,
            cy,
            rx,
            ry,
            color,
            fill,
            brush,
        } => {
            let brush = BrushSize::parse(*brush)?;
            canvas.paint_diamond(*cx, *cy, *rx, *ry, color, *fill, brush)
        }
        DrawOp::Curve {
            points,
            color,
            brush,
        } => {
            let brush = BrushSize::parse(*brush)?;
            let path: Vec<(u32, u32)> = points.iter().map(|[x, y]| (*x, *y)).collect();
            canvas.paint_curve(&path, color, brush)
        }
        DrawOp::FloodFill { x, y, color } => canvas.flood_fill(*x, *y, color),
        DrawOp::FillBackground { color } => canvas.fill_background(color),
        DrawOp::MoveRegion {
            x0,
            y0,
            x1,
            y1,
            dx,
            dy,
            tx,
            ty,
        } => canvas
            .move_region(*x0, *y0, *x1, *y1, *dx, *dy, *tx, *ty)
            .map(|outcome| outcome.written),
        DrawOp::CopyRegion {
            x0,
            y0,
            x1,
            y1,
            dx,
            dy,
            tx,
            ty,
        } => canvas
            .copy_region(*x0, *y0, *x1, *y1, *dx, *dy, *tx, *ty)
            .map(|outcome| outcome.written),
        DrawOp::Pixels { pixels, brush } => {
            let updates =
                parse_pixels(pixels).map_err(|message| CanvasError::InvalidOp { message })?;
            let brush = BrushSize::parse(*brush)?;
            canvas.paint(&updates, brush)
        }
    }
}

fn encode_or_internal(canvas: &Canvas) -> Result<Vec<u8>, McpError> {
    canvas
        .encode_png_8x()
        .map_err(|err| McpError::internal_error(format!("PNG 编码失败: {err}"), None))
}

fn region_message(outcome: &RegionOutcome, copy: bool) -> String {
    if !outcome.changed {
        return "内容无变化：源区域与目标内容一致，未修改画布。".to_string();
    }
    let verb = if copy { "复制到" } else { "移动到" };
    format!(
        "已将 ({},{})-({},{}) 的内容{verb} ({},{})-({},{})。",
        outcome.x0,
        outcome.y0,
        outcome.x1,
        outcome.y1,
        outcome.nx0,
        outcome.ny0,
        outcome.nx1,
        outcome.ny1
    )
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
    for &[x_raw, y_raw, color] in tokens.as_chunks::<3>().0 {
        let x = x_raw
            .parse::<u32>()
            .map_err(|_| format!("无效的 X 坐标「{}」，必须是非负整数。", x_raw))?;
        let y = y_raw
            .parse::<u32>()
            .map_err(|_| format!("无效的 Y 坐标「{}」，必须是非负整数。", y_raw))?;
        updates.push(PixelUpdate {
            x,
            y,
            color: color.to_string(),
        });
    }
    Ok(updates)
}

/// Preflight the complete set before writing. Each file is atomic, but a set is
/// not a filesystem transaction: report already saved files on a late failure.
fn persist_exports(
    output_dir: &Path,
    exports: &[(String, Vec<u8>)],
    autogenerated: bool,
) -> Result<Vec<PathBuf>, String> {
    if exports.len() == 1 {
        return persist_png(output_dir, &exports[0].0, autogenerated, &exports[0].1)
            .map(|p| vec![p]);
    }
    let attempts = if autogenerated { MAX_SAVE_SUFFIX } else { 0 };
    for suffix in 0..=attempts {
        let names: Vec<String> = exports
            .iter()
            .map(|(name, _)| {
                if suffix == 0 {
                    name.clone()
                } else {
                    format!("{}-{suffix}.png", &name[..name.len() - 4])
                }
            })
            .collect();
        let mut collision = false;
        for name in &names {
            if output_dir
                .join(name)
                .try_exists()
                .map_err(|e| e.to_string())?
            {
                collision = true;
            }
        }
        if collision {
            if suffix < attempts {
                continue;
            }
            return Err("保存失败：至少一个输出文件已存在；未写入任何文件".into());
        }
        let mut paths = Vec::new();
        for (name, (_, bytes)) in names.iter().zip(exports) {
            match persist_png(output_dir, name, false, bytes) {
                Ok(path) => paths.push(path),
                Err(err) => return Err(format!("{err}；本次已保存：{paths:?}")),
            }
        }
        return Ok(paths);
    }
    unreachable!("bounded export loop")
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

    #[tokio::test]
    async fn export_modes_and_companion_collision() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        create(&server).await;
        for (output, name, image_count) in [
            (ExportOutput::Pixel, "pixel", 1),
            (ExportOutput::Pattern, "chart", 1),
            (ExportOutput::Both, "pair.PNG", 2),
        ] {
            let r = server
                .save_image(Parameters(SaveImageRequest {
                    filename: Some(name.into()),
                    output,
                }))
                .await
                .unwrap();
            assert_ne!(r.is_error, Some(true));
            assert_eq!(
                r.content.iter().filter(|c| c.as_image().is_some()).count(),
                image_count
            );
        }
        assert!(dir.path().join("pair.PNG").exists());
        assert!(dir.path().join("pair-pattern.png").exists());
        let pixel = image::open(dir.path().join("pixel.png")).unwrap();
        let chart = image::open(dir.path().join("chart.png")).unwrap();
        assert_eq!((pixel.width(), pixel.height()), (32, 32));
        assert!(chart.width() > pixel.width());
        fs::write(dir.path().join("blocked-pattern.png"), b"keep").unwrap();
        let r = server
            .save_image(Parameters(SaveImageRequest {
                filename: Some("blocked".into()),
                output: ExportOutput::Both,
            }))
            .await
            .unwrap();
        assert_eq!(r.is_error, Some(true));
        assert!(!dir.path().join("blocked.png").exists());
        assert_eq!(
            fs::read(dir.path().join("blocked-pattern.png")).unwrap(),
            b"keep"
        );
        // Validate the derived filename before writing the first file.
        let r = server
            .save_image(Parameters(SaveImageRequest {
                filename: Some("a".repeat(196)),
                output: ExportOutput::Both,
            }))
            .await
            .unwrap();
        assert_eq!(r.is_error, Some(true));
        assert!(!dir.path().join(format!("{}.png", "a".repeat(196))).exists());
    }

    #[test]
    fn defaults_and_invalid_output_are_typed() {
        let req: SaveImageRequest = serde_json::from_str("{}").unwrap();
        assert!(matches!(req.output, ExportOutput::Pixel));
        assert!(serde_json::from_str::<SaveImageRequest>(r#"{"output":"unknown"}"#).is_err());
        let req: CreateCanvasRequest = serde_json::from_str(r#"{"width":1,"height":1}"#).unwrap();
        assert!(matches!(req.background, Background::White));
        assert!(req.background_color.is_none());
        let req: CreateCanvasRequest =
            serde_json::from_str(r#"{"width":1,"height":1,"background_color":"正红"}"#).unwrap();
        assert!(matches!(req.background, Background::White));
        assert_eq!(req.background_color.as_deref(), Some("正红"));
        assert!(
            serde_json::from_str::<CreateCanvasRequest>(
                r#"{"width":1,"height":1,"background":"unknown"}"#
            )
            .is_err()
        );
        let op: DrawOp =
            serde_json::from_str(r#"{"type":"fill_background","color":"正红"}"#).unwrap();
        assert!(matches!(op, DrawOp::FillBackground { color } if color == "正红"));
    }

    #[test]
    fn circle_and_ellipse_reject_old_and_missing_parameters() {
        let circle: DrawCircleRequest =
            serde_json::from_str(r#"{"x":1,"y":2,"diameter":4,"color":"纯黑","fill":true}"#)
                .unwrap();
        assert_eq!((circle.x, circle.y, circle.diameter), (1, 2, 4));
        assert!(circle.brush.is_none());
        let ellipse: DrawEllipseRequest = serde_json::from_str(
            r#"{"x":0,"y":0,"width":3,"height":5,"color":"纯黑","fill":false,"brush":2}"#,
        )
        .unwrap();
        assert_eq!(
            (ellipse.width, ellipse.height, ellipse.brush),
            (3, 5, Some(2))
        );

        for json in [
            r#"{"x":0,"y":0,"diameter":4,"color":"纯黑","fill":true,"cx":1}"#,
            r#"{"x":0,"y":0,"diameter":4,"color":"纯黑","fill":true,"cy":1}"#,
            r#"{"x":0,"y":0,"diameter":4,"color":"纯黑","fill":true,"radius":1}"#,
            r#"{"cx":1,"cy":1,"radius":2,"color":"纯黑","fill":true}"#,
            r#"{"x":0,"y":0,"color":"纯黑","fill":true}"#,
            r#"{"x":0,"y":0,"diameter":-1,"color":"纯黑","fill":true}"#,
        ] {
            assert!(
                serde_json::from_str::<DrawCircleRequest>(json).is_err(),
                "{json}"
            );
        }
        for json in [
            r#"{"x":0,"y":0,"width":4,"height":2,"color":"纯黑","fill":true,"rx":1}"#,
            r#"{"x":0,"y":0,"width":4,"height":2,"color":"纯黑","fill":true,"ry":1}"#,
            r#"{"cx":1,"cy":1,"rx":2,"ry":1,"color":"纯黑","fill":true}"#,
            r#"{"x":0,"y":0,"width":4,"color":"纯黑","fill":true}"#,
            r#"{"x":0,"y":0,"width":4,"height":-2,"color":"纯黑","fill":true}"#,
        ] {
            assert!(
                serde_json::from_str::<DrawEllipseRequest>(json).is_err(),
                "{json}"
            );
        }
        assert!(serde_json::from_str::<DrawOp>(
            r#"{"type":"circle","x":0,"y":0,"diameter":4,"color":"纯黑","fill":true,"radius":1}"#
        )
        .is_err());
        assert!(serde_json::from_str::<DrawOp>(
            r#"{"type":"ellipse","x":0,"y":0,"width":4,"height":2,"color":"纯黑","fill":true,"cx":0}"#
        )
        .is_err());
        assert!(
            serde_json::from_str::<DrawOp>(
                r#"{"type":"circle","cx":0,"cy":0,"radius":2,"color":"纯黑","fill":true}"#
            )
            .is_err()
        );
        let op: DrawOp = serde_json::from_str(
            r#"{"type":"circle","x":1,"y":2,"diameter":8,"color":"纯黑","fill":false}"#,
        )
        .unwrap();
        assert!(matches!(
            op,
            DrawOp::Circle {
                x: 1,
                y: 2,
                diameter: 8,
                fill: false,
                ..
            }
        ));
    }

    #[test]
    fn diamond_rejects_unknown_fields_and_keeps_half_axes() {
        let diamond: DrawDiamondRequest = serde_json::from_str(
            r#"{"cx":3,"cy":4,"rx":2,"ry":5,"color":"纯黑","fill":true,"brush":2}"#,
        )
        .unwrap();
        assert_eq!(
            (
                diamond.cx,
                diamond.cy,
                diamond.rx,
                diamond.ry,
                diamond.brush
            ),
            (3, 4, 2, 5, Some(2))
        );
        for json in [
            r#"{"cx":1,"cy":1,"rx":2,"ry":2,"color":"纯黑","fill":true,"x":0}"#,
            r#"{"cx":1,"cy":1,"rx":2,"ry":2,"color":"纯黑","fill":true,"y":0}"#,
            r#"{"cx":1,"cy":1,"rx":2,"ry":2,"color":"纯黑","fill":true,"width":5}"#,
            r#"{"cx":1,"cy":1,"rx":2,"ry":2,"color":"纯黑","fill":true,"height":5}"#,
            r#"{"cx":1,"cy":1,"rx":2,"ry":2,"color":"纯黑","fill":true,"diameter":5}"#,
            r#"{"cx":1,"cy":1,"rx":2,"ry":2,"color":"纯黑","fill":true,"radius":2}"#,
            r#"{"x":0,"y":0,"width":5,"height":5,"color":"纯黑","fill":true}"#,
            r#"{"cx":1,"cy":1,"color":"纯黑","fill":true}"#,
            r#"{"cx":1,"cy":1,"rx":-1,"ry":2,"color":"纯黑","fill":true}"#,
        ] {
            assert!(
                serde_json::from_str::<DrawDiamondRequest>(json).is_err(),
                "{json}"
            );
        }
        assert!(serde_json::from_str::<DrawOp>(
            r#"{"type":"diamond","cx":1,"cy":1,"rx":2,"ry":2,"color":"纯黑","fill":true,"width":4}"#
        )
        .is_err());
        assert!(serde_json::from_str::<DrawOp>(
            r#"{"type":"diamond","cx":1,"cy":1,"rx":2,"ry":2,"color":"纯黑","fill":true,"radius":1}"#
        )
        .is_err());
        let op: DrawOp = serde_json::from_str(
            r#"{"type":"diamond","cx":4,"cy":5,"rx":0,"ry":3,"color":"纯黑","fill":false}"#,
        )
        .unwrap();
        assert!(matches!(
            op,
            DrawOp::Diamond {
                cx: 4,
                cy: 5,
                rx: 0,
                ry: 3,
                fill: false,
                ..
            }
        ));
    }

    #[test]
    fn export_set_retries_names_and_reports_late_failures() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a-pattern.png"), b"keep").unwrap();
        let exports = vec![("a.png".into(), vec![1]), ("a-pattern.png".into(), vec![2])];
        let paths = persist_exports(dir.path(), &exports, true).unwrap();
        assert_eq!(
            paths,
            vec![
                dir.path().join("a-1.png"),
                dir.path().join("a-pattern-1.png")
            ]
        );
        // A nested invalid target simulates an I/O failure after the first persist.
        let exports = vec![
            ("first.png".into(), vec![1]),
            ("missing/second.png".into(), vec![2]),
        ];
        let err = persist_exports(dir.path(), &exports, false).unwrap_err();
        assert!(err.contains("first.png"));
        assert_eq!(fs::read(dir.path().join("first.png")).unwrap(), vec![1]);
    }

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
        assert_eq!(img.get_pixel(0, 0), &image::Rgb([0xFC, 0x3D, 0x46]));
        assert_eq!(img.get_pixel(8, 16), &image::Rgb([0, 0, 0]));
    }

    #[test]
    fn parsed_string_paints_with_brush_two() {
        let mut canvas = Canvas::new(8, 8, PaletteMode::Colors144).unwrap();
        let updates = parse_pixels("0 0 正红\n2 0 纯黑").unwrap();
        assert_eq!(canvas.paint(&updates, BrushSize::Two).unwrap(), 8);
        let png = canvas.encode_png_8x().unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(8, 8), &image::Rgb([0xFC, 0x3D, 0x46]));
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
                background: Background::White,
                background_color: None,
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
                output: ExportOutput::Pixel,
                filename: Some("demo".into()),
            }))
            .await
            .unwrap();
        assert_eq!(response_png(&result), expected);
        assert_eq!(fs::read(output.join("demo.png")).unwrap(), expected);
        fs::write(output.join("demo.png"), b"existing").unwrap();
        let result = server
            .save_image(Parameters(SaveImageRequest {
                output: ExportOutput::Pixel,
                filename: Some("demo".into()),
            }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(fs::read(output.join("demo.png")).unwrap(), b"existing");
        assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
        let result = server
            .save_image(Parameters(SaveImageRequest {
                output: ExportOutput::Pixel,
                filename: None,
            }))
            .await
            .unwrap();
        assert_eq!(response_png(&result), expected);
        assert_eq!(fs::read_dir(&output).unwrap().count(), 2);
        for filename in ["", "../escape.png", r"..\escape.png", "NUL.png"] {
            let result = server
                .save_image(Parameters(SaveImageRequest {
                    output: ExportOutput::Pixel,
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
            .save_image(Parameters(SaveImageRequest {
                output: ExportOutput::Pixel,
                filename: None,
            }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(fs::read(output).unwrap(), b"existing");
    }

    #[tokio::test]
    async fn draw_batch_is_atomic_and_ordered() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        create(&server).await;

        let ok = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::Line {
                        x0: 0,
                        y0: 0,
                        x1: 3,
                        y1: 0,
                        color: "纯黑".into(),
                        brush: Some(1),
                    },
                    DrawOp::Rect {
                        x0: 0,
                        y0: 1,
                        x1: 3,
                        y1: 3,
                        color: "中国红".into(),
                        fill: true,
                        brush: None,
                    },
                ],
            }))
            .await
            .unwrap();
        assert_ne!(ok.is_error, Some(true));
        let png = response_png(&ok);
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &image::Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(8, 16), &image::Rgb([0xE7, 0x00, 0x2F]));

        let before = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .encode_png_8x()
            .unwrap();
        let before_codes = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .codes()
            .to_vec();
        let bad = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::Circle {
                        x: 1,
                        y: 1,
                        diameter: 1,
                        color: "明黄".into(),
                        fill: true,
                        brush: None,
                    },
                    DrawOp::Line {
                        x0: 0,
                        y0: 0,
                        x1: 1,
                        y1: 1,
                        color: "彩虹".into(),
                        brush: Some(1),
                    },
                ],
            }))
            .await
            .unwrap();
        assert_eq!(bad.is_error, Some(true));
        let after = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .encode_png_8x()
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes(),
            before_codes
        );
    }

    #[tokio::test]
    async fn circle_and_ellipse_are_one_undo_and_bad_batch_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        server
            .create_canvas(Parameters(CreateCanvasRequest {
                background: Background::White,
                background_color: None,
                width: 16,
                height: 16,
                palette: Some("24".into()),
            }))
            .await
            .unwrap();
        let blank = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .encode_png_8x()
            .unwrap();

        let drawn = server
            .draw_circle(Parameters(DrawCircleRequest {
                x: 1,
                y: 1,
                diameter: 9,
                color: "纯黑".into(),
                fill: true,
                brush: None,
            }))
            .await
            .unwrap();
        assert_ne!(drawn.is_error, Some(true));
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);

        let ellipse = server
            .draw_ellipse(Parameters(DrawEllipseRequest {
                x: 0,
                y: 0,
                width: 9,
                height: 9,
                color: "中国红".into(),
                fill: true,
                brush: Some(8),
            }))
            .await
            .unwrap();
        assert_ne!(ellipse.is_error, Some(true));
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 2);
        // Same width and height as the circle, but a different anchor, so the
        // second step is visible. One undo removes only the ellipse.
        let undone = server
            .undo(Parameters(UndoRequest { steps: Some(1) }))
            .await
            .unwrap();
        assert_ne!(undone.is_error, Some(true));
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);

        let before = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .encode_png_8x()
            .unwrap();
        let bad = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::Line {
                        x0: 0,
                        y0: 0,
                        x1: 4,
                        y1: 0,
                        color: "明黄".into(),
                        brush: Some(1),
                    },
                    DrawOp::Circle {
                        x: 0,
                        y: 0,
                        diameter: 0,
                        color: "纯黑".into(),
                        fill: true,
                        brush: None,
                    },
                ],
            }))
            .await
            .unwrap();
        assert_eq!(bad.is_error, Some(true));
        assert!(bad.content[0].as_text().unwrap().text.contains("直径"));
        let oob = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![DrawOp::Ellipse {
                    x: 16,
                    y: 0,
                    width: 4,
                    height: 4,
                    color: "纯黑".into(),
                    fill: false,
                    brush: None,
                }],
            }))
            .await
            .unwrap();
        assert_eq!(oob.is_error, Some(true));
        let after = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .encode_png_8x()
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);
        server
            .undo(Parameters(UndoRequest { steps: None }))
            .await
            .unwrap();
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .encode_png_8x()
                .unwrap(),
            blank
        );
    }

    #[tokio::test]
    async fn diamond_is_one_undo_and_bad_batch_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        let missing = server
            .draw_diamond(Parameters(DrawDiamondRequest {
                cx: 1,
                cy: 1,
                rx: 1,
                ry: 1,
                color: "纯黑".into(),
                fill: true,
                brush: None,
            }))
            .await
            .unwrap();
        assert_eq!(missing.is_error, Some(true));
        assert!(
            missing.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("create_canvas")
        );

        server
            .create_canvas(Parameters(CreateCanvasRequest {
                background: Background::White,
                background_color: None,
                width: 16,
                height: 16,
                palette: Some("24".into()),
            }))
            .await
            .unwrap();
        let drawn = server
            .draw_diamond(Parameters(DrawDiamondRequest {
                cx: 6,
                cy: 6,
                rx: 3,
                ry: 2,
                color: "纯黑".into(),
                fill: true,
                brush: None,
            }))
            .await
            .unwrap();
        assert_ne!(drawn.is_error, Some(true));
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);
        let before = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .encode_png_8x()
            .unwrap();
        let before_codes = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .codes()
            .to_vec();

        let bad = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::Line {
                        x0: 0,
                        y0: 0,
                        x1: 4,
                        y1: 0,
                        color: "明黄".into(),
                        brush: Some(1),
                    },
                    DrawOp::Diamond {
                        cx: 16,
                        cy: 0,
                        rx: 2,
                        ry: 2,
                        color: "纯黑".into(),
                        fill: true,
                        brush: None,
                    },
                ],
            }))
            .await
            .unwrap();
        assert_eq!(bad.is_error, Some(true));
        assert!(
            bad.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("整批已拒绝")
        );
        let unknown = server
            .draw_diamond(Parameters(DrawDiamondRequest {
                cx: 1,
                cy: 1,
                rx: 2,
                ry: 2,
                color: "彩虹".into(),
                fill: false,
                brush: Some(4),
            }))
            .await
            .unwrap();
        assert_eq!(unknown.is_error, Some(true));
        let after = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .encode_png_8x()
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes(),
            before_codes
        );
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);

        let batched = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::Diamond {
                        cx: 2,
                        cy: 2,
                        rx: 0,
                        ry: 2,
                        color: "中国红".into(),
                        fill: true,
                        brush: Some(8),
                    },
                    DrawOp::Diamond {
                        cx: 12,
                        cy: 4,
                        rx: 2,
                        ry: 0,
                        color: "明黄".into(),
                        fill: false,
                        brush: None,
                    },
                ],
            }))
            .await
            .unwrap();
        assert_ne!(batched.is_error, Some(true));
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 2);
        server
            .undo(Parameters(UndoRequest { steps: Some(1) }))
            .await
            .unwrap();
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .encode_png_8x()
                .unwrap(),
            before
        );
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);
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
                color: "纯黑".into(),
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
                color: "中国红".into(),
                fill: true,
                brush: None,
            }))
            .await
            .unwrap();
        assert_ne!(filled.is_error, Some(true));
        let png = response_png(&filled);
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &image::Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(8, 8), &image::Rgb([0xE7, 0x00, 0x2F]));

        let seed = server
            .flood_fill(Parameters(FloodFillRequest {
                x: 0,
                y: 0,
                color: "明黄".into(),
            }))
            .await
            .unwrap();
        assert_ne!(seed.is_error, Some(true));
        let png = response_png(&seed);
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), &image::Rgb([0xFB, 0xED, 0x56]));
    }

    #[tokio::test]
    async fn undo_restores_after_draw_and_batch() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        let blank = response_png(&create(&server).await);

        let painted = server
            .draw_line(Parameters(DrawLineRequest {
                x0: 0,
                y0: 0,
                x1: 3,
                y1: 0,
                color: "纯黑".into(),
                brush: Some(1),
            }))
            .await
            .unwrap();
        let painted_png = response_png(&painted);
        assert_ne!(painted_png, blank);

        let undone = server
            .undo(Parameters(UndoRequest { steps: None }))
            .await
            .unwrap();
        assert_ne!(undone.is_error, Some(true));
        assert_eq!(response_png(&undone), blank);
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 0);

        let again = server
            .undo(Parameters(UndoRequest { steps: Some(1) }))
            .await
            .unwrap();
        assert_eq!(again.is_error, Some(true));

        server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::Line {
                        x0: 0,
                        y0: 0,
                        x1: 3,
                        y1: 0,
                        color: "纯黑".into(),
                        brush: Some(1),
                    },
                    DrawOp::Rect {
                        x0: 0,
                        y0: 1,
                        x1: 3,
                        y1: 3,
                        color: "中国红".into(),
                        fill: true,
                        brush: None,
                    },
                ],
            }))
            .await
            .unwrap();
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);
        let undone = server
            .undo(Parameters(UndoRequest { steps: None }))
            .await
            .unwrap();
        assert_eq!(response_png(&undone), blank);
    }

    #[tokio::test]
    async fn draw_curve_paints_path_and_undo_reverts_it() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        let blank = response_png(&create(&server).await);
        let curved = server
            .draw_curve(Parameters(DrawCurveRequest {
                points: vec![[0, 3], [1, 0], [3, 3]],
                color: "纯黑".into(),
                brush: Some(1),
            }))
            .await
            .unwrap();
        assert_ne!(curved.is_error, Some(true));
        let png = response_png(&curved);
        assert_ne!(png, blank);
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 24), &image::Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(24, 24), &image::Rgb([0, 0, 0]));

        let bad = server
            .draw_curve(Parameters(DrawCurveRequest {
                points: vec![[0, 0]],
                color: "纯黑".into(),
                brush: None,
            }))
            .await
            .unwrap();
        assert_eq!(bad.is_error, Some(true));

        let undone = server
            .undo(Parameters(UndoRequest { steps: None }))
            .await
            .unwrap();
        assert_eq!(response_png(&undone), blank);
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
            .save_image(Parameters(SaveImageRequest {
                output: ExportOutput::Pixel,
                filename: None,
            }))
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
                    background: Background::White,
                    background_color: None,
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
            ("0 0 纯白".into(), None),
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

    #[tokio::test]
    async fn fill_background_overwrites_canvas_and_undoes_once() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        let missing = server
            .fill_background(Parameters(FillBackgroundRequest {
                color: "正红".into(),
            }))
            .await
            .unwrap();
        assert_eq!(missing.is_error, Some(true));
        assert!(
            missing.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("尚未创建图纸")
        );

        let blank = response_png(&create(&server).await);
        let painted = server
            .draw_pixels(Parameters(DrawPixelsRequest {
                pixels: "0 0 纯黑\n1 1 正红\n3 3 明黄".into(),
                brush: Some(1),
            }))
            .await
            .unwrap();
        let painted_png = response_png(&painted);
        assert_ne!(painted_png, blank);
        let before_codes = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .codes()
            .to_vec();
        let undo_before = server.canvas.lock().await.as_ref().unwrap().undo_len();

        let rejected = server
            .fill_background(Parameters(FillBackgroundRequest {
                color: "彩虹".into(),
            }))
            .await
            .unwrap();
        assert_eq!(rejected.is_error, Some(true));
        assert!(
            rejected.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("未知颜色")
        );
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes(),
            before_codes
        );
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().undo_len(),
            undo_before
        );

        let filled = server
            .fill_background(Parameters(FillBackgroundRequest {
                color: "中国红".into(),
            }))
            .await
            .unwrap();
        let text = &filled.content[0].as_text().unwrap().text;
        assert!(text.contains("覆盖全部 16 个格子"));
        assert!(text.contains("改写 16 个"));
        let png = response_png(&filled);
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (32, 32));
        assert!(img.pixels().all(|pixel| pixel.0 == [0xE7, 0x00, 0x2F]));
        assert!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .codes()
                .iter()
                .all(|code| *code == Some("F5"))
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("F5")
        );
        assert!(text.contains("当前背景色为 中国红（F5）"));
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().undo_len(),
            undo_before + 1
        );

        let same = server
            .fill_background(Parameters(FillBackgroundRequest { color: "f5".into() }))
            .await
            .unwrap();
        assert!(
            same.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("改写 0 个")
        );
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().undo_len(),
            undo_before + 1
        );

        let undone = server
            .undo(Parameters(UndoRequest { steps: None }))
            .await
            .unwrap();
        assert_eq!(response_png(&undone), painted_png);
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes(),
            before_codes
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("H2")
        );

        let backup = server.canvas.lock().await.clone();
        let bad_batch = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::FillBackground {
                        color: "纯黑".into(),
                    },
                    DrawOp::Line {
                        x0: 0,
                        y0: 0,
                        x1: 1,
                        y1: 1,
                        color: "彩虹".into(),
                        brush: None,
                    },
                ],
            }))
            .await
            .unwrap();
        assert_eq!(bad_batch.is_error, Some(true));
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes(),
            backup.as_ref().unwrap().codes()
        );
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().undo_len(),
            backup.as_ref().unwrap().undo_len()
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            backup.as_ref().unwrap().background_code()
        );

        server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::FillBackground {
                        color: "明黄".into(),
                    },
                    DrawOp::Pixels {
                        pixels: "0 0 纯黑".into(),
                        brush: Some(1),
                    },
                ],
            }))
            .await
            .unwrap();
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 2);
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("A4")
        );
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes()[0],
            Some("H7")
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .codes()
                .iter()
                .filter(|code| **code == Some("A4"))
                .count(),
            15
        );
        let reverted = server
            .undo(Parameters(UndoRequest { steps: Some(1) }))
            .await
            .unwrap();
        assert_eq!(response_png(&reverted), painted_png);
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("H2")
        );

        server
            .create_canvas(Parameters(CreateCanvasRequest {
                background: Background::Empty,
                background_color: None,
                width: 2,
                height: 2,
                palette: Some("24".into()),
            }))
            .await
            .unwrap();
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 0);
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            None
        );
        let empty_filled = server
            .fill_background(Parameters(FillBackgroundRequest {
                color: "纯黑".into(),
            }))
            .await
            .unwrap();
        assert!(
            empty_filled.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("改写 4 个")
        );
        let empty_png = response_png(&empty_filled);
        let empty_img = image::load_from_memory(&empty_png).unwrap().to_rgba8();
        assert_eq!(empty_img.dimensions(), (16, 16));
        assert!(empty_img.pixels().all(|pixel| pixel.0 == [0, 0, 0, 255]));
        assert!(
            empty_filled.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("当前背景色为 纯黑（H7）")
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("H7")
        );
        let restored = server
            .undo(Parameters(UndoRequest { steps: None }))
            .await
            .unwrap();
        let restored_img = image::load_from_memory(&response_png(&restored))
            .unwrap()
            .to_rgba8();
        assert!(restored_img.pixels().all(|pixel| pixel.0[3] == 0));
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            None
        );
    }

    #[tokio::test]
    async fn create_canvas_background_color_is_optional_and_tracks_refill() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        let created = server
            .create_canvas(Parameters(CreateCanvasRequest {
                width: 2,
                height: 2,
                palette: Some("24".into()),
                background: Background::White,
                background_color: Some("正红".into()),
            }))
            .await
            .unwrap();
        assert_ne!(created.is_error, Some(true));
        assert!(
            created.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("背景为 正红（F2）")
        );
        let img = image::load_from_memory(&response_png(&created))
            .unwrap()
            .to_rgb8();
        assert!(img.pixels().all(|pixel| pixel.0 == [0xFC, 0x3D, 0x46]));
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("F2")
        );
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 0);

        let white = server
            .create_canvas(Parameters(CreateCanvasRequest {
                width: 2,
                height: 2,
                palette: Some("24".into()),
                background: Background::White,
                background_color: None,
            }))
            .await
            .unwrap();
        assert!(
            white.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("背景为 雪白（H2）")
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("H2")
        );

        let rejected = server
            .create_canvas(Parameters(CreateCanvasRequest {
                width: 2,
                height: 2,
                palette: Some("24".into()),
                background: Background::White,
                background_color: Some("淡鹅黄".into()),
            }))
            .await
            .unwrap();
        assert_eq!(rejected.is_error, Some(true));
        assert!(
            rejected.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("未知颜色")
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("H2")
        );

        let conflict = server
            .create_canvas(Parameters(CreateCanvasRequest {
                width: 2,
                height: 2,
                palette: Some("24".into()),
                background: Background::Empty,
                background_color: Some("正红".into()),
            }))
            .await
            .unwrap();
        assert_eq!(conflict.is_error, Some(true));
        assert!(
            conflict.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("background_color")
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("H2")
        );

        let blank = server
            .create_canvas(Parameters(CreateCanvasRequest {
                width: 2,
                height: 2,
                palette: Some("24".into()),
                background: Background::Empty,
                background_color: Some("   ".into()),
            }))
            .await
            .unwrap();
        assert_ne!(blank.is_error, Some(true));
        assert!(
            blank.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("背景为 空格")
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            None
        );

        server
            .create_canvas(Parameters(CreateCanvasRequest {
                width: 2,
                height: 2,
                palette: Some("144".into()),
                background: Background::White,
                background_color: Some("f2".into()),
            }))
            .await
            .unwrap();
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("F2")
        );
        server
            .draw_pixels(Parameters(DrawPixelsRequest {
                pixels: "0 0 纯黑".into(),
                brush: Some(1),
            }))
            .await
            .unwrap();
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("F2")
        );

        let filled = server
            .fill_background(Parameters(FillBackgroundRequest {
                color: "中国红".into(),
            }))
            .await
            .unwrap();
        assert!(
            filled.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("当前背景色为 中国红（F5）")
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("F5")
        );
        server
            .undo(Parameters(UndoRequest { steps: None }))
            .await
            .unwrap();
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            Some("F2")
        );
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes()[0],
            Some("H7")
        );
    }

    #[allow(clippy::too_many_arguments)] // Mirrors RegionRequest fields.
    fn region(
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        dx: Option<i32>,
        dy: Option<i32>,
        tx: Option<i32>,
        ty: Option<i32>,
    ) -> RegionRequest {
        RegionRequest {
            x0,
            y0,
            x1,
            y1,
            dx,
            dy,
            tx,
            ty,
        }
    }

    fn tool_text(result: &CallToolResult) -> &str {
        &result.content[0].as_text().unwrap().text
    }

    #[tokio::test]
    async fn region_address_requires_one_pair_and_a_canvas() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        let missing = server
            .move_region(Parameters(region(0, 0, 0, 0, Some(1), Some(0), None, None)))
            .await
            .unwrap();
        assert_eq!(missing.is_error, Some(true));
        assert!(tool_text(&missing).contains("尚未创建图纸"));
        let missing_copy = server
            .copy_region(Parameters(region(0, 0, 0, 0, None, None, Some(0), Some(0))))
            .await
            .unwrap();
        assert!(tool_text(&missing_copy).contains("尚未创建图纸"));

        create(&server).await;
        server
            .draw_pixels(Parameters(DrawPixelsRequest {
                pixels: "0 0 正红".into(),
                brush: Some(1),
            }))
            .await
            .unwrap();
        let before_codes = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .codes()
            .to_vec();
        let undo = server.canvas.lock().await.as_ref().unwrap().undo_len();
        let background = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .background_code();

        let address = "「dx,dy」与「tx,ty」只能给一组：相对位移填 dx/dy，绝对目标填 tx/ty。";
        for (dx, dy, tx, ty) in [
            (Some(1), Some(0), Some(1), Some(0)),
            (None, None, None, None),
            (Some(1), None, None, None),
            (None, Some(0), Some(1), None),
        ] {
            for copy in [false, true] {
                let result = if copy {
                    server
                        .copy_region(Parameters(region(0, 0, 0, 0, dx, dy, tx, ty)))
                        .await
                        .unwrap()
                } else {
                    server
                        .move_region(Parameters(region(0, 0, 0, 0, dx, dy, tx, ty)))
                        .await
                        .unwrap()
                };
                assert_eq!(result.is_error, Some(true));
                assert_eq!(tool_text(&result), address);
            }
        }
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes(),
            before_codes
        );
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().undo_len(),
            undo
        );
        assert_eq!(
            server
                .canvas
                .lock()
                .await
                .as_ref()
                .unwrap()
                .background_code(),
            background
        );

        let moved = server
            .move_region(Parameters(region(0, 0, 0, 0, Some(1), Some(0), None, None)))
            .await
            .unwrap();
        assert_ne!(moved.is_error, Some(true));
        assert_eq!(
            tool_text(&moved),
            "已将 (0,0)-(0,0) 的内容移动到 (1,0)-(1,0)。"
        );
        let copied = server
            .copy_region(Parameters(region(1, 0, 1, 0, None, None, Some(2), Some(0))))
            .await
            .unwrap();
        assert_eq!(
            tool_text(&copied),
            "已将 (1,0)-(1,0) 的内容复制到 (2,0)-(2,0)。"
        );
        let unchanged = server
            .move_region(Parameters(region(3, 3, 3, 3, Some(0), Some(0), None, None)))
            .await
            .unwrap();
        assert_ne!(unchanged.is_error, Some(true));
        assert_eq!(
            tool_text(&unchanged),
            "内容无变化：源区域与目标内容一致，未修改画布。"
        );
        assert!(unchanged.content[1].as_image().is_some());
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 3);
    }

    #[tokio::test]
    async fn draw_batch_move_region_preserves_order_and_rejects_as_one_step() {
        let dir = tempfile::tempdir().unwrap();
        let server = PixelDraw::new(dir.path().to_path_buf());
        create(&server).await;
        server
            .draw_pixels(Parameters(DrawPixelsRequest {
                pixels: "0 0 正红\n1 0 纯黑".into(),
                brush: Some(1),
            }))
            .await
            .unwrap();
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);

        let batched = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::MoveRegion {
                        x0: 0,
                        y0: 0,
                        x1: 1,
                        y1: 0,
                        dx: Some(2),
                        dy: Some(0),
                        tx: None,
                        ty: None,
                    },
                    DrawOp::Pixels {
                        pixels: "2 0 明黄".into(),
                        brush: Some(1),
                    },
                ],
            }))
            .await
            .unwrap();
        assert_ne!(batched.is_error, Some(true));
        let codes = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .codes()
            .to_vec();
        assert_eq!(codes[0], Some("H2"));
        assert_eq!(codes[1], Some("H2"));
        // pixels runs after the move, so it overwrites the transplanted 正红.
        assert_eq!(codes[2], Some("A4"));
        assert_eq!(codes[3], Some("H7"));
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 2);

        server
            .undo(Parameters(UndoRequest { steps: Some(1) }))
            .await
            .unwrap();
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes()[0],
            Some("F2")
        );
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes()[1],
            Some("H7")
        );
        assert_eq!(
            server.canvas.lock().await.as_ref().unwrap().codes()[2],
            Some("H2")
        );
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);

        let before = server
            .canvas
            .lock()
            .await
            .as_ref()
            .unwrap()
            .codes()
            .to_vec();
        let bad = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::MoveRegion {
                        x0: 0,
                        y0: 0,
                        x1: 1,
                        y1: 0,
                        dx: Some(2),
                        dy: Some(0),
                        tx: None,
                        ty: None,
                    },
                    DrawOp::Pixels {
                        pixels: "2 0 彩虹".into(),
                        brush: Some(1),
                    },
                ],
            }))
            .await
            .unwrap();
        assert_eq!(bad.is_error, Some(true));
        assert!(tool_text(&bad).contains("批量第 2 步失败"));
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().codes(), before);
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);

        let bad_move = server
            .draw_batch(Parameters(DrawBatchRequest {
                ops: vec![
                    DrawOp::Pixels {
                        pixels: "0 0 纯黑".into(),
                        brush: Some(1),
                    },
                    DrawOp::CopyRegion {
                        x0: 0,
                        y0: 0,
                        x1: 0,
                        y1: 0,
                        dx: None,
                        dy: None,
                        tx: Some(-1),
                        ty: Some(0),
                    },
                ],
            }))
            .await
            .unwrap();
        assert_eq!(bad_move.is_error, Some(true));
        assert!(tool_text(&bad_move).contains("目标区域越界"));
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().codes(), before);
        assert_eq!(server.canvas.lock().await.as_ref().unwrap().undo_len(), 1);
    }
}
