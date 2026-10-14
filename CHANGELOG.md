# Changelog

## Unreleased

- Symmetric Bresenham lines: A→B and B→A always cover the same pixels (no direction-dependent jogs on shallow slopes).
- Circle/ellipse strokes are now the 1px contour of the matching fill (hollow, gap-free, consistent with `fill=true`).
- Unify thick-brush footprints: stroke tools and `draw_pixels` now stamp the same top-left `N×N` block at the same coordinates.
- Cap circle/ellipse radii at 256 before rasterization (prevents huge allocations, hangs, and `u32`→`i32` wrap).
- Skip undo history for draw calls that write zero pixels (e.g. fully clipped strokes).
- Report unique painted cell counts instead of `stamps × brush²`.
- `flood_fill` accepts `-` to clear a region back to empty cells.

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
