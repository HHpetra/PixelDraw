# Changelog

## 0.2.0

- `draw_curve`: quadratic/cubic Bezier and Catmull-Rom smooth curves through waypoints.
- `undo`: reverse recent drawing steps; `draw_batch` collapses to a single undo step.
- Docs and tool instructions cover curves and undo.

## 0.1.1

- Prebuilt Windows x64, Linux x64, macOS ARM64 and macOS x64 archives.
- Automated release builds with release-mode MCP tests and SHA-256 checksums.
- English and Chinese instructions for use without Rust or Cargo.

## 0.1.0

- Initial source release of the stdio MCP server.
- Locked 24/144/221 palettes and canvases up to 128 x 128 pixels.
- Atomic validation of drawing batches and aligned 1/2/4/8 brushes.
- Nearest-neighbor 8x PNG previews and exports.
- Runtime output directory, atomic non-overwriting saves, portable filenames.
- Isolated file tests, MCP smoke coverage, and cross-platform CI.
