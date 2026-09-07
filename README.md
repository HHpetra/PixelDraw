# PixelDraw

[English](README.md) | [简体中文](README.zh-CN.md)

A local MCP server that lets AI draw pixel art through explicit coordinates
and constrained MARD palettes, rather than text-to-image generation.

Rust executes the drawing commands; your MCP client supplies the model.
PixelDraw itself needs no API key and exposes no network listener.

## What it does

PixelDraw gives an AI assistant a small drawing workspace. You describe what
to draw in your MCP client; the assistant creates a canvas, queries the
available colors, places pixels or square brush stamps, inspects the returned
preview, and continues drawing before saving the result.

- **Precise pixel placement:** draw on a 1..128 pixel-wide and -high canvas
  with explicit coordinates, rather than asking an image model to imitate pixels.
- **Constrained colors:** choose a locked 24-, 144-, or 221-color MARD palette
  and address colors by code or the palette's Chinese names.
- **Incremental drawing:** combine aligned 1x1, 2x2, 4x4 and 8x8 brushes across
  multiple calls. An invalid batch is rejected without partially painting it.
- **Immediate previews:** creation and drawing return an 8x nearest-neighbor
  PNG so an image-capable client can display the current canvas.
- **Local export:** save PNGs to a configurable directory without overwriting
  existing work. The model and its API billing remain the client's responsibility.

Use it for small icons, pixel-art illustrations, and experiments with tool-driven
AI drawing. It is not a standalone AI model, a photo converter, or a full image editor.

## Gallery

Selected existing artwork from this project's `output/` directory, copied to
`docs/images/` for GitHub display. The source canvases below are enlarged 8x
in the exported PNGs; these examples do not guarantee identical model output.

| Croissant | Seaside sunset | Temple of Heaven |
| --- | --- | --- |
| ![Pixel-art croissant](docs/images/croissant-32x32.png) | ![Pixel-art seaside sunset](docs/images/seaside-sunset-32x32.png) | ![Pixel-art Temple of Heaven](docs/images/temple-of-heaven-64x64.png) |
| 32x32 canvas | 32x32 canvas | 64x64 canvas |

## Install

### Download a binary (no Rust required)

Open [Releases](https://github.com/HHpetra/PixelDraw/releases/latest), expand
**Assets**, and download the archive for your system. Do not choose GitHub's
automatically generated **Source code** archives.

| System | Archive suffix |
| --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc.zip` |
| Linux x64 (glibc 2.35+, e.g. Ubuntu 22.04+) | `x86_64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon (ARM64) | `aarch64-apple-darwin.tar.gz` |
| macOS Intel (x64) | `x86_64-apple-darwin.tar.gz` |

Extract the archive to a permanent folder. It contains `pixeldraw.exe` on
Windows or `pixeldraw` on Linux/macOS, plus documentation. No Rust, Cargo,
Git or separate model runtime is needed for the server.

Windows PowerShell (example after extraction):

```powershell
& "C:/Tools/PixelDraw/pixeldraw.exe" --version
& "C:/Tools/PixelDraw/pixeldraw.exe" --help
```

Linux/macOS (substitute the actual extracted directory):

```sh
tar -xzf pixeldraw-v0.1.1-aarch64-apple-darwin.tar.gz
./pixeldraw-v0.1.1-aarch64-apple-darwin/pixeldraw --version
```

Use the executable's **absolute path** in your client's MCP configuration:

```json
{
  "mcp": {
    "pixeldraw": {
      "type": "local",
      "command": ["C:/Tools/PixelDraw/pixeldraw.exe", "--output-dir", "C:/Pictures/PixelDraw"],
      "enabled": true
    }
  }
}
```

On Linux/macOS replace the executable with a path such as
`/home/you/tools/pixeldraw` or `/Users/you/tools/pixeldraw`, and choose your own
output directory. This OpenCode example does not invoke Cargo. Restart or
reload the MCP client after configuration. The client starts the server;
double-clicking the executable does not open a drawing window.

Each archive has a `.sha256` file. Compare it with `Get-FileHash <archive>
-Algorithm SHA256` in PowerShell, `sha256sum -c <archive>.sha256` on Linux,
or `shasum -a 256 -c <archive>.sha256` on macOS. Keep archive and checksum in
the same directory. Checksums verify integrity, not publisher identity.
The binaries are not code-signed or notarized; macOS or Windows may show a
security warning. Verify the download source before using the OS's per-file
approval flow; do not disable system-wide security protections.

### Build from source (developers)

Install Rust 1.96.1 or newer using [rustup](https://rustup.rs/), then:

```sh
git clone https://github.com/HHpetra/PixelDraw.git
cd PixelDraw
cargo install --path . --locked
pixeldraw --help
```

The executable is installed into Cargo's bin directory, which must be on PATH.
Prebuilt packages are available starting with v0.1.1. CI checks Windows,
Linux and macOS; inspect the Actions results for platform status.

## Connect an MCP client

Configure a stdio server with command `pixeldraw` and arguments
`["--output-dir", "/absolute/path/to/art"]`. Use an absolute executable path
if your desktop client does not inherit Cargo's PATH. On Windows, use a path
such as `C:/Users/you/Pictures/PixelDraw`.

For OpenCode, merge this entry into your configuration:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "pixeldraw": {
      "type": "local",
      "command": ["pixeldraw", "--output-dir", "/absolute/path/to/art"],
      "enabled": true
    }
  }
}
```

Replace the example path with your own. The repository's `opencode.json` is
a development configuration that runs Cargo from the project directory.
The wire protocol is covered by an automated smoke test; individual client
versions may differ in image display and configuration support.

## Draw your first image

Ask your client:

> Please use MCP tools to draw a pixel-art seaside sunset. The canvas size should be 32*32.

Minimal tool sequence (arguments shown as JSON):

```text
create_canvas {"width":8,"height":8,"palette":"24"}
list_colors   {}
draw_pixels   {"pixels":"0 0 C4 4 0 A4 0 4 B5 4 4 F5","brush":4}
save_image    {"filename":"first-icon.png"}
```

This produces four colored blocks in a 64x64 PNG. Tool calls return PNG image
content and text; use an MCP client that can display image tool results.

## Tools and constraints

| Tool | Behavior |
| --- | --- |
| `create_canvas` | White canvas, width/height 1..128; locks palette 24, 144 or 221 (default). Successful recreation replaces the old canvas. |
| `list_colors` | Lists the locked palette, or a requested palette before creation. |
| `draw_pixels` | Whitespace-separated `x y color` triples; brush 1, 2, 4 or 8. |
| `save_image` | Saves the current 8x PNG without overwriting existing files. |

- Coordinates start at the top left. Brush positions must be multiples of
  the brush size, and the entire square must fit inside the canvas.
- Invalid drawing batches leave the canvas unchanged. Repeated positions
  are applied in order. Colors use MARD codes or the active palette's Chinese names.
- Input is limited to 1 MiB and 65,536 brush stamps per drawing call.
- Only one canvas is held in memory per server instance. Restarting loses
  unsaved work. There is no undo, import, session persistence, or native-size export.
- Images are always enlarged 8x with nearest-neighbor sampling. The 24-color
  legacy palette differs from larger palettes for some names and RGB values.

## Output and troubleshooting

`--output-dir PATH` selects the destination. The default is `./output` relative
to the server's working directory, not the source or executable directory.
Use a directory you control; this local tool is not a filesystem sandbox.

Filenames accept ASCII letters, digits, dots, underscores and hyphens, with
a maximum of 200 bytes including the appended `.png` extension. Paths,
trailing dots and Windows device names are rejected. Omit `filename` for an
automatic name with collision retries; explicit existing names return an error.

If the server appears idle in a terminal, that is expected: it waits for MCP
messages on stdin. Logs go to stderr. For permission failures, select a writable
output directory. If colors are rejected, query `list_colors` rather than
assuming names transfer between palettes. Use `RUST_LOG=debug` for diagnostics.

## Development and licensing

See [CONTRIBUTING.md](CONTRIBUTING.md) for checks and [AGENTS.md](AGENTS.md)
for detailed drawing guidance and color tables. Code is licensed under
[MIT](LICENSE); see [THIRD_PARTY.md](THIRD_PARTY.md) for palette provenance
and the scope of third-party rights. Only the selected gallery images are
included; local reference images, screenshots and the rest of `output/` remain excluded.
