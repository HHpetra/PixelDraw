use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(10);

struct Process {
    child: Child,
    stdout: Receiver<io::Result<String>>,
}

impl Process {
    fn spawn(cwd: &Path, args: &[&std::ffi::OsStr]) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_pixeldraw"))
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn pixeldraw");
        let (sender, stdout) = mpsc::channel();
        // Install the guard before any further operation can panic.
        let mut process = Self { child, stdout };
        let pipe = process.child.stdout.take().expect("stdout pipe");
        thread::spawn(move || {
            for line in BufReader::new(pipe).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        process
    }

    fn send(&mut self, message: Value) {
        let stdin = self.child.stdin.as_mut().expect("stdin pipe");
        serde_json::to_writer(&mut *stdin, &message).expect("write JSON-RPC");
        stdin.write_all(b"\n").expect("write newline");
        stdin.flush().expect("flush JSON-RPC");
    }

    fn exchange(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let line = self
                .stdout
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("JSON-RPC response timed out or stdout closed")
                .expect("read stdout");
            let response: Value = serde_json::from_str(&line).expect("stdout must be JSON-RPC");
            assert_eq!(response["jsonrpc"], "2.0", "{response}");
            if response.get("id").is_none() && response["method"].is_string() {
                continue;
            }
            assert_eq!(response["id"], id, "{response}");
            return response;
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let response = self.exchange(id, method, params);
        assert!(response.get("error").is_none(), "{response}");
        response.get("result").expect("JSON-RPC result").clone()
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll child") {
                return status;
            }
            assert!(Instant::now() < deadline, "pixeldraw did not exit in time");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn response_png(result: &Value) -> Vec<u8> {
    assert_ne!(result["isError"], true, "{result}");
    let content = result["content"].as_array().expect("tool content");
    assert!(content.iter().any(|block| block["type"] == "text"));
    let images: Vec<_> = content
        .iter()
        .filter(|block| block["type"] == "image")
        .collect();
    assert_eq!(images.len(), 1, "{result}");
    assert_eq!(images[0]["mimeType"], "image/png");
    let png = STANDARD
        .decode(images[0]["data"].as_str().expect("base64 image"))
        .expect("valid base64");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let image =
        image::load_from_memory_with_format(&png, image::ImageFormat::Png).expect("valid PNG");
    assert_eq!((image.width(), image.height()), (64, 64));
    png
}

#[test]
fn stdio_draws_and_saves_outside_the_working_directory() {
    let output = tempfile::tempdir().expect("output directory");
    let cwd = tempfile::tempdir().expect("separate working directory");
    assert_ne!(output.path(), cwd.path());
    let mut process = Process::spawn(
        cwd.path(),
        &[
            std::ffi::OsStr::new("--output-dir"),
            output.path().as_os_str(),
        ],
    );

    let initialized = process.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "pixeldraw-integration-test", "version": "1.0.0"}
        }),
    );
    assert_eq!(initialized["protocolVersion"], "2024-11-05");
    assert_eq!(initialized["serverInfo"]["name"], "pixeldraw");
    assert!(initialized["capabilities"]["tools"].is_object());
    process.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let listed = process.request(2, "tools/list", json!({}));
    let mut names: Vec<_> = listed["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "copy_region",
            "create_canvas",
            "draw_batch",
            "draw_circle",
            "draw_curve",
            "draw_diamond",
            "draw_ellipse",
            "draw_line",
            "draw_pixels",
            "draw_rect",
            "draw_triangle",
            "fill_background",
            "flood_fill",
            "list_colors",
            "move_region",
            "save_image",
            "undo"
        ]
    );

    let created = process.request(
        3,
        "tools/call",
        json!({
            "name": "create_canvas", "arguments": {"width": 8, "height": 8, "palette": "24"}
        }),
    );
    let blank = image::load_from_memory(&response_png(&created))
        .unwrap()
        .to_rgb8();
    assert!(blank.pixels().all(|pixel| pixel.0 == [254, 255, 255]));

    let drawn = process.request(
        4,
        "tools/call",
        json!({
            "name": "draw_pixels",
            "arguments": {"pixels": "0 0 C4\n4 0 A4\n0 4 B5\n4 4 F5", "brush": 4}
        }),
    );
    let painted_png = response_png(&drawn);
    let painted = image::load_from_memory(&painted_png).unwrap().to_rgb8();
    let colors = [
        [0x41, 0xCC, 0xFF],
        [0xFB, 0xED, 0x56],
        [0x35, 0xE3, 0x52],
        [0xE7, 0x00, 0x2F],
    ];
    for (x, y, pixel) in painted.enumerate_pixels() {
        assert_eq!(
            pixel.0,
            colors[((y / 32) * 2 + x / 32) as usize],
            "at {x},{y}"
        );
    }

    let saved = process.request(
        5,
        "tools/call",
        json!({
            "name": "save_image", "arguments": {"filename": "first-icon.png"}
        }),
    );
    let saved_png = response_png(&saved);
    assert_eq!(saved_png, painted_png);
    let path = output.path().join("first-icon.png");
    assert_eq!(
        std::fs::read(&path).expect("saved PNG in configured output"),
        saved_png
    );
    assert!(saved["content"].as_array().unwrap().iter().any(|block| {
        block["text"]
            .as_str()
            .is_some_and(|text| text.contains(path.to_str().unwrap()))
    }));
    assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read_dir(cwd.path()).unwrap().count(), 0);
}

#[test]
fn cli_help_and_unknown_argument_exit_without_a_handshake() {
    let cwd = tempfile::tempdir().expect("working directory");
    let mut help = Process::spawn(cwd.path(), &[std::ffi::OsStr::new("--help")]);
    let first_line = help
        .stdout
        .recv_timeout(TIMEOUT)
        .expect("help stdout timed out or closed")
        .expect("read help");
    assert!(first_line.contains("PixelDraw"), "{first_line}");
    assert!(help.wait().success());

    let mut unknown = Process::spawn(cwd.path(), &[std::ffi::OsStr::new("--unknown-test-option")]);
    assert!(!unknown.wait().success());
}

#[test]
fn stdio_exports_both_with_empty_background_and_clear() {
    let output = tempfile::tempdir().unwrap();
    let mut process = Process::spawn(
        output.path(),
        &[
            std::ffi::OsStr::new("--output-dir"),
            output.path().as_os_str(),
        ],
    );
    process.request(1,"initialize",json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"export-test","version":"1"}}));
    process.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let schema = process.request(2, "tools/list", json!({}));
    let save = schema["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "save_image")
        .unwrap();
    let reference = save["inputSchema"]["properties"]["output"]["$ref"]
        .as_str()
        .unwrap();
    let output_schema = save["inputSchema"]
        .pointer(reference.trim_start_matches('#'))
        .unwrap();
    assert_eq!(output_schema["enum"], json!(["pixel", "pattern", "both"]));
    let created = process.request(
        3,
        "tools/call",
        json!({"name":"create_canvas","arguments":{"width":8,"height":8,"background":"empty"}}),
    );
    let blank = image::load_from_memory(&response_png(&created))
        .unwrap()
        .to_rgba8();
    assert!(blank.pixels().all(|p| p.0[3] == 0));
    process.request(
        4,
        "tools/call",
        json!({"name":"draw_pixels","arguments":{"pixels":"0 0 H2 1 0 H7 2 0 A1 2 0 -"}}),
    );
    let saved = process.request(
        5,
        "tools/call",
        json!({"name":"save_image","arguments":{"filename":"test","output":"both"}}),
    );
    assert_ne!(saved["isError"], true);
    let images: Vec<_> = saved["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["type"] == "image")
        .collect();
    assert_eq!(images.len(), 2);
    for (block, name) in images.iter().zip(["test.png", "test-pattern.png"]) {
        assert_eq!(
            std::fs::read(output.path().join(name)).unwrap(),
            STANDARD.decode(block["data"].as_str().unwrap()).unwrap()
        );
    }
    let pixels = image::open(output.path().join("test.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(pixels.get_pixel(0, 0).0, [254, 255, 255, 255]);
    assert_eq!(pixels.get_pixel(8, 0).0, [0, 0, 0, 255]);
    assert_eq!(pixels.get_pixel(16, 0).0[3], 0);
    let chart = image::open(output.path().join("test-pattern.png")).unwrap();
    assert!(chart.width() > pixels.width());
}

#[test]
fn stdio_shapes_and_fill_export_the_same_state_as_pixels() {
    let output = tempfile::tempdir().unwrap();
    let mut process = Process::spawn(
        output.path(),
        &[
            std::ffi::OsStr::new("--output-dir"),
            output.path().as_os_str(),
        ],
    );
    process.request(1, "initialize", json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"drawing-state-test","version":"1"}}));
    process.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let mut id = 1;
    let mut call = |name: &str, arguments: Value| {
        id += 1;
        let result = process.request(id, "tools/call", json!({"name":name,"arguments":arguments}));
        assert_ne!(result["isError"], true, "{result}");
        result
    };
    for background in ["white", "empty"] {
        for batch in [false, true] {
            call(
                "create_canvas",
                json!({"width":8,"height":8,"background":background}),
            );
            let ops = vec![
                json!({"type":"line","x0":0,"y0":0,"x1":7,"y1":0,"color":"f5"}),
                json!({"type":"rect","x0":0,"y0":1,"x1":3,"y1":3,"color":"H7","fill":true}),
                json!({"type":"flood_fill","x":7,"y":7,"color":"A4"}),
                json!({"type":"pixels","pixels":"0 0 -"}),
            ];
            let preview = if batch {
                call("draw_batch", json!({"ops":ops}))
            } else {
                let mut result = Value::Null;
                for (mut op, name) in
                    ops.into_iter()
                        .zip(["draw_line", "draw_rect", "flood_fill", "draw_pixels"])
                {
                    op.as_object_mut().unwrap().remove("type");
                    result = call(name, op);
                }
                result
            };
            let name = format!("{background}-{batch}");
            call("save_image", json!({"filename":name,"output":"both"}));
            call(
                "create_canvas",
                json!({"width":8,"height":8,"background":background}),
            );
            let pixels = (0..8)
                .flat_map(|y| {
                    (0..8).map(move |x| {
                        let color = if x == 0 && y == 0 {
                            "-"
                        } else if y == 0 {
                            "F5"
                        } else if x < 4 && y <= 3 {
                            "H7"
                        } else {
                            "A4"
                        };
                        format!("{x} {y} {color}")
                    })
                })
                .collect::<Vec<_>>()
                .join(" ");
            let expected = call("draw_pixels", json!({"pixels":pixels}));
            assert_eq!(response_png(&preview), response_png(&expected));
            let control = format!("{name}-control");
            call("save_image", json!({"filename":control,"output":"both"}));
            for suffix in [".png", "-pattern.png"] {
                assert_eq!(
                    std::fs::read(output.path().join(format!("{name}{suffix}"))).unwrap(),
                    std::fs::read(output.path().join(format!("{control}{suffix}"))).unwrap(),
                    "{background}, batch={batch}, {suffix}"
                );
            }
        }
    }
}

#[test]
fn stdio_fill_background_overwrites_existing_pixels() {
    let output = tempfile::tempdir().unwrap();
    let mut process = Process::spawn(
        output.path(),
        &[
            std::ffi::OsStr::new("--output-dir"),
            output.path().as_os_str(),
        ],
    );
    process.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "fill-background-test", "version": "1"}
        }),
    );
    process.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let schema = process.request(2, "tools/list", json!({}));
    let tool = schema["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "fill_background")
        .expect("fill_background tool");
    assert!(tool["description"].as_str().unwrap().contains("每一个格子"));
    let required = tool["inputSchema"]["required"]
        .as_array()
        .expect("fill_background parameters");
    assert!(required.iter().any(|name| name == "color"));

    process.request(
        3,
        "tools/call",
        json!({"name":"create_canvas","arguments":{"width":8,"height":8,"palette":"24"}}),
    );
    let same = process.request(
        4,
        "tools/call",
        json!({"name":"fill_background","arguments":{"color":"H2"}}),
    );
    assert_ne!(same["isError"], true, "{same}");
    assert!(
        same["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("改写 0 个")
    );
    let no_undo = process.request(5, "tools/call", json!({"name":"undo","arguments":{}}));
    assert_eq!(no_undo["isError"], true, "{no_undo}");

    process.request(
        6,
        "tools/call",
        json!({"name":"draw_pixels","arguments":{"pixels":"0 0 H7 7 7 A4 3 3 F2"}}),
    );
    let rejected = process.request(
        7,
        "tools/call",
        json!({"name":"fill_background","arguments":{"color":"彩虹"}}),
    );
    assert_eq!(rejected["isError"], true, "{rejected}");

    let filled = process.request(
        8,
        "tools/call",
        json!({"name":"fill_background","arguments":{"color":"中国红"}}),
    );
    let image = image::load_from_memory(&response_png(&filled))
        .unwrap()
        .to_rgb8();
    assert!(image.pixels().all(|pixel| pixel.0 == [0xE7, 0x00, 0x2F]));

    let undone = process.request(
        9,
        "tools/call",
        json!({"name":"undo","arguments":{"steps":1}}),
    );
    let restored = image::load_from_memory(&response_png(&undone))
        .unwrap()
        .to_rgb8();
    assert_eq!(restored.get_pixel(0, 0).0, [0, 0, 0]);
    assert_eq!(restored.get_pixel(56, 56).0, [0xFB, 0xED, 0x56]);
    assert_eq!(restored.get_pixel(24, 24).0, [0xFC, 0x3D, 0x46]);
    assert_eq!(restored.get_pixel(8, 0).0, [254, 255, 255]);

    process.request(
        10,
        "tools/call",
        json!({"name":"create_canvas","arguments":{"width":8,"height":8,"background":"empty","palette":"24"}}),
    );
    let black = process.request(
        11,
        "tools/call",
        json!({"name":"fill_background","arguments":{"color":"纯黑"}}),
    );
    let black_img = image::load_from_memory(&response_png(&black))
        .unwrap()
        .to_rgba8();
    assert!(black_img.pixels().all(|pixel| pixel.0 == [0, 0, 0, 255]));
    let cleared = process.request(12, "tools/call", json!({"name":"undo","arguments":{}}));
    let cleared_img = image::load_from_memory(&response_png(&cleared))
        .unwrap()
        .to_rgba8();
    assert!(cleared_img.pixels().all(|pixel| pixel.0[3] == 0));
}

#[test]
fn stdio_create_canvas_background_color_updates_when_refilled() {
    let output = tempfile::tempdir().unwrap();
    let mut process = Process::spawn(
        output.path(),
        &[
            std::ffi::OsStr::new("--output-dir"),
            output.path().as_os_str(),
        ],
    );
    process.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "background-color-test", "version": "1"}
        }),
    );
    process.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let schema = process.request(2, "tools/list", json!({}));
    let tool = schema["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "create_canvas")
        .expect("create_canvas tool");
    let description = tool["description"].as_str().unwrap();
    assert!(description.contains("background_color"));
    assert!(description.contains("fill_background"));
    let properties = &tool["inputSchema"]["properties"];
    assert!(properties["background_color"].is_object(), "{properties}");
    let required = tool["inputSchema"]["required"].as_array();
    assert!(
        required.is_none_or(|names| !names.iter().any(|name| name == "background_color")),
        "background_color must stay optional: {required:?}"
    );

    let created = process.request(
        3,
        "tools/call",
        json!({
            "name": "create_canvas",
            "arguments": {"width": 8, "height": 8, "palette": "24", "background_color": "正红"}
        }),
    );
    assert_ne!(created["isError"], true, "{created}");
    assert!(
        created["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("背景为 正红（F2）")
    );
    let red = image::load_from_memory(&response_png(&created))
        .unwrap()
        .to_rgb8();
    assert!(red.pixels().all(|pixel| pixel.0 == [0xFC, 0x3D, 0x46]));

    let omitted = process.request(
        4,
        "tools/call",
        json!({"name":"create_canvas","arguments":{"width":8,"height":8,"palette":"24"}}),
    );
    assert!(
        omitted["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("背景为 雪白（H2）")
    );
    let white = image::load_from_memory(&response_png(&omitted))
        .unwrap()
        .to_rgb8();
    assert!(white.pixels().all(|pixel| pixel.0 == [254, 255, 255]));

    let rejected = process.request(
        5,
        "tools/call",
        json!({
            "name": "create_canvas",
            "arguments": {
                "width": 8,
                "height": 8,
                "palette": "24",
                "background": "empty",
                "background_color": "纯黑"
            }
        }),
    );
    assert_eq!(rejected["isError"], true, "{rejected}");
    let still_white = process.request(
        6,
        "tools/call",
        json!({"name":"fill_background","arguments":{"color":"H2"}}),
    );
    assert!(
        still_white["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("改写 0 个")
    );
    assert!(
        still_white["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("当前背景色为 雪白（H2）")
    );
    let no_undo = process.request(7, "tools/call", json!({"name":"undo","arguments":{}}));
    assert_eq!(no_undo["isError"], true, "{no_undo}");

    process.request(
        8,
        "tools/call",
        json!({
            "name": "create_canvas",
            "arguments": {"width": 8, "height": 8, "palette": "24", "background_color": "F2"}
        }),
    );
    process.request(
        9,
        "tools/call",
        json!({"name":"draw_pixels","arguments":{"pixels":"0 0 H7"}}),
    );
    let filled = process.request(
        10,
        "tools/call",
        json!({"name":"fill_background","arguments":{"color":"中国红"}}),
    );
    assert!(
        filled["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("当前背景色为 中国红（F5）")
    );
    let china = image::load_from_memory(&response_png(&filled))
        .unwrap()
        .to_rgb8();
    assert!(china.pixels().all(|pixel| pixel.0 == [0xE7, 0x00, 0x2F]));

    let undone = process.request(11, "tools/call", json!({"name":"undo","arguments":{}}));
    let restored = image::load_from_memory(&response_png(&undone))
        .unwrap()
        .to_rgb8();
    assert_eq!(restored.get_pixel(0, 0).0, [0, 0, 0]);
    assert_eq!(restored.get_pixel(8, 0).0, [0xFC, 0x3D, 0x46]);
    let remembered = process.request(
        12,
        "tools/call",
        json!({"name":"fill_background","arguments":{"color":"正红"}}),
    );
    let remembered_text = remembered["content"][0]["text"].as_str().unwrap();
    assert!(remembered_text.contains("改写 1 个"), "{remembered_text}");
    assert!(
        remembered_text.contains("当前背景色为 正红（F2）"),
        "{remembered_text}"
    );
}

#[test]
fn stdio_move_region_changes_preview_and_pattern_codes() {
    let output = tempfile::tempdir().unwrap();
    let mut process = Process::spawn(
        output.path(),
        &[
            std::ffi::OsStr::new("--output-dir"),
            output.path().as_os_str(),
        ],
    );
    process.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "move-region-test", "version": "1"}
        }),
    );
    process.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let listed = process.request(2, "tools/list", json!({}));
    let tools = listed["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 17);
    let move_tool = tools
        .iter()
        .find(|tool| tool["name"] == "move_region")
        .expect("move_region tool");
    let required = move_tool["inputSchema"]["required"].as_array().unwrap();
    for name in ["x0", "y0", "x1", "y1"] {
        assert!(required.iter().any(|item| item == name), "{required:?}");
    }
    assert!(required.iter().all(|item| item != "dx" && item != "tx"));

    let created = process.request(
        3,
        "tools/call",
        json!({"name":"create_canvas","arguments":{"width":8,"height":8,"palette":"24"}}),
    );
    let blank = response_png(&created);
    let drawn = process.request(
        4,
        "tools/call",
        json!({
            "name": "draw_pixels",
            "arguments": {"pixels": "0 0 F5 1 0 H7 0 1 A4 1 1 - 3 0 C4"}
        }),
    );
    let before = response_png(&drawn);
    assert_ne!(before, blank);

    let moved = process.request(
        5,
        "tools/call",
        json!({
            "name": "move_region",
            "arguments": {"x0": 1, "y0": 1, "x1": 0, "y1": 0, "dx": 3, "dy": 1}
        }),
    );
    assert_ne!(moved["isError"], true, "{moved}");
    let text = moved["content"][0]["text"].as_str().unwrap();
    assert_eq!(text, "已将 (0,0)-(1,1) 的内容移动到 (3,1)-(4,2)。");
    let after = response_png(&moved);
    assert_ne!(after, before);
    let preview = image::load_from_memory(&after).unwrap().to_rgba8();
    assert_eq!(preview.get_pixel(0, 0).0, [254, 255, 255, 255]);
    assert_eq!(preview.get_pixel(8, 8).0, [254, 255, 255, 255]);
    assert_eq!(preview.get_pixel(24, 0).0, [0x41, 0xCC, 0xFF, 255]);
    assert_eq!(preview.get_pixel(24, 8).0, [0xE7, 0x00, 0x2F, 255]);
    assert_eq!(preview.get_pixel(32, 8).0, [0, 0, 0, 255]);
    assert_eq!(preview.get_pixel(24, 16).0, [0xFB, 0xED, 0x56, 255]);
    assert_eq!(preview.get_pixel(32, 16).0[3], 0);

    process.request(
        6,
        "tools/call",
        json!({"name":"save_image","arguments":{"filename":"moved","output":"pattern"}}),
    );
    process.request(
        7,
        "tools/call",
        json!({"name":"create_canvas","arguments":{"width":8,"height":8,"palette":"24"}}),
    );
    process.request(
        8,
        "tools/call",
        json!({
            "name": "draw_pixels",
            "arguments": {"pixels": "3 0 C4 3 1 F5 4 1 H7 3 2 A4 4 2 -"}
        }),
    );
    process.request(
        9,
        "tools/call",
        json!({"name":"save_image","arguments":{"filename":"moved-control","output":"pattern"}}),
    );
    assert_eq!(
        std::fs::read(output.path().join("moved.png")).unwrap(),
        std::fs::read(output.path().join("moved-control.png")).unwrap()
    );
}

fn tool_failed(response: &Value) -> bool {
    response.get("error").is_some() || response["result"]["isError"] == true
}

fn cell(image: &image::RgbImage, x: u32, y: u32) -> [u8; 3] {
    image.get_pixel(x * 8, y * 8).0
}

#[test]
fn stdio_circle_and_ellipse_use_diameter() {
    let output = tempfile::tempdir().unwrap();
    let mut process = Process::spawn(
        output.path(),
        &[
            std::ffi::OsStr::new("--output-dir"),
            output.path().as_os_str(),
        ],
    );
    process.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "circle-diameter-test", "version": "1"}
        }),
    );
    process.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let listed = process.request(2, "tools/list", json!({}));
    let tools = listed["tools"].as_array().expect("tools");
    let circle = tools
        .iter()
        .find(|tool| tool["name"] == "draw_circle")
        .expect("draw_circle");
    let ellipse = tools
        .iter()
        .find(|tool| tool["name"] == "draw_ellipse")
        .expect("draw_ellipse");
    let circle_desc = circle["description"].as_str().unwrap();
    assert!(circle_desc.contains("直径"));
    assert!(circle_desc.contains("diameter"));
    assert!(ellipse["description"].as_str().unwrap().contains("width"));
    let circle_props = &circle["inputSchema"]["properties"];
    for name in ["x", "y", "diameter", "color", "fill"] {
        assert!(circle_props[name].is_object(), "{circle_props}");
    }
    for retired in ["cx", "cy", "radius"] {
        assert!(circle_props.get(retired).is_none(), "{circle_props}");
    }
    let ellipse_props = &ellipse["inputSchema"]["properties"];
    for name in ["x", "y", "width", "height"] {
        assert!(ellipse_props[name].is_object(), "{ellipse_props}");
    }
    for retired in ["cx", "cy", "rx", "ry"] {
        assert!(ellipse_props.get(retired).is_none(), "{ellipse_props}");
    }

    process.request(
        3,
        "tools/call",
        json!({"name":"create_canvas","arguments":{"width":8,"height":8,"palette":"24"}}),
    );
    let white = [254, 255, 255];
    let black = [0, 0, 0];

    let dot = process.request(
        4,
        "tools/call",
        json!({"name":"draw_circle","arguments":{"x":0,"y":0,"diameter":1,"color":"H7","fill":true}}),
    );
    let dot_img = image::load_from_memory(&response_png(&dot))
        .unwrap()
        .to_rgb8();
    assert_eq!(cell(&dot_img, 0, 0), black);
    assert_eq!(cell(&dot_img, 1, 0), white);
    assert_eq!(cell(&dot_img, 0, 1), white);

    let undone = process.request(5, "tools/call", json!({"name":"undo","arguments":{}}));
    let blank = image::load_from_memory(&response_png(&undone))
        .unwrap()
        .to_rgb8();
    assert!(blank.pixels().all(|pixel| pixel.0 == white));

    let four = process.request(
        6,
        "tools/call",
        json!({"name":"draw_circle","arguments":{"x":1,"y":1,"diameter":2,"color":"H7","fill":true}}),
    );
    let four_img = image::load_from_memory(&response_png(&four))
        .unwrap()
        .to_rgb8();
    for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
        assert_eq!(cell(&four_img, x, y), black, "{x},{y}");
    }
    assert_eq!(cell(&four_img, 0, 0), white);
    assert_eq!(cell(&four_img, 3, 1), white);
    process.request(7, "tools/call", json!({"name":"undo","arguments":{}}));

    // Diameter 4 anchored at (2, 2): rows of width 2, 4, 4, 2.
    let disk = process.request(
        8,
        "tools/call",
        json!({"name":"draw_circle","arguments":{"x":2,"y":2,"diameter":4,"color":"H7","fill":true}}),
    );
    let disk_png = response_png(&disk);
    let disk_img = image::load_from_memory(&disk_png).unwrap().to_rgb8();
    let painted: [(u32, u32); 12] = [
        (3, 2),
        (4, 2),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (2, 4),
        (3, 4),
        (4, 4),
        (5, 4),
        (3, 5),
        (4, 5),
    ];
    for x in 0..8u32 {
        for y in 0..8u32 {
            let expect = if painted.contains(&(x, y)) {
                black
            } else {
                white
            };
            assert_eq!(cell(&disk_img, x, y), expect, "{x},{y}");
        }
    }

    let saved = process.request(
        9,
        "tools/call",
        json!({"name":"save_image","arguments":{"filename":"circle","output":"pixel"}}),
    );
    assert_eq!(response_png(&saved), disk_png);

    let mut id = 10u64;
    for arguments in [
        json!({"x":0,"y":0,"diameter":4,"color":"H7","fill":true,"cx":1,"radius":2}),
        json!({"cx":1,"cy":1,"radius":2,"color":"H7","fill":true}),
        json!({"x":0,"y":0,"color":"H7","fill":true}),
        json!({"x":0,"y":0,"diameter":-1,"color":"H7","fill":true}),
        json!({"x":0,"y":0,"diameter":0,"color":"H7","fill":true}),
        json!({"x":8,"y":0,"diameter":4,"color":"H7","fill":true}),
        json!({"x":0,"y":-1,"diameter":4,"color":"H7","fill":true}),
    ] {
        let response = process.exchange(
            id,
            "tools/call",
            json!({"name":"draw_circle","arguments":arguments}),
        );
        assert!(tool_failed(&response), "{arguments} -> {response}");
        id += 1;
    }
    for arguments in [
        json!({"x":0,"y":0,"width":4,"height":3,"color":"H7","fill":true,"rx":1}),
        json!({"cx":0,"cy":0,"rx":2,"ry":1,"color":"H7","fill":true}),
        json!({"x":0,"y":0,"width":0,"height":4,"color":"H7","fill":true}),
    ] {
        let response = process.exchange(
            id,
            "tools/call",
            json!({"name":"draw_ellipse","arguments":arguments}),
        );
        assert!(tool_failed(&response), "{arguments} -> {response}");
        id += 1;
    }
    let bad_batch = process.exchange(
        id,
        "tools/call",
        json!({
            "name": "draw_batch",
            "arguments": {"ops": [
                {"type":"line","x0":0,"y0":0,"x1":3,"y1":0,"color":"A4"},
                {"type":"circle","x":0,"y":0,"diameter":4,"color":"H7","fill":true,"radius":1}
            ]}
        }),
    );
    id += 1;
    assert!(tool_failed(&bad_batch), "{bad_batch}");
    let zero_batch = process.exchange(
        id,
        "tools/call",
        json!({
            "name": "draw_batch",
            "arguments": {"ops": [
                {"type":"circle","x":0,"y":0,"diameter":0,"color":"H7","fill":true},
                {"type":"line","x0":0,"y0":0,"x1":3,"y1":0,"color":"A4"}
            ]}
        }),
    );
    assert!(tool_failed(&zero_batch), "{zero_batch}");

    let still = process.request(
        id + 1,
        "tools/call",
        json!({"name":"save_image","arguments":{"filename":"circle-still","output":"pixel"}}),
    );
    assert_eq!(response_png(&still), disk_png);

    let clipped = process.request(
        id + 2,
        "tools/call",
        json!({"name":"draw_circle","arguments":{"x":6,"y":0,"diameter":5,"color":"H7","fill":true}}),
    );
    let clipped_img = image::load_from_memory(&response_png(&clipped))
        .unwrap()
        .to_rgb8();
    let overflow = [
        (7, 0),
        (6, 1),
        (7, 1),
        (6, 2),
        (7, 2),
        (6, 3),
        (7, 3),
        (7, 4),
    ];
    for (x, y) in overflow {
        assert_eq!(cell(&clipped_img, x, y), black, "clipped {x},{y}");
    }
    assert_eq!(cell(&clipped_img, 6, 0), white);
    assert_eq!(cell(&clipped_img, 5, 2), white);

    process.request(id + 3, "tools/call", json!({"name":"undo","arguments":{}}));
    process.request(id + 4, "tools/call", json!({"name":"undo","arguments":{}}));
    let square = process.request(
        id + 5,
        "tools/call",
        json!({"name":"draw_ellipse","arguments":{"x":2,"y":2,"width":4,"height":4,"color":"H7","fill":true}}),
    );
    assert_eq!(response_png(&square), disk_png);

    process.request(id + 6, "tools/call", json!({"name":"undo","arguments":{}}));
    let ring = process.request(
        id + 7,
        "tools/call",
        json!({"name":"draw_circle","arguments":{"x":0,"y":0,"diameter":8,"color":"H7","fill":false,"brush":2}}),
    );
    let ring_img = image::load_from_memory(&response_png(&ring))
        .unwrap()
        .to_rgb8();
    assert_eq!(cell(&ring_img, 0, 3), black);
    for (x, y) in [(3, 3), (4, 3), (3, 4), (4, 4)] {
        assert_eq!(cell(&ring_img, x, y), white, "brush 2 filled {x},{y}");
    }
    let thin = process.request(id + 8, "tools/call", json!({"name":"undo","arguments":{}}));
    assert_ne!(response_png(&thin), response_png(&ring));
    let thin_ring = process.request(
        id + 9,
        "tools/call",
        json!({"name":"draw_circle","arguments":{"x":0,"y":0,"diameter":8,"color":"H7","fill":false}}),
    );
    let thin_img = image::load_from_memory(&response_png(&thin_ring))
        .unwrap()
        .to_rgb8();
    let thick_count = ring_img.pixels().filter(|pixel| pixel.0 == black).count();
    let thin_count = thin_img.pixels().filter(|pixel| pixel.0 == black).count();
    assert!(thick_count > thin_count);
    assert_eq!(cell(&thin_img, 3, 3), white);
}

#[test]
fn stdio_diamond_uses_center_and_half_axes() {
    let output = tempfile::tempdir().unwrap();
    let mut process = Process::spawn(
        output.path(),
        &[
            std::ffi::OsStr::new("--output-dir"),
            output.path().as_os_str(),
        ],
    );
    process.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "diamond-test", "version": "1"}
        }),
    );
    process.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let listed = process.request(2, "tools/list", json!({}));
    let tools = listed["tools"].as_array().expect("tools");
    let names: Vec<_> = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect();
    assert!(names.contains(&"draw_diamond"), "{names:?}");
    let diamond = tools
        .iter()
        .find(|tool| tool["name"] == "draw_diamond")
        .expect("draw_diamond");
    let desc = diamond["description"].as_str().unwrap();
    assert!(desc.contains("中心格"), "{desc}");
    assert!(desc.contains("菱形半轴"), "{desc}");
    assert!(desc.contains("2*rx+1"), "{desc}");
    assert!(desc.contains("2*ry+1"), "{desc}");
    assert!(desc.contains("奇数"), "{desc}");
    assert!(desc.contains("1 格轮廓"), "{desc}");
    assert!(desc.contains("不是左上角"), "{desc}");
    assert!(desc.contains("已废止的椭圆"), "{desc}");
    assert!(desc.contains("必须在图纸内"), "{desc}");
    let props = &diamond["inputSchema"]["properties"];
    for name in ["cx", "cy", "rx", "ry", "color", "fill", "brush"] {
        assert!(props[name].is_object(), "{name} missing from {props}");
    }
    for retired in ["x", "y", "width", "height", "diameter", "radius"] {
        assert!(props.get(retired).is_none(), "{retired} in {props}");
    }
    let ellipse = tools
        .iter()
        .find(|tool| tool["name"] == "draw_ellipse")
        .expect("draw_ellipse");
    let ellipse_props = &ellipse["inputSchema"]["properties"];
    for retired in ["cx", "cy", "rx", "ry"] {
        assert!(
            ellipse_props.get(retired).is_none(),
            "ellipse still has {retired}"
        );
    }

    process.request(
        3,
        "tools/call",
        json!({"name":"create_canvas","arguments":{"width":8,"height":8,"palette":"24"}}),
    );
    let created = process.request(
        4,
        "tools/call",
        json!({"name":"save_image","arguments":{"filename":"diamond-blank","output":"pixel"}}),
    );
    let blank_png = response_png(&created);

    let drawn = process.request(
        5,
        "tools/call",
        json!({"name":"draw_diamond","arguments":{"cx":3,"cy":3,"rx":2,"ry":2,"color":"H7","fill":true}}),
    );
    let img = image::load_from_memory(&response_png(&drawn))
        .unwrap()
        .to_rgb8();
    let white = [254, 255, 255];
    let black = [0, 0, 0];
    for x in 0..8u32 {
        for y in 0..8u32 {
            let inside = (x as i32 - 3).abs() + (y as i32 - 3).abs() <= 2;
            let expect = if inside { black } else { white };
            assert_eq!(cell(&img, x, y), expect, "{x},{y}");
        }
    }
    assert_eq!(cell(&img, 3, 3), black);
    assert_eq!(cell(&img, 5, 5), white);
    assert_eq!(cell(&img, 1, 1), white);

    process.request(6, "tools/call", json!({"name":"undo","arguments":{}}));
    let bar = process.request(
        7,
        "tools/call",
        json!({"name":"draw_diamond","arguments":{"cx":1,"cy":3,"rx":0,"ry":2,"color":"F5","fill":true}}),
    );
    let bar_img = image::load_from_memory(&response_png(&bar))
        .unwrap()
        .to_rgb8();
    let red = [0xE7, 0x00, 0x2F];
    for y in 1..=5u32 {
        assert_eq!(cell(&bar_img, 1, y), red, "vertical bar y={y}");
    }
    assert_eq!(cell(&bar_img, 0, 3), white);
    assert_eq!(cell(&bar_img, 2, 3), white);

    process.request(8, "tools/call", json!({"name":"undo","arguments":{}}));
    let cleared = process.request(
        9,
        "tools/call",
        json!({"name":"save_image","arguments":{"filename":"diamond-cleared","output":"pixel"}}),
    );
    assert_eq!(response_png(&cleared), blank_png);

    let batch = process.request(
        10,
        "tools/call",
        json!({
            "name": "draw_batch",
            "arguments": {"ops": [
                {"type":"diamond","cx":4,"cy":2,"rx":1,"ry":0,"color":"H7","fill":false},
                {"type":"line","x0":0,"y0":7,"x1":2,"y1":7,"color":"A4"}
            ]}
        }),
    );
    assert!(!tool_failed(&batch), "{batch}");
    let batch_png = response_png(&batch);
    let batch_img = image::load_from_memory(&batch_png).unwrap().to_rgb8();
    // ry = 0 is a horizontal bar of length 2*rx+1 centered on (4, 2).
    for x in 3..=5u32 {
        assert_eq!(cell(&batch_img, x, 2), black, "horizontal bar x={x}");
    }
    assert_eq!(cell(&batch_img, 4, 1), white);
    assert_eq!(cell(&batch_img, 4, 3), white);
    let yellow = [0xFB, 0xED, 0x56];
    assert_eq!(cell(&batch_img, 0, 7), yellow);

    let bad_batch = process.exchange(
        11,
        "tools/call",
        json!({
            "name": "draw_batch",
            "arguments": {"ops": [
                {"type":"line","x0":0,"y0":0,"x1":3,"y1":0,"color":"A4"},
                {"type":"diamond","cx":8,"cy":0,"rx":2,"ry":2,"color":"H7","fill":true}
            ]}
        }),
    );
    assert!(tool_failed(&bad_batch), "{bad_batch}");
    let unknown_field = process.exchange(
        12,
        "tools/call",
        json!({
            "name": "draw_batch",
            "arguments": {"ops": [
                {"type":"diamond","cx":1,"cy":1,"rx":2,"ry":2,"color":"H7","fill":true,"width":5,"height":5}
            ]}
        }),
    );
    assert!(tool_failed(&unknown_field), "{unknown_field}");
    let direct = process.exchange(
        13,
        "tools/call",
        json!({"name":"draw_diamond","arguments":{"cx":3,"cy":3,"rx":2,"ry":1,"color":"H7","fill":true,"x":0,"diameter":4}}),
    );
    assert!(tool_failed(&direct), "{direct}");

    let still = process.request(
        14,
        "tools/call",
        json!({"name":"save_image","arguments":{"filename":"diamond-still","output":"pixel"}}),
    );
    assert_eq!(response_png(&still), batch_png);
}
