# Changelog

## 0.2.1

- Pixel-art circle and ellipse fills use midpoint half-widths so silhouettes stay round at every size: small bead circles keep their classic shape, and large circles / flat ellipses no longer collapse into cones or boxes.
- Circle and ellipse strokes remain the 1px contour of the matching fill (hollow, gap-free, stroke never outside the fill).
- Triangle strokes are the contour of the barycentric fill, so acute and flat triangles no longer draw edges outside the painted region.
- Regression tests lock circle smoothness (monotone silhouette, true equator) and triangle stroke/fill consistency.

## 0.2.0

- Shape tools: `draw_line`, `draw_rect`, `draw_triangle`, `draw_circle`, `draw_ellipse`, and `flood_fill` for large-area fills and outlines.
- `draw_batch`: submit multiple ordered ops atomically; one final preview and one undo step.
- `draw_curve`: quadratic/cubic Bezier and Catmull-Rom smooth curves through waypoints.
- `undo`: reverse recent drawing steps.
- `save_image` `output`: `pixel`, bead `pattern`, or `both` in one call.
- Pattern sheets: four-side 1-based index bands, per-cell MARD codes, center-out every-10 counting lines, and a color/count legend; sized for Xiaohongshu's 2048px limit with larger codes.
- Unify MARD color names across the 24/144/221 palettes (BeiYa color card).
- `background: empty` starts a canvas with no beads; `flood_fill` / `draw_pixels` accept `-` to clear cells.
- Symmetric Bresenham lines: A→B and B→A always cover the same pixels.
- Circle/ellipse strokes are the 1px contour of the matching fill (hollow, gap-free).
- Unify thick-brush footprints: stroke tools and `draw_pixels` stamp the same top-left `N×N` block.
- Cap circle/ellipse radii at 256 before rasterization.
- Skip undo history for draw calls that write zero pixels.
- Report unique painted cell counts instead of `stamps × brush²`.
- Keep shape and flood-fill bead state in sync (empty cells vs H2 white beads).

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
