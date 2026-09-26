# PixelDraw

[English](README.md) | [简体中文](README.zh-CN.md)

A local MCP server that lets AI draw pixel art through explicit coordinates
and constrained MARD palettes, rather than text-to-image generation.

Rust executes the drawing commands; your MCP client supplies the model.
PixelDraw itself needs no API key and exposes no network listener.

## What it does

PixelDraw gives an AI assistant a small drawing workspace. You describe what
to draw in your MCP client; the assistant creates a canvas and draws with lines,
rectangles, triangles, circles, ellipses, flood fill or pixel stamps. After each pass it
reviews the returned preview, keeps adjusting, then saves when satisfied.

- **Precise pixel placement:** draw on a 1..128 pixel-wide and -high canvas
  with explicit coordinates, rather than asking an image model to imitate pixels.
- **Constrained colors:** choose a locked 24-, 144-, or 221-color MARD palette
  and address colors by code or the palette's Chinese names.
- **Brush sizes:** aligned 1x1, 2x2, 4x4 and 8x8 stamps, also used as line/stroke
  width. An invalid batch is rejected without partially painting it.
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

Use shape tools or precise pixel stamps as needed. After drawing, review the
preview and keep adjusting as needed. Minimal tool sequence (arguments shown as JSON):

```text
create_canvas {"width":16,"height":16,"palette":"24"}
draw_circle   {"cx":8,"cy":8,"radius":5,"color":"C4","fill":true}
draw_rect     {"x0":2,"y0":2,"x1":13,"y1":13,"color":"A4","fill":false,"brush":2}
save_image    {"filename":"first-icon.png"}
```

This produces a filled circle and a stroked rectangle in a 128x128 PNG. Tool
calls return PNG image content and text; use an MCP client that can display
image tool results.

## Choosing an export

`save_image.output` accepts three values; omitting it preserves the existing pixel-only behavior:

| output | Result with `filename: "cat.png"` |
| --- | --- |
| `pixel` (default) | `cat.png`: 8x pixel art, with transparent empty cells |
| `pattern` | `cat.png`: a labelled bead chart |
| `both` | `cat.png` and `cat-pattern.png`, with both previews returned in that order |

Charts show a MARD code inside each occupied cell, one-based row/column numbers, heavier lines every five cells, and rectangular color swatches containing only code and count (e.g. `H2 12`). Rendering uses a built-in bitmap font, requiring no Python, system fonts, or network. Charts are PNG only, without SVG/PDF, pagination, or physical 1:1 printing. Drawing coordinates remain zero-based.

```text
create_canvas {"width":24,"height":24,"palette":"221","background":"empty"}
draw_pixels   {"pixels":"0 0 H2 1 0 H7 2 0 F5"}
draw_pixels   {"pixels":"2 0 -"}
save_image    {"filename":"cat.png","output":"both"}
```

The default `background: "white"` fills the canvas with H2 beads. `empty` starts without beads. Use `-` to clear cells, with the same brushes and batch validation as painting. H2 always means a real white bead; empty cells have no code label and are excluded from totals. The example uses two beads. No white region is automatically removed as background.

Both exports use the same snapshot with canonical bead codes and occupancy, never RGB-to-code inference. Overlapping strokes count only the final cells.

Each file is saved atomically. `both` checks both names before writing, so existing files cause a refusal without writing. The pair is not a transaction: on a late disk error or concurrent collision, the error lists already saved files. Automatically named pairs retry numeric suffixes for collisions. The derived filename must also fit the 200-byte limit; an overlong name fails before writing.

## Tools and constraints

| Tool | Behavior |
| --- | --- |
| `create_canvas` | Canvas (`background`: `white` default beads or `empty`), width/height 1..128; locks palette 24, 144 or 221 (default). Successful recreation replaces the old canvas. |
| `list_colors` | Lists the locked palette, or a requested palette before creation. Optional lookup only. |
| `draw_line` | Line; `brush` is stroke width 1/2/4/8, coordinates need not align to a grid. |
| `draw_rect` | Rectangle; `fill` selects filled vs stroked, stroke uses `brush` width. |
| `draw_triangle` | Triangle; `fill` selects filled vs stroked, stroke uses `brush` width. |
| `draw_circle` | Circle; `fill` selects filled vs stroked, stroke uses `brush` width. |
| `draw_ellipse` | Ellipse; `fill` selects filled vs stroked, stroke uses `brush` width. |
| `flood_fill` | Four-connected fill of the seed's current color. |
| `draw_batch` | Ordered atomic batch; any failure rejects the whole batch; one final preview. |
| `draw_pixels` | Whitespace-separated `x y color` triples; `-` clears a cell. |
| `save_image` | Exports `pixel` (default), `pattern`, or `both` as PNG without overwriting existing files. |

- Prefer `draw_batch` for multi-stroke work; do not call drawing tools in parallel. Review the preview and keep adjusting.
- Stroke/line `brush` sizes are 1, 2, 4 or 8, expanded around each raster point.
  Fills and flood fill ignore brush size.
- For `draw_pixels`, brush positions must be multiples of the brush size, and
  the entire square must fit inside the canvas.
- Invalid drawing batches leave the canvas unchanged. Repeated positions
  are applied in order. Colors use MARD codes or the active palette's Chinese names.
- Circle/ellipse radii may extend past the canvas and are clipped. Line ends,
  circle centers, and flood-fill seeds must stay inside the canvas.
- Only one canvas is held in memory per server instance. Restarting loses
  unsaved work. There is no undo, import, session persistence, or native-size export.
- Pixel previews are enlarged 8x with nearest-neighbor sampling; bead charts have a separate grid layout. The 24-color
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
