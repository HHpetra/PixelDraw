//! Rasterize pixel-art primitives to center-line / interior points.
//! Coordinates are signed so thick brushes can expand past the origin;
//! the canvas clips when stamping.

/// Integer Bresenham line. Same endpoints always cover the same pixels,
/// regardless of draw direction (A→B equals reversed B→A).
pub fn line_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    // Canonicalize start/end so tie-breaking does not depend on call order.
    let reverse = (x1, y1) < (x0, y0);
    let (ax, ay, bx, by) = if reverse {
        (x1, y1, x0, y0)
    } else {
        (x0, y0, x1, y1)
    };
    let mut points = Vec::new();
    let mut x = ax;
    let mut y = ay;
    let dx = (bx - ax).abs();
    let dy = -(by - ay).abs();
    let sx = if ax < bx { 1 } else { -1 };
    let sy = if ay < by { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        points.push((x, y));
        if x == bx && y == by {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    if reverse {
        points.reverse();
    }
    points
}

/// Normalize two corners into inclusive min/max bounds.
pub fn rect_bounds(x0: i32, y0: i32, x1: i32, y1: i32) -> (i32, i32, i32, i32) {
    (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

pub fn rect_fill_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let (left, top, right, bottom) = rect_bounds(x0, y0, x1, y1);
    let mut points = Vec::with_capacity(((right - left + 1) * (bottom - top + 1)) as usize);
    for y in top..=bottom {
        for x in left..=right {
            points.push((x, y));
        }
    }
    points
}

pub fn rect_stroke_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let (left, top, right, bottom) = rect_bounds(x0, y0, x1, y1);
    if left == right && top == bottom {
        return vec![(left, top)];
    }
    if left == right {
        return line_points(left, top, left, bottom);
    }
    if top == bottom {
        return line_points(left, top, right, top);
    }
    let mut points = Vec::with_capacity((2 * (right - left + 1) + 2 * (bottom - top - 1)) as usize);
    for x in left..=right {
        points.push((x, top));
        points.push((x, bottom));
    }
    for y in (top + 1)..bottom {
        points.push((left, y));
        points.push((right, y));
    }
    points
}

/// 1px boundary of a filled cell set (cells with a 4-neighbor outside).
/// Used so shape strokes stay hollow, thin, and consistent with their fill.
fn outline_cells(fill: &[(i32, i32)]) -> Vec<(i32, i32)> {
    use std::collections::HashSet;
    let set: HashSet<(i32, i32)> = fill.iter().copied().collect();
    fill.iter()
        .copied()
        .filter(|&(x, y)| {
            !set.contains(&(x + 1, y))
                || !set.contains(&(x - 1, y))
                || !set.contains(&(x, y + 1))
                || !set.contains(&(x, y - 1))
        })
        .collect()
}

/// Inclusive columns of one ellipse row.
///
/// `max_dx2` is the greatest squared doubled-offset allowed on this row.
/// Cell centers sit on odd or even doubled coordinates (same parity as
/// `1 - width`), so the span is every column whose center satisfies
/// `|dx|² <= max_dx2`, with no second rounding rule.
fn column_span(width: i64, max_dx2: i64) -> Option<(i64, i64)> {
    let max_dx = i64::try_from((max_dx2.max(0) as u64).isqrt()).expect("half-width fits i64");
    // dx = 2*col + 1 - width, and |dx| <= max_dx.
    // ceil((width - 1 - max_dx) / 2); the dividend is >= -1 for a real ellipse.
    let span_start = width - 1 - max_dx;
    let col_min = (if span_start >= 0 {
        (span_start + 1) / 2
    } else {
        span_start / 2
    })
    .max(0);
    let col_max = ((width - 1 + max_dx) / 2).min(width - 1);
    (col_min <= col_max).then_some((col_min, col_max))
}

/// Filled pixel circle anchored at the top-left `(x, y)` of its bounding box.
/// `diameter` is the side length in cells and must be >= 1.
pub fn circle_fill_points(x: i32, y: i32, diameter: i32) -> Vec<(i32, i32)> {
    ellipse_fill_points(x, y, diameter, diameter)
}

/// 1-cell outline of [`circle_fill_points`]. Same predicate, so the stroke
/// stays inside the fill.
pub fn circle_stroke_points(x: i32, y: i32, diameter: i32) -> Vec<(i32, i32)> {
    ellipse_stroke_points(x, y, diameter, diameter)
}

/// 1-cell outline of [`ellipse_fill_points`].
pub fn ellipse_stroke_points(x: i32, y: i32, width: i32, height: i32) -> Vec<(i32, i32)> {
    outline_cells(&ellipse_fill_points(x, y, width, height))
}

fn edge(ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32) -> i64 {
    (bx as i64 - ax as i64) * (py as i64 - ay as i64)
        - (by as i64 - ay as i64) * (px as i64 - ax as i64)
}

pub fn triangle_stroke_points(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
) -> Vec<(i32, i32)> {
    // Degenerate (collinear) triangles have no interior — the three edges
    // already form a thin polyline. Otherwise contour the fill so the ring
    // always sits on the same boundary as `triangle_fill_points` (Bresenham
    // edges can fall outside the barycentric fill on acute/flat triangles).
    if edge(x0, y0, x1, y1, x2, y2) == 0 {
        return collinear_triangle_edges(x0, y0, x1, y1, x2, y2);
    }
    outline_cells(&triangle_fill_points(x0, y0, x1, y1, x2, y2))
}

fn collinear_triangle_edges(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
) -> Vec<(i32, i32)> {
    let mut points = line_points(x0, y0, x1, y1);
    points.extend(line_points(x1, y1, x2, y2));
    points.extend(line_points(x2, y2, x0, y0));
    points
}

pub fn triangle_fill_points(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
) -> Vec<(i32, i32)> {
    let area = edge(x0, y0, x1, y1, x2, y2);
    if area == 0 {
        return collinear_triangle_edges(x0, y0, x1, y1, x2, y2);
    }
    // Center-in barycentric pixels alone leave thin triangles as disconnected
    // islands (a sharp tip can sit two steps away from the rest). The stroke
    // is the outline of this fill, so those islands become gaps. Span each
    // row from the leftmost to the rightmost pixel of the barycentric set
    // and the three Bresenham edges — edges cover every y, and the span
    // closes the islands without leaving the vertex bounding box.
    let left = x0.min(x1).min(x2);
    let right = x0.max(x1).max(x2);
    let top = y0.min(y1).min(y2);
    let bottom = y0.max(y1).max(y2);
    let rows = (bottom - top + 1) as usize;
    let mut min_x = vec![i32::MAX; rows];
    let mut max_x = vec![i32::MIN; rows];
    let mut cover = |x: i32, y: i32| {
        let i = (y - top) as usize;
        min_x[i] = min_x[i].min(x);
        max_x[i] = max_x[i].max(x);
    };
    for y in top..=bottom {
        for x in left..=right {
            let w0 = edge(x1, y1, x2, y2, x, y);
            let w1 = edge(x2, y2, x0, y0, x, y);
            let w2 = edge(x0, y0, x1, y1, x, y);
            let same_sign = (w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0);
            if same_sign {
                cover(x, y);
            }
        }
    }
    for (x, y) in line_points(x0, y0, x1, y1)
        .into_iter()
        .chain(line_points(x1, y1, x2, y2))
        .chain(line_points(x2, y2, x0, y0))
    {
        cover(x, y);
    }
    let mut points = Vec::new();
    for (i, y) in (top..=bottom).enumerate() {
        for x in min_x[i]..=max_x[i] {
            points.push((x, y));
        }
    }
    points
}

/// Filled pixel ellipse anchored at the top-left `(x, y)`.
///
/// A cell `(i, j)` is inside when its center `(i+0.5, j+0.5)` lies in the
/// ellipse centered at `(x + width/2, y + height/2)` with half-axes
/// `width/2` and `height/2`. In doubled integers that is
/// `dx = 2*(i-x)+1-width`, `dy = 2*(j-y)+1-height`, and
/// `dx² * height² + dy² * width² <= width² * height²`.
///
/// Each row takes one half-width from that inequality
/// (`dx² <= width² * (height² - dy²) / height²`, floored). There is no
/// half-pixel cover test, no analytic `+1`, and no transpose clamp: the
/// predicate is symmetric, so a circle is its own transpose and swapping
/// `width` with `height` swaps the cells.
///
/// `width` and `height` must be >= 1. Callers clamp values above 512.
pub fn ellipse_fill_points(x: i32, y: i32, width: i32, height: i32) -> Vec<(i32, i32)> {
    if width < 1 || height < 1 {
        return Vec::new();
    }
    let width64 = i64::from(width);
    let height64 = i64::from(height);
    let w2 = width64 * width64;
    let h2 = height64 * height64;
    let mut points = Vec::new();
    for row in 0..height {
        let dy = 2 * i64::from(row) + 1 - height64;
        let dy2 = dy * dy;
        debug_assert!(dy2 < h2);
        let max_dx2 = w2 * (h2 - dy2) / h2;
        let Some((col_min, col_max)) = column_span(width64, max_dx2) else {
            continue;
        };
        let py = y + row;
        for col in col_min..=col_max {
            points.push((x + i32::try_from(col).expect("column fits i32"), py));
        }
    }
    points
}

/// Filled pixel diamond centered on cell `(cx, cy)`.
///
/// `rx` and `ry` are non-negative diamond half-axes: cells from the center to
/// one side. The diamond occupies `(2*rx+1) × (2*ry+1)` cells. A cell
/// `(x, y)` is inside when its center lies in the diamond, i.e.
/// `|x-cx|*ry + |y-cy|*rx <= rx*ry`. There is no coverage test and no second
/// rule. `rx = 0` is a vertical bar, `ry = 0` is a horizontal bar, and both
/// 0 is the center cell — the same inequality, with no special case.
/// Callers clamp half-axes above 255.
pub fn diamond_fill_points(cx: i32, cy: i32, rx: i32, ry: i32) -> Vec<(i32, i32)> {
    let rx64 = i64::from(rx);
    let ry64 = i64::from(ry);
    let limit = rx64 * ry64;
    let mut points = Vec::new();
    for dj in -ry..=ry {
        let abs_dj = i64::from(dj.abs());
        let py = cy + dj;
        for di in -rx..=rx {
            let abs_di = i64::from(di.abs());
            if abs_di * ry64 + abs_dj * rx64 <= limit {
                points.push((cx + di, py));
            }
        }
    }
    points
}

/// 1-cell outline of [`diamond_fill_points`]. Same predicate as circle and
/// ellipse strokes: a filled cell with a 4-neighbor outside the fill.
pub fn diamond_stroke_points(cx: i32, cy: i32, rx: i32, ry: i32) -> Vec<(i32, i32)> {
    outline_cells(&diamond_fill_points(cx, cy, rx, ry))
}

/// Connect sampled curve positions with Bresenham lines so short segments stay gap-free.
fn chain_samples(samples: &[(f64, f64)]) -> Vec<(i32, i32)> {
    let mut points = Vec::new();
    if samples.is_empty() {
        return points;
    }
    let mut prev = (samples[0].0.round() as i32, samples[0].1.round() as i32);
    points.push(prev);
    for &(fx, fy) in &samples[1..] {
        let next = (fx.round() as i32, fy.round() as i32);
        if next != prev {
            points.extend(line_points(prev.0, prev.1, next.0, next.1));
            prev = next;
        }
    }
    points
}

/// Quadratic Bezier (P0, control, P1). Always includes both endpoints.
pub fn quadratic_bezier_points(
    x0: i32,
    y0: i32,
    cx: i32,
    cy: i32,
    x1: i32,
    y1: i32,
) -> Vec<(i32, i32)> {
    let (x0, y0, cx, cy, x1, y1) = (
        x0 as f64, y0 as f64, cx as f64, cy as f64, x1 as f64, y1 as f64,
    );
    let span = ((x1 - x0).abs() + (y1 - y0).abs() + (cx - x0).abs() + (cy - y0).abs()) as i32;
    let steps = (span * 4).clamp(8, 4096) as usize;
    let mut samples = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let u = 1.0 - t;
        let x = u * u * x0 + 2.0 * u * t * cx + t * t * x1;
        let y = u * u * y0 + 2.0 * u * t * cy + t * t * y1;
        samples.push((x, y));
    }
    chain_samples(&samples)
}

/// Cubic Bezier (P0, C1, C2, P1). Always includes both endpoints.
pub fn cubic_bezier_points(
    p0: (i32, i32),
    c1: (i32, i32),
    c2: (i32, i32),
    p1: (i32, i32),
) -> Vec<(i32, i32)> {
    let (x0, y0, cx1, cy1, cx2, cy2, x1, y1) = (
        p0.0 as f64,
        p0.1 as f64,
        c1.0 as f64,
        c1.1 as f64,
        c2.0 as f64,
        c2.1 as f64,
        p1.0 as f64,
        p1.1 as f64,
    );
    let span = (x1 - x0).abs()
        + (y1 - y0).abs()
        + (cx1 - x0).abs()
        + (cy1 - y0).abs()
        + (cx2 - x0).abs()
        + (cy2 - y0).abs();
    let steps = (span as i32 * 4).clamp(8, 4096) as usize;
    let mut samples = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let u = 1.0 - t;
        let uu = u * u;
        let tt = t * t;
        let x = uu * u * x0 + 3.0 * uu * t * cx1 + 3.0 * u * tt * cx2 + tt * t * x1;
        let y = uu * u * y0 + 3.0 * uu * t * cy1 + 3.0 * u * tt * cy2 + tt * t * y1;
        samples.push((x, y));
    }
    chain_samples(&samples)
}

/// Catmull-Rom curve through all waypoints (endpoints included).
/// Two points collapse to a straight line.
pub fn catmull_rom_points(points: &[(i32, i32)]) -> Vec<(i32, i32)> {
    match points.len() {
        0 => Vec::new(),
        1 => vec![points[0]],
        2 => line_points(points[0].0, points[0].1, points[1].0, points[1].1),
        n => {
            let mut samples: Vec<(f64, f64)> = Vec::new();
            for i in 0..n - 1 {
                let p0 = points[i.saturating_sub(1)];
                let p1 = points[i];
                let p2 = points[i + 1];
                let p3 = points[(i + 2).min(n - 1)];
                let (x0, y0) = (p0.0 as f64, p0.1 as f64);
                let (x1, y1) = (p1.0 as f64, p1.1 as f64);
                let (x2, y2) = (p2.0 as f64, p2.1 as f64);
                let (x3, y3) = (p3.0 as f64, p3.1 as f64);
                let span = ((x2 - x1).abs() + (y2 - y1).abs()) as i32;
                let steps = (span * 4).clamp(4, 512) as usize;
                for step in 0..steps {
                    let t = step as f64 / steps as f64;
                    let t2 = t * t;
                    let t3 = t2 * t;
                    // Uniform Catmull-Rom, equivalent to a cubic Bezier of the segment.
                    let x = 0.5
                        * ((2.0 * x1)
                            + (-x0 + x2) * t
                            + (2.0 * x0 - 5.0 * x1 + 4.0 * x2 - x3) * t2
                            + (-x0 + 3.0 * x1 - 3.0 * x2 + x3) * t3);
                    let y = 0.5
                        * ((2.0 * y1)
                            + (-y0 + y2) * t
                            + (2.0 * y0 - 5.0 * y1 + 4.0 * y2 - y3) * t2
                            + (-y0 + 3.0 * y1 - 3.0 * y2 + y3) * t3);
                    samples.push((x, y));
                }
            }
            let last = points[n - 1];
            samples.push((last.0 as f64, last.1 as f64));
            chain_samples(&samples)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn sorted(mut points: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
        points.sort_unstable();
        points.dedup();
        points
    }

    fn unique(points: &[(i32, i32)]) -> HashSet<(i32, i32)> {
        points.iter().copied().collect()
    }

    /// Cells whose centers lie inside the top-left-anchored ellipse.
    /// Independent of the row half-width formula used by the rasterizer.
    fn oracle_ellipse(x: i32, y: i32, width: i32, height: i32) -> HashSet<(i32, i32)> {
        let w = i64::from(width);
        let h = i64::from(height);
        let limit = w * w * h * h;
        let mut cells = HashSet::new();
        for row in 0..height {
            for col in 0..width {
                let dx = 2 * i64::from(col) + 1 - w;
                let dy = 2 * i64::from(row) + 1 - h;
                if dx * dx * h * h + dy * dy * w * w <= limit {
                    cells.insert((x + col, y + row));
                }
            }
        }
        cells
    }

    fn row_width(fill: &HashSet<(i32, i32)>, y: i32, x0: i32, width: i32) -> i32 {
        (x0..x0 + width).filter(|&x| fill.contains(&(x, y))).count() as i32
    }

    /// Cells of the top row whose centers satisfy the circle predicate.
    fn true_top_chord(diameter: i32) -> i32 {
        let d = i64::from(diameter);
        (0..d)
            .filter(|&col| {
                let dx = 2 * col + 1 - d;
                let dy = 1 - d;
                dx * dx + dy * dy <= d * d
            })
            .count() as i32
    }

    #[test]
    fn line_is_bresenham() {
        assert_eq!(
            line_points(0, 0, 3, 0),
            vec![(0, 0), (1, 0), (2, 0), (3, 0)]
        );
        assert_eq!(line_points(0, 0, 0, 2), vec![(0, 0), (0, 1), (0, 2)]);
        assert_eq!(line_points(1, 1, 1, 1), vec![(1, 1)]);
        let diag = line_points(0, 0, 2, 2);
        assert_eq!(diag, vec![(0, 0), (1, 1), (2, 2)]);
    }

    #[test]
    fn rect_normalizes_corners() {
        assert_eq!(rect_bounds(3, 4, 1, 2), (1, 2, 3, 4));
        assert_eq!(rect_fill_points(1, 1, 1, 1), vec![(1, 1)]);
        let stroke = sorted(rect_stroke_points(0, 0, 2, 2));
        assert_eq!(
            stroke,
            sorted(vec![
                (0, 0),
                (1, 0),
                (2, 0),
                (0, 1),
                (2, 1),
                (0, 2),
                (1, 2),
                (2, 2)
            ])
        );
    }

    fn widths_top_to_bottom(fill: &HashSet<(i32, i32)>, size: i32) -> Vec<i32> {
        (0..size).map(|y| row_width(fill, y, 0, size)).collect()
    }

    /// Row widths from the equator toward the top, inclusive.
    fn widths_equator_to_top(fill: &HashSet<(i32, i32)>, diameter: i32) -> Vec<i32> {
        let equator = if diameter % 2 == 0 {
            diameter / 2 - 1
        } else {
            diameter / 2
        };
        (0..=equator)
            .rev()
            .map(|y| row_width(fill, y, 0, diameter))
            .collect()
    }

    fn assert_four_fold(fill: &HashSet<(i32, i32)>, width: i32, height: i32) {
        for &(x, y) in fill {
            assert!(
                fill.contains(&(width - 1 - x, y)),
                "left-right mismatch at ({x},{y})"
            );
            assert!(
                fill.contains(&(x, height - 1 - y)),
                "up-down mismatch at ({x},{y})"
            );
            assert!(fill.contains(&(width - 1 - x, height - 1 - y)));
        }
    }

    #[test]
    fn circle_diameter_one_is_one_cell_and_two_is_four() {
        assert_eq!(circle_fill_points(5, 5, 1), vec![(5, 5)]);
        assert_eq!(
            sorted(circle_fill_points(3, 4, 2)),
            vec![(3, 4), (3, 5), (4, 4), (4, 5)]
        );
        // Nothing outside the bounding box.
        let fill = unique(&circle_fill_points(5, 5, 4));
        assert!(
            fill.iter()
                .all(|&(x, y)| (5..9).contains(&x) && (5..9).contains(&y))
        );
        assert!(!fill.contains(&(5, 9)));
        assert!(!fill.contains(&(9, 5)));
    }

    #[test]
    fn circle_shape_follows_cell_center_rules() {
        for diameter in 1..=64 {
            let fill = unique(&circle_fill_points(0, 0, diameter));
            assert_eq!(
                fill,
                oracle_ellipse(0, 0, diameter, diameter),
                "d={diameter} drifted from the cell-center predicate"
            );
            assert_four_fold(&fill, diameter, diameter);
            let equator = if diameter % 2 == 0 {
                diameter / 2 - 1
            } else {
                diameter / 2
            };
            assert_eq!(
                row_width(&fill, equator, 0, diameter),
                diameter,
                "d={diameter} equator"
            );
            if diameter % 2 == 0 {
                assert_eq!(row_width(&fill, diameter / 2, 0, diameter), diameter);
                let mid = diameter / 2;
                for &(dx, dy) in &[(-1, -1), (0, -1), (-1, 0), (0, 0)] {
                    assert!(
                        fill.contains(&(mid + dx, mid + dy)),
                        "d={diameter} missing central cell"
                    );
                }
            } else {
                let mid = diameter / 2;
                assert!(
                    fill.contains(&(mid, mid)),
                    "d={diameter} missing center cell"
                );
            }
            let mut prev = row_width(&fill, equator, 0, diameter);
            for y in (0..equator).rev() {
                let width = row_width(&fill, y, 0, diameter);
                assert!(
                    width <= prev,
                    "d={diameter} y={y}: silhouette grew {prev} -> {width}"
                );
                prev = width;
            }
            assert_eq!(
                row_width(&fill, 0, 0, diameter),
                true_top_chord(diameter),
                "d={diameter} top chord"
            );
            if diameter >= 4 {
                assert!(
                    row_width(&fill, 0, 0, diameter) < diameter,
                    "d={diameter} top is as wide as the equator"
                );
            }
        }
        // Locked silhouettes: equator → top. Diameter 9 used to be 9,9,7,7,3.
        assert_eq!(
            widths_equator_to_top(&unique(&circle_fill_points(0, 0, 9)), 9),
            vec![9, 9, 9, 7, 5]
        );
        assert_eq!(
            widths_equator_to_top(&unique(&circle_fill_points(0, 0, 16)), 16),
            vec![16, 16, 16, 14, 14, 12, 10, 6]
        );
        assert_eq!(
            widths_top_to_bottom(&unique(&circle_fill_points(0, 0, 9)), 9)[0],
            5
        );
    }

    #[test]
    fn large_circle_top_matches_the_chord_not_a_cone() {
        // A "drop at most one cell per row" floor would leave a blunt cone.
        // The top row is the real chord, much narrower than the equator.
        for diameter in [32, 64, 128, 256, 512] {
            let fill = unique(&circle_fill_points(0, 0, diameter));
            let top = row_width(&fill, 0, 0, diameter);
            let equator = if diameter % 2 == 0 {
                diameter / 2 - 1
            } else {
                diameter / 2
            };
            assert_eq!(row_width(&fill, equator, 0, diameter), diameter);
            assert_eq!(top, true_top_chord(diameter), "d={diameter}");
            assert!(top < diameter / 2, "d={diameter} top={top} is conical");
            assert_eq!(fill, oracle_ellipse(0, 0, diameter, diameter));
        }
    }

    #[test]
    fn flat_ellipse_is_not_a_rectangle() {
        // Height 1 is a bar: every cell center sits on the axis.
        let bar = unique(&ellipse_fill_points(0, 0, 20, 1));
        assert_eq!(bar.len(), 20);
        // A wide flat ellipse is inset from its box. Very flat ones (20×2)
        // can share one width on both rows; taller ones taper toward the pole.
        for &(width, height) in &[(20, 2), (20, 4), (15, 4), (32, 3), (7, 4)] {
            let fill = unique(&ellipse_fill_points(0, 0, width, height));
            assert_eq!(fill, oracle_ellipse(0, 0, width, height));
            assert!(
                fill.len() < (width * height) as usize,
                "{width}x{height} filled its box"
            );
            let mid = if height % 2 == 0 {
                height / 2 - 1
            } else {
                height / 2
            };
            let middle = row_width(&fill, mid, 0, width);
            let top = row_width(&fill, 0, 0, width);
            assert!(top <= middle, "{width}x{height} top={top} middle={middle}");
            assert!(top < width, "{width}x{height} top row is the full width");
        }
        let tapered = unique(&ellipse_fill_points(0, 0, 20, 4));
        assert!(row_width(&tapered, 0, 0, 20) < row_width(&tapered, 1, 0, 20));
    }

    #[test]
    fn ellipse_matches_oracle_transpose_and_circle() {
        for width in 1..=64 {
            for height in 1..=64 {
                let fill = unique(&ellipse_fill_points(0, 0, width, height));
                assert_eq!(
                    fill,
                    oracle_ellipse(0, 0, width, height),
                    "{width}x{height} drifted from the cell-center predicate"
                );
                let turned = unique(&ellipse_fill_points(0, 0, height, width));
                let transpose: HashSet<(i32, i32)> = turned.iter().map(|&(x, y)| (y, x)).collect();
                assert_eq!(fill, transpose, "{width}x{height} is not the transpose");
                if width == height {
                    assert_eq!(fill, unique(&circle_fill_points(0, 0, width)));
                }
            }
        }
        for &(x, y, width, height) in &[
            (5, 7, 9, 16),
            (1, 0, 2, 2),
            (4, 4, 1, 8),
            (-3, 2, 15, 6),
            (0, 0, 512, 3),
            (0, 0, 3, 512),
            (2, 2, 512, 512),
        ] {
            assert_eq!(
                unique(&ellipse_fill_points(x, y, width, height)),
                oracle_ellipse(x, y, width, height),
                "anchor ({x},{y}) {width}x{height}"
            );
        }
    }

    #[test]
    fn tall_ellipse_tapers_to_a_single_cell() {
        // A 3×12 ellipse fills its equator and tapers to one cell at each pole.
        let fill = unique(&ellipse_fill_points(2, 1, 3, 12));
        assert_eq!(fill, oracle_ellipse(2, 1, 3, 12));
        assert_eq!(row_width(&fill, 1 + 5, 2, 3), 3, "equator width");
        assert_eq!(row_width(&fill, 1 + 6, 2, 3), 3, "equator width");
        assert_eq!(row_width(&fill, 1, 2, 3), 1, "top pole");
        assert_eq!(row_width(&fill, 1 + 11, 2, 3), 1, "bottom pole");
        let mut prev = 3;
        for y in (1..1 + 5).rev() {
            let width = row_width(&fill, y, 2, 3);
            assert!(width <= prev, "y={y} widened to {width} from {prev}");
            prev = width;
        }
    }

    #[test]
    fn triangle_fill_and_stroke() {
        let fill = sorted(triangle_fill_points(0, 0, 4, 0, 0, 4));
        assert!(fill.contains(&(0, 0)));
        assert!(fill.contains(&(1, 1)));
        assert!(fill.contains(&(0, 3)));
        assert!(!fill.contains(&(3, 3)));
        let stroke = sorted(triangle_stroke_points(0, 0, 2, 0, 0, 2));
        assert!(stroke.contains(&(0, 0)));
        assert!(stroke.contains(&(2, 0)));
        assert!(stroke.contains(&(0, 2)));
        assert!(stroke.contains(&(1, 1)) || stroke.contains(&(0, 1)));
    }

    #[test]
    fn ellipse_fill_covers_axis_extents() {
        // 7×3 ellipse anchored at (4, 4): equator is the full middle row,
        // the bounding box is y = 4..=6, and the corners of that box are outside.
        let fill = unique(&ellipse_fill_points(4, 4, 7, 3));
        assert_eq!(fill, oracle_ellipse(4, 4, 7, 3));
        assert!(fill.contains(&(4, 5)));
        assert!(fill.contains(&(10, 5)));
        assert!(fill.contains(&(7, 5)));
        assert!(
            !fill.contains(&(4, 4)),
            "corner of the box is outside the ellipse"
        );
        assert!(!fill.contains(&(7, 3)));
        assert!(!fill.contains(&(7, 7)));
        assert!(
            fill.iter()
                .all(|&(x, y)| (4..11).contains(&x) && (4..7).contains(&y))
        );
    }

    #[test]
    fn bezier_includes_endpoints_and_bulges_toward_control() {
        let quad = sorted(quadratic_bezier_points(0, 0, 8, 0, 8, 8));
        assert!(quad.contains(&(0, 0)));
        assert!(quad.contains(&(8, 8)));
        // Mid-curve should lean toward the control point (8,0).
        assert!(quad.iter().any(|&(x, y)| x >= 5 && y <= 3));

        let cubic = sorted(cubic_bezier_points((0, 8), (0, 0), (8, 0), (8, 8)));
        assert!(cubic.contains(&(0, 8)));
        assert!(cubic.contains(&(8, 8)));
        assert!(cubic.iter().any(|&(x, y)| x <= 3 && y <= 3));
    }

    #[test]
    fn catmull_rom_passes_through_waypoints() {
        let path = catmull_rom_points(&[(0, 8), (4, 0), (8, 8)]);
        assert!(path.contains(&(0, 8)));
        assert!(path.contains(&(4, 0)));
        assert!(path.contains(&(8, 8)));
        assert_eq!(
            catmull_rom_points(&[(1, 1), (3, 1)]),
            line_points(1, 1, 3, 1)
        );
    }

    // --- Drawing quality: continuity (no gaps) and thinness (no extra bulk) ---

    /// Consecutive samples may sit on the same pixel, but never jump.
    fn is_8_connected_path(points: &[(i32, i32)]) -> bool {
        points
            .windows(2)
            .all(|w| (w[0].0 - w[1].0).abs() <= 1 && (w[0].1 - w[1].1).abs() <= 1)
    }

    /// Every cell is reachable from an arbitrary start via 8-neighbors.
    fn is_8_connected_region(cells: &HashSet<(i32, i32)>) -> bool {
        let Some(&start) = cells.iter().next() else {
            return true;
        };
        let mut seen = HashSet::from([start]);
        let mut stack = vec![start];
        while let Some((x, y)) = stack.pop() {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let next = (x + dx, y + dy);
                    if cells.contains(&next) && seen.insert(next) {
                        stack.push(next);
                    }
                }
            }
        }
        seen.len() == cells.len()
    }

    /// Bresenham line: endpoints on the path, one cell per major-axis step
    /// (never two cells sharing a major coordinate → no unnecessary thickness),
    /// and 8-connected samples with no gaps.
    fn assert_line_quality(x0: i32, y0: i32, x1: i32, y1: i32) {
        let points = line_points(x0, y0, x1, y1);
        assert_eq!(points.first(), Some(&(x0, y0)), "{x0},{y0}->{x1},{y1}");
        assert_eq!(points.last(), Some(&(x1, y1)), "{x0},{y0}->{x1},{y1}");
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        assert_eq!(
            points.len(),
            dx.max(dy) as usize + 1,
            "line should be thin: {x0},{y0}->{x1},{y1}"
        );
        for w in points.windows(2) {
            let step_x = (w[0].0 - w[1].0).abs();
            let step_y = (w[0].1 - w[1].1).abs();
            // Orthogonal or diagonal neighbor; never a jump, never a duplicate.
            assert!(
                step_x <= 1 && step_y <= 1 && step_x + step_y >= 1,
                "gap or duplicate step {w:?} on {x0},{y0}->{x1},{y1}"
            );
        }
        let mut xs: Vec<i32> = points.iter().map(|p| p.0).collect();
        xs.sort_unstable();
        let count = xs.len();
        xs.dedup();
        let mut ys: Vec<i32> = points.iter().map(|p| p.1).collect();
        ys.sort_unstable();
        let count_y = ys.len();
        ys.dedup();
        if dx >= dy {
            assert_eq!(
                xs.len(),
                count,
                "two cells share a column (thick) {x0},{y0}->{x1},{y1}"
            );
        } else {
            assert_eq!(
                ys.len(),
                count_y,
                "two cells share a row (thick) {x0},{y0}->{x1},{y1}"
            );
        }
    }

    #[test]
    fn lines_in_all_directions_are_connected_and_thin() {
        // Axis-aligned, diagonals, shallow, steep, and every sign combination.
        let deltas = [
            (0, 0),
            (7, 0),
            (0, 7),
            (-7, 0),
            (0, -7),
            (6, 6),
            (6, -6),
            (-6, 6),
            (-6, -6),
            (8, 1),
            (8, -1),
            (-8, 1),
            (-8, -1),
            (1, 8),
            (1, -8),
            (-1, 8),
            (-1, -8),
            (8, 2),
            (8, 3),
            (5, 8),
            (2, 8),
            (3, 5),
            (4, 4),
            (5, 3),
        ];
        for &(dx, dy) in &deltas {
            // Keep both endpoints inside a 17×17 field around the origin.
            let (x0, y0) = if dx >= 0 && dy >= 0 {
                (0, 0)
            } else if dx < 0 && dy >= 0 {
                (-dx, 0)
            } else if dx >= 0 && dy < 0 {
                (0, -dy)
            } else {
                (-dx, -dy)
            };
            let (x1, y1) = (x0 + dx, y0 + dy);
            assert_line_quality(x0, y0, x1, y1);
            // Reversing the endpoints must stay thin and gap-free.
            assert_line_quality(x1, y1, x0, y0);
        }
    }

    #[test]
    fn line_neighbors_are_not_implicitly_thickened() {
        // A long shallow line must not spill into adjacent rows.
        let points = line_points(0, 0, 12, 2);
        let rows: HashSet<i32> = points.iter().map(|p| p.1).collect();
        assert!(rows.len() <= 3, "shallow line spread over rows {rows:?}");
        let points = line_points(0, 0, 2, 12);
        let cols: HashSet<i32> = points.iter().map(|p| p.0).collect();
        assert!(cols.len() <= 3, "steep line spread over cols {cols:?}");
    }

    #[test]
    fn rect_stroke_is_hollow_and_fill_is_exact_block() {
        let stroke = unique(&rect_stroke_points(1, 1, 6, 5));
        for y in 2..5 {
            for x in 2..6 {
                assert!(
                    !stroke.contains(&(x, y)),
                    "stroke leaked into interior ({x},{y})"
                );
            }
        }
        for x in 1..=6 {
            assert!(stroke.contains(&(x, 1)), "missing top edge x={x}");
            assert!(stroke.contains(&(x, 5)), "missing bottom edge x={x}");
        }
        for y in 1..=5 {
            assert!(stroke.contains(&(1, y)), "missing left edge y={y}");
            assert!(stroke.contains(&(6, y)), "missing right edge y={y}");
        }
        // 1px border of a 6×5 block: 2w + 2h − 4 = 18 cells.
        assert_eq!(
            stroke.len(),
            2 * 6 + 2 * 5 - 4,
            "stroke thicker or thinner than a 1px border"
        );

        let fill = unique(&rect_fill_points(1, 1, 6, 5));
        assert_eq!(fill.len(), 6 * 5);
        assert!(stroke.is_subset(&fill));
    }

    #[test]
    fn lines_are_direction_independent() {
        // Drawing A→B must cover exactly the same pixels as B→A.
        let pairs = [
            ((0, 0), (8, 1)),
            ((0, 0), (4, 1)),
            ((0, 0), (5, 2)),
            ((0, 0), (15, 2)),
            ((0, 0), (7, 0)),
            ((0, 0), (0, 7)),
            ((1, 1), (6, 6)),
            ((6, 1), (1, 6)),
            ((2, 10), (11, 3)),
            ((0, 0), (1, 8)),
        ];
        for &(a, b) in &pairs {
            let forward = line_points(a.0, a.1, b.0, b.1);
            let mut backward = line_points(b.0, b.1, a.0, a.1);
            backward.reverse();
            assert_eq!(
                forward, backward,
                "direction-dependent raster for {a:?}->{b:?}"
            );
        }
    }

    fn interior_cells(fill: &HashSet<(i32, i32)>) -> HashSet<(i32, i32)> {
        fill.iter()
            .copied()
            .filter(|&(x, y)| {
                fill.contains(&(x + 1, y))
                    && fill.contains(&(x - 1, y))
                    && fill.contains(&(x, y + 1))
                    && fill.contains(&(x, y - 1))
            })
            .collect()
    }

    fn assert_stroke_outline(fill: &HashSet<(i32, i32)>, stroke: &HashSet<(i32, i32)>) {
        assert!(
            stroke.is_subset(fill),
            "stroke outside fill: {:?}",
            stroke.difference(fill).take(8).collect::<Vec<_>>()
        );
        assert!(
            stroke.is_disjoint(&interior_cells(fill)),
            "stroke filled the interior"
        );
        assert!(is_8_connected_region(stroke), "stroke is not 8-connected");
    }

    #[test]
    fn circle_stroke_is_the_outline_of_the_fill() {
        for diameter in 1..=64 {
            let fill = unique(&circle_fill_points(2, 3, diameter));
            let stroke = unique(&circle_stroke_points(2, 3, diameter));
            assert_stroke_outline(&fill, &stroke);
            if diameter >= 8 {
                assert!(
                    stroke.len() * 2 < fill.len(),
                    "d={diameter} stroke looks filled: {} vs {}",
                    stroke.len(),
                    fill.len()
                );
            }
        }
        // Diameter 9 anchored at (1, 1): the unique center cell is interior.
        let stroke = unique(&circle_stroke_points(1, 1, 9));
        assert!(!stroke.contains(&(5, 5)));
        let fill = unique(&circle_fill_points(1, 1, 9));
        assert!(fill.contains(&(5, 5)));
    }

    #[test]
    fn ellipse_stroke_is_hollow_and_connected() {
        for &(width, height) in &[(9, 9), (16, 10), (7, 4), (4, 12), (2, 2), (1, 8)] {
            let fill = unique(&ellipse_fill_points(1, 2, width, height));
            let stroke = unique(&ellipse_stroke_points(1, 2, width, height));
            assert_stroke_outline(&fill, &stroke);
            if width >= 8 && height >= 8 {
                assert!(stroke.len() * 2 < fill.len());
            }
        }
    }

    /// Cells whose centers lie in the L1 diamond `|di|*ry + |dj|*rx <= rx*ry`.
    /// Independent of [`diamond_fill_points`].
    fn oracle_diamond(cx: i32, cy: i32, rx: i32, ry: i32) -> HashSet<(i32, i32)> {
        let mut cells = HashSet::new();
        let limit = i64::from(rx) * i64::from(ry);
        for y in (cy - ry)..=(cy + ry) {
            for x in (cx - rx)..=(cx + rx) {
                let di = i64::from(x - cx).abs();
                let dj = i64::from(y - cy).abs();
                if di * i64::from(ry) + dj * i64::from(rx) <= limit {
                    cells.insert((x, y));
                }
            }
        }
        cells
    }

    fn assert_diamond_extent(fill: &HashSet<(i32, i32)>, cx: i32, cy: i32, rx: i32, ry: i32) {
        assert_eq!(fill.iter().map(|p| p.0).min(), Some(cx - rx));
        assert_eq!(fill.iter().map(|p| p.0).max(), Some(cx + rx));
        assert_eq!(fill.iter().map(|p| p.1).min(), Some(cy - ry));
        assert_eq!(fill.iter().map(|p| p.1).max(), Some(cy + ry));
        assert!(fill.iter().all(|&(x, y)| {
            (cx - rx..=cx + rx).contains(&x) && (cy - ry..=cy + ry).contains(&y)
        }));
        assert!(fill.contains(&(cx + rx, cy)), "missing right tip");
        assert!(fill.contains(&(cx - rx, cy)), "missing left tip");
        assert!(fill.contains(&(cx, cy + ry)), "missing bottom tip");
        assert!(fill.contains(&(cx, cy - ry)), "missing top tip");
    }

    fn assert_diamond_symmetry(fill: &HashSet<(i32, i32)>, cx: i32, cy: i32) {
        for &(x, y) in fill {
            let di = x - cx;
            let dj = y - cy;
            for (sx, sy) in [(di, dj), (-di, dj), (di, -dj), (-di, -dj)] {
                assert!(
                    fill.contains(&(cx + sx, cy + sy)),
                    "symmetry broke at ({x},{y}) -> ({},{})",
                    cx + sx,
                    cy + sy
                );
            }
        }
    }

    #[test]
    fn diamond_matches_cell_center_oracle() {
        for rx in 0..=32 {
            for ry in 0..=32 {
                let fill = unique(&diamond_fill_points(0, 0, rx, ry));
                assert_eq!(
                    fill,
                    oracle_diamond(0, 0, rx, ry),
                    "rx={rx} ry={ry} drifted from the cell-center predicate"
                );
                assert_diamond_extent(&fill, 0, 0, rx, ry);
                assert_diamond_symmetry(&fill, 0, 0);
                let turned = unique(&diamond_fill_points(0, 0, ry, rx));
                let transpose: HashSet<(i32, i32)> = turned.iter().map(|&(x, y)| (y, x)).collect();
                assert_eq!(fill, transpose, "rx={rx} ry={ry} is not the transpose");
                if rx == ry {
                    assert_eq!(fill, turned, "rx={rx} is not a square diamond");
                    if rx >= 1 {
                        assert!(
                            !fill.contains(&(rx, ry)),
                            "rx={rx} filled the bounding-box corner"
                        );
                    }
                    for y in -rx..=rx {
                        for x in -rx..=rx {
                            assert_eq!(
                                fill.contains(&(x, y)),
                                x.abs() + y.abs() <= rx,
                                "square diamond ({x},{y}) rx={rx}"
                            );
                        }
                    }
                }
                if rx == 0 && ry == 0 {
                    assert_eq!(fill, HashSet::from([(0, 0)]));
                } else if rx == 0 {
                    assert!(
                        fill.iter().all(|&(x, _)| x == 0),
                        "rx=0 is not a vertical bar"
                    );
                    assert_eq!(fill.len(), (2 * ry + 1) as usize);
                } else if ry == 0 {
                    assert!(
                        fill.iter().all(|&(_, y)| y == 0),
                        "ry=0 is not a horizontal bar"
                    );
                    assert_eq!(fill.len(), (2 * rx + 1) as usize);
                }
            }
        }
        for &(cx, cy, rx, ry) in &[
            (5, 7, 9, 4),
            (1, 0, 0, 0),
            (4, 4, 0, 6),
            (3, 8, 11, 0),
            (-3, 2, 15, 6),
            (0, 0, 255, 255),
            (0, 0, 255, 0),
            (0, 0, 0, 255),
            (2, 3, 255, 1),
            (1, 1, 128, 64),
            (8, 8, 64, 128),
        ] {
            let fill = unique(&diamond_fill_points(cx, cy, rx, ry));
            assert_eq!(
                fill,
                oracle_diamond(cx, cy, rx, ry),
                "center ({cx},{cy}) rx={rx} ry={ry}"
            );
            assert_diamond_extent(&fill, cx, cy, rx, ry);
            assert_diamond_symmetry(&fill, cx, cy);
            let swapped = unique(&diamond_fill_points(cx, cy, ry, rx));
            let transpose: HashSet<(i32, i32)> = fill
                .iter()
                .map(|&(x, y)| (cx + (y - cy), cy + (x - cx)))
                .collect();
            assert_eq!(swapped, transpose, "transpose ({cx},{cy}) {rx},{ry}");
        }
    }

    #[test]
    fn diamond_stroke_is_the_outline_of_the_fill() {
        for rx in 0..=16 {
            for ry in 0..=16 {
                let fill = unique(&diamond_fill_points(2, 3, rx, ry));
                let stroke = unique(&diamond_stroke_points(2, 3, rx, ry));
                assert_stroke_outline(&fill, &stroke);
                if rx >= 4 && ry >= 4 {
                    assert!(
                        stroke.len() * 2 < fill.len(),
                        "rx={rx} ry={ry} stroke looks filled: {} vs {}",
                        stroke.len(),
                        fill.len()
                    );
                }
            }
        }
        for &(cx, cy, rx, ry) in &[(1, 1, 8, 5), (0, 0, 12, 12), (4, 2, 6, 20), (3, 3, 20, 7)] {
            let fill = unique(&diamond_fill_points(cx, cy, rx, ry));
            let stroke = unique(&diamond_stroke_points(cx, cy, rx, ry));
            assert_stroke_outline(&fill, &stroke);
            assert!(stroke.len() * 2 < fill.len());
        }
        // rx = ry = 2 centered at (4, 4): the center cell is interior.
        let stroke = unique(&diamond_stroke_points(4, 4, 2, 2));
        assert!(!stroke.contains(&(4, 4)));
        let fill = unique(&diamond_fill_points(4, 4, 2, 2));
        assert!(fill.contains(&(4, 4)));
    }

    #[test]
    fn triangle_stroke_has_edges_without_filling() {
        let stroke = unique(&triangle_stroke_points(2, 2, 12, 2, 2, 12));
        assert!(stroke.contains(&(2, 2)));
        assert!(stroke.contains(&(12, 2)));
        assert!(stroke.contains(&(2, 12)));
        assert!(stroke.contains(&(2, 7)), "left edge broken");
        assert!(stroke.contains(&(7, 2)), "top edge broken");
        assert!(!stroke.contains(&(4, 4)), "stroke filled interior");
        assert!(is_8_connected_region(&stroke), "triangle outline has gaps");

        let fill = unique(&triangle_fill_points(2, 2, 12, 2, 2, 12));
        assert!(fill.contains(&(3, 3)), "fill missing interior");
        assert!(fill.contains(&(2, 2)));
        assert!(stroke.is_subset(&fill));
        assert!(fill.len() > stroke.len());
    }

    #[test]
    fn triangle_stroke_stays_inside_fill_on_awkward_shapes() {
        // Bresenham edges can fall outside the barycentric fill on acute and
        // flat triangles; the stroke must still be the fill's contour.
        let cases = [
            (2, 2, 12, 2, 2, 12),
            (10, 1, 2, 12, 18, 12),
            (10, 1, 8, 14, 12, 14),
            (2, 8, 16, 8, 9, 9),
            (3, 2, 14, 6, 5, 13),
            (5, 5, 7, 5, 6, 7),
            (5, 5, 15, 5, 10, 6),
            (2, 2, 2, 12, 12, 2),
            // Thin tips used to rasterize as separate islands, so the
            // outline stroke broke between the base and the tip.
            (0, 0, 0, 1, 1, 3),
            (0, 0, 0, 1, 2, 6),
            (1, 0, 8, 1, 4, 2),
        ];
        for &(x0, y0, x1, y1, x2, y2) in &cases {
            let fill = unique(&triangle_fill_points(x0, y0, x1, y1, x2, y2));
            let stroke = unique(&triangle_stroke_points(x0, y0, x1, y1, x2, y2));
            for vertex in [(x0, y0), (x1, y1), (x2, y2)] {
                assert!(fill.contains(&vertex), "fill missing {vertex:?}");
                assert!(stroke.contains(&vertex), "stroke missing {vertex:?}");
            }
            assert!(
                stroke.is_subset(&fill),
                "stroke outside fill for ({x0},{y0})-({x1},{y1})-({x2},{y2}): {:?}",
                stroke.difference(&fill).collect::<Vec<_>>()
            );
            assert!(is_8_connected_region(&fill), "fill has gaps");
            assert!(is_8_connected_region(&stroke), "stroke has gaps");
        }
    }

    #[test]
    fn every_small_triangle_fill_and_stroke_is_connected() {
        for x0 in 0..6 {
            for y0 in 0..6 {
                for x1 in 0..6 {
                    for y1 in 0..6 {
                        for x2 in 0..6 {
                            for y2 in 0..6 {
                                let fill = unique(&triangle_fill_points(x0, y0, x1, y1, x2, y2));
                                let stroke =
                                    unique(&triangle_stroke_points(x0, y0, x1, y1, x2, y2));
                                for vertex in [(x0, y0), (x1, y1), (x2, y2)] {
                                    assert!(
                                        fill.contains(&vertex),
                                        "fill dropped {vertex:?} ({x0},{y0})-({x1},{y1})-({x2},{y2})"
                                    );
                                    assert!(
                                        stroke.contains(&vertex),
                                        "stroke dropped {vertex:?} ({x0},{y0})-({x1},{y1})-({x2},{y2})"
                                    );
                                }
                                assert!(
                                    stroke.is_subset(&fill) && is_8_connected_region(&stroke),
                                    "broken stroke ({x0},{y0})-({x1},{y1})-({x2},{y2})"
                                );
                                assert!(
                                    is_8_connected_region(&fill),
                                    "broken fill ({x0},{y0})-({x1},{y1})-({x2},{y2})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn curves_stay_gap_free_from_start_to_end() {
        let quad = quadratic_bezier_points(0, 12, 8, 0, 16, 12);
        assert!(is_8_connected_path(&quad), "quadratic Bezier has a gap");
        assert!(quad.contains(&(0, 12)) && quad.contains(&(16, 12)));

        let cubic = cubic_bezier_points((0, 16), (0, 0), (16, 0), (16, 16));
        assert!(is_8_connected_path(&cubic), "cubic Bezier has a gap");
        assert!(cubic.contains(&(0, 16)) && cubic.contains(&(16, 16)));

        let spline = catmull_rom_points(&[(0, 16), (4, 0), (12, 16), (16, 4)]);
        assert!(is_8_connected_path(&spline), "Catmull-Rom has a gap");
        assert!(spline.contains(&(0, 16)));
        assert!(spline.contains(&(16, 4)));

        // Curves must not paint a solid blob (no unnecessary bulk).
        let area = 17 * 17;
        assert!(unique(&quad).len() * 2 < area);
        assert!(unique(&cubic).len() * 2 < area);
        assert!(unique(&spline).len() * 2 < area);
    }
}
