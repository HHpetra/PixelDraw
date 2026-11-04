# Changelog

## Unreleased

- **Breaking:** `draw_circle` takes top-left `x`, `y`, and `diameter` (cell count). `draw_ellipse` takes top-left `x`, `y`, `width`, and `height`. `cx`, `cy`, `radius`, `rx`, and `ry` are removed and rejected as unknown fields. `draw_batch` rejects unknown fields on every op, so a leftover `radius` next to `diameter` fails the whole batch. A circle occupies `diameter × diameter` cells and an ellipse occupies `width × height`; a cell is painted when its center lies inside (`dx² + dy² <= 1` after normalizing to `[-1, 1]`). `fill: false` is the 1-cell outline of that fill. Diameters and axes must be integers ≥ 1 and are clamped to 512. The anchor must be on the canvas; right and bottom overflow is clipped. Convert an old radius with `diameter = 2 × radius + 1` (the anchor moves to `center − radius`). Even diameters such as 16 and 32 had no old equivalent.
- `move_region` and `copy_region`: transplant a rectangle, including empty cells, by relative `dx`/`dy` or absolute destination top-left `tx`/`ty` (exactly one pair). The block is read before the destination is written, so a one-cell overlap stays correct. Move fills the source with the current background bead, or leaves spaces when the background is empty, and does not change the remembered background color. Copy keeps the source; empty cells overwrite the destination. A source endpoint or destination rectangle outside the canvas rejects the whole call. A transplant that would not change any cell records no undo step. Both operations are `draw_batch` ops (`type: move_region` / `copy_region`).
- `fill_background`: replace every cell on the current canvas with one palette color (MARD code or Chinese name), overwriting existing artwork and empty cells. An unknown color is rejected and the canvas stays unchanged. A fill that changes nothing records no undo step; any real change is one undo step. The same operation is available in `draw_batch` as `type: fill_background`.
- `create_canvas` accepts optional `background_color` (Chinese name or MARD code). Omitting it keeps `background: white` (H2 beads) and `background: empty` (no beads). When set, every cell starts as that bead and the canvas remembers it as the current background color. `background: empty` cannot be combined with `background_color`. An unknown color does not replace an existing canvas.
- `fill_background` updates that remembered background color to the new color, including when every cell was already that color but the remembered color differed. Undo restores the previous background color together with the cells. A fill that changes neither the cells nor the remembered color still records no undo step.

## 0.2.2

- Ellipse and circle fills rasterize along the major axis and are clamped to a staircase that matches its own transpose, so `(rx, ry)` rotated by 90° now paints exactly the transposed cells of `(ry, rx)`. Tall ellipses used to come out as a 3px-wide banana while the same ellipse rotated stayed a thin lens.
- A circle is now exactly its own transpose at every radius. `r=4` used to paint `4,4,4,3,1` row extents — a notch no rotation could reproduce — and now paints `4,4,3,3,1`.
- A batch (`draw_batch`) can no longer lose its undo step when the history is full. The pre-batch snapshot is captured before the first op instead of being merged afterwards, which used to evict it as soon as the batch wrote 64 steps.
- A batch that paints nothing records no undo step, matching single drawing calls.

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
