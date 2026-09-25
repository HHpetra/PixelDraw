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

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
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
            assert!(response.get("error").is_none(), "{response}");
            return response.get("result").expect("JSON-RPC result").clone();
        }
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
            "create_canvas",
            "draw_batch",
            "draw_circle",
            "draw_ellipse",
            "draw_line",
            "draw_pixels",
            "draw_rect",
            "draw_triangle",
            "flood_fill",
            "list_colors",
            "save_image"
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
    assert!(blank.pixels().all(|pixel| pixel.0 == [255, 255, 255]));

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
        [0x42, 0xCC, 0xFF],
        [0xFF, 0xE9, 0x53],
        [0x00, 0xBD, 0x35],
        [0xD8, 0x01, 0x27],
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
