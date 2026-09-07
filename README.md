# PixelDraw

A local MCP server that lets AI draw pixel art through explicit coordinates
and constrained MARD palettes, rather than text-to-image generation.

Rust executes the drawing commands; your MCP client supplies the model.
PixelDraw itself needs no API key and exposes no network listener.

## Install

Install Rust 1.96.1 or newer using [rustup](https://rustup.rs/), then:

```sh
git clone https://github.com/HHpetra/PixelDraw.git
cd PixelDraw
cargo install --path . --locked
pixeldraw --help
```

The executable is installed into Cargo's bin directory, which must be on PATH.
This first release distributes source, not prebuilt binaries. CI checks
Windows, Linux and macOS; inspect the Actions results for platform status.

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

Ask your client: "Use PixelDraw to create a 16x16 pixel-art icon with the
24-color palette. Query the colors, draw it with aligned brushes, then save
it as first-icon.png."

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
and the scope of third-party rights. Local reference images and generated
output are intentionally excluded from the release.
