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

use crate::canvas::{BrushSize, Canvas, PixelUpdate};
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
pub struct SaveImageRequest {
    /// 可选文件名，例如 `cat.png`，不填则按时间戳自动命名，重名时最多尝试 100 个数字后缀。不覆盖已有文件。仅允许 ASCII 字母、数字、点、下划线和连字符；禁止路径、空名、尾点及 Windows 设备名，补全 .png 后最多 200 字节。
    pub filename: Option<String>,
    /// pixel（默认）：8 倍像素画；pattern：带逐格色号及数量图例的 PNG；both：两者。
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
        description = "创建指定尺寸的像素图纸。background 可选 white（默认，H2 白豆）或 empty（透明空格，不计豆数）。宽和高必须在 1 到 128 之间。(0,0) 为左上角。palette 为色表模式：\"24\"、\"144\" 或 \"221\"，默认 \"221\"。色表在创建时锁定，绘制中途不可更改；要换色表请重新 create_canvas（会覆盖当前图纸）。返回放大 8 倍后的 PNG。"
    )]
    async fn create_canvas(
        &self,
        Parameters(CreateCanvasRequest {
            width,
            height,
            palette,
            background,
        }): Parameters<CreateCanvasRequest>,
    ) -> Result<CallToolResult, McpError> {
        let mode = match PaletteMode::parse(palette.as_deref()) {
            Ok(mode) => mode,
            Err(err) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(err)]));
            }
        };
        let canvas = match match background {
            Background::White => Canvas::new(width, height, mode),
            Background::Empty => Canvas::new_empty(width, height, mode),
        } {
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
                "已创建 {width}x{height} 像素图纸，背景为 {:?}，已锁定 {} 色模式。请用 list_colors 查看可用颜色，用 draw_pixels 按坐标填色。绘制中途不能切换色表。",
                background,
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
        description = "在当前像素图纸上按坐标填色，可多次调用以增量绘制。pixels 是一整段字符串，格式为「x y 颜色」三元组，可用空格或换行分隔，例如「0 0 正红\\n0 1 纯黑」。每次最多 1 MiB（1048576 字节）、65536 个落点，超限整批拒绝。可选 brush 为 1、2、4 或 8（默认 1）：在 (x,y) 涂满左上对齐的 N×N 方块，且 x、y 必须是 N 的倍数（笔刷 2 只能落在 0,2,4,6…），不对齐不吸附。坐标 (0,0) 为左上角。颜色必须使用创建时锁定色表中的中文名或 MARD 色号；用 - 清除为空格（不是 H2 白豆）；完整列表请调用 list_colors。格式错误、笔刷非法、未对齐、越界或颜色非法时整批拒绝。返回当前图纸放大 8 倍后的 PNG。"
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
        description = "导出当前快照：output 为 pixel（默认，8 倍 PNG）、pattern（带网格、逐格 MARD 色号、矩形色号与数量图例的 PNG）或 both（两者）。空格留白、不计数；H2 是白豆。both 使用 filename 保存像素画，并在扩展名前加 -pattern 保存图纸。每个文件原子保存且不覆盖；多文件并非事务，如遇中途磁盘故障会返回已保存路径。未创建图纸时不可调用。可选 filename（如 cat.png）仅允许 ASCII 字母、数字、点、下划线和连字符；禁止正反斜杠、空名、.、..、尾点及 Windows 设备名（含扩展名），补全 .png 后最多 200 字节。省略则按时间戳命名，重名时最多尝试 100 个数字后缀。保存失败返回工具错误，成功返回保存路径和预览图。"
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
                "PixelDraw：用强约束像素指令绘图，不要直接文生图。先 create_canvas(width, height, palette?) 选定 24、144 或 221 色（默认 221，创建后锁定），用 list_colors 查看颜色，再多次 draw_pixels({pixels:\"x y 颜色 ...\", brush?:1|2|4|8})（可用换行；笔刷落点必须对齐网格；每次最多 1 MiB、65536 个落点），完成后用 save_image 将快照原子保存到运行时配置的输出目录，不覆盖已有文件。文件名仅限 ASCII 字母、数字、点、下划线和连字符，禁止路径、空名、尾点及 Windows 设备名，补全 .png 后最多 200 字节；省略时按时间戳命名，重名最多尝试 100 个数字后缀。颜色只用当前模式的中文名或 MARD 色号。绘制返回 8 倍 PNG；save_image 的 output 可选 pixel（默认）、pattern 或 both。background 可选 white（默认）或 empty；draw_pixels 用 - 清空格，H2 仍是白色豆。图纸使用每格色号及矩形色号/数量图例；空格不标号、不计数。",
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
        assert!(
            serde_json::from_str::<CreateCanvasRequest>(
                r#"{"width":1,"height":1,"background":"unknown"}"#
            )
            .is_err()
        );
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
                background: Background::White,
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
