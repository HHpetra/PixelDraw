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

/// Fill cells from half-widths `w[dy] = max |dx|` on each row `|dy|`.
fn fill_from_half_widths(cx: i32, cy: i32, w: &[i32]) -> Vec<(i32, i32)> {
    let capacity: usize = w
        .iter()
        .enumerate()
        .map(|(dy, &half)| {
            let width = (2 * half + 1) as usize;
            if dy == 0 { width } else { width * 2 }
        })
        .sum();
    let mut points = Vec::with_capacity(capacity);
    for (dy, &half) in w.iter().enumerate() {
        let dy = dy as i32;
        for dx in -half..=half {
            points.push((cx + dx, cy + dy));
            if dy > 0 {
                points.push((cx + dx, cy - dy));
            }
        }
    }
    points
}

/// Largest integer `dx ≥ 0` with `dx² ≤ n / d` (real division; `d > 0`).
fn floor_sqrt_div(n: i64, d: i64) -> i64 {
    debug_assert!(d > 0);
    (n / d).isqrt()
}

/// Half-widths `w[0..=ry]` of a filled ellipse with semi-axes `rx`/`ry`.
///
/// Each row is the cover-rule silhouette clamped to at most 1px outside
/// the mathematical ellipse: small circles keep the classic bead outline
/// (r=2 → half-widths 2,2,1) and the true equator, while large or flat
/// shapes stay elliptical instead of collapsing into a cone or a box.
/// The pole may therefore step by more than 1 on large radii — that is
/// the circle tapering, not a quantization glitch.
fn ellipse_half_widths(rx: i32, ry: i32) -> Vec<i32> {
    let rx = rx.max(0) as i64;
    let ry = ry.max(0) as i64;
    if ry == 0 {
        return vec![rx as i32];
    }
    if rx == 0 {
        return vec![0; (ry + 1) as usize];
    }
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    // Cover test with half-pixel axes: (dx/(rx+.5))² + (dy/(ry+.5))² ≤ 1,
    // cleared of fractions via ×16.
    let two_rx1 = 2 * rx + 1;
    let two_ry1 = 2 * ry + 1;
    let cover_limit = two_rx1 * two_rx1 * two_ry1 * two_ry1;
    let cover_a = 4 * two_ry1 * two_ry1;
    let cover_b = 4 * two_rx1 * two_rx1;
    let mut w = vec![0i32; (ry + 1) as usize];
    for dy in 0..=ry {
        let dy2 = dy * dy;
        // Center inside the true ellipse: dx²·ry² + dy²·rx² ≤ rx²·ry².
        let analytic_half = floor_sqrt_div(rx2 * (ry2 - dy2), ry2);
        // Center inside the half-pixel-grown ellipse: cover_a·dx² + cover_b·dy² ≤ cover_limit.
        let cover_half = floor_sqrt_div(cover_limit - cover_b * dy2, cover_a);
        w[dy as usize] = cover_half.min(analytic_half + 1) as i32;
    }
    w
}

/// Outline of the filled disk: thin ring, no gaps, matches fill radius.
pub fn circle_stroke_points(cx: i32, cy: i32, r: i32) -> Vec<(i32, i32)> {
    ellipse_stroke_points(cx, cy, r, r)
}

pub fn circle_fill_points(cx: i32, cy: i32, r: i32) -> Vec<(i32, i32)> {
    ellipse_fill_points(cx, cy, r, r)
}

pub fn ellipse_stroke_points(cx: i32, cy: i32, rx: i32, ry: i32) -> Vec<(i32, i32)> {
    if rx <= 0 && ry <= 0 {
        return vec![(cx, cy)];
    }
    if rx <= 0 {
        return line_points(cx, cy - ry, cx, cy + ry);
    }
    if ry <= 0 {
        return line_points(cx - rx, cy, cx + rx, cy);
    }
    // Same contour approach as the circle: keeps the ring gap-free and
    // on the same boundary as `ellipse_fill_points`.
    outline_cells(&ellipse_fill_points(cx, cy, rx, ry))
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

pub fn ellipse_fill_points(cx: i32, cy: i32, rx: i32, ry: i32) -> Vec<(i32, i32)> {
    if rx <= 0 && ry <= 0 {
        return vec![(cx, cy)];
    }
    if rx <= 0 {
        return line_points(cx, cy - ry.max(0), cx, cy + ry.max(0));
    }
    if ry <= 0 {
        return line_points(cx - rx, cy, cx + rx, cy);
    }
    fill_from_half_widths(cx, cy, &ellipse_half_widths(rx, ry))
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

    /// Largest filled `x` on row `y` within `0..=max_x` (row half-width).
    fn row_half_width(fill: &HashSet<(i32, i32)>, y: i32, max_x: i32) -> i32 {
        (0..=max_x)
            .filter(|&x| fill.contains(&(x, y)))
            .max()
            .expect("row empty")
    }

    /// Mathematical half-width `floor(sqrt(r² − y²))` of a disk.
    fn true_circle_half(r: i32, y: i32) -> i32 {
        (((r * r - y * y) as f64).sqrt().floor() as i32).max(0)
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

    #[test]
    fn circle_fill_contains_center_and_outside_empty() {
        let fill = sorted(circle_fill_points(5, 5, 2));
        assert!(fill.contains(&(5, 5)));
        assert!(fill.contains(&(5, 7)));
        assert!(!fill.contains(&(5, 8)));
        assert_eq!(circle_fill_points(3, 3, 0), vec![(3, 3)]);
    }

    #[test]
    fn circle_silhouette_is_round_not_conical() {
        // Classic small bead circles stay smooth; every size keeps the true
        // equator, a monotone silhouette, and a half-width within 1px of the
        // mathematical disk (so large radii cannot turn into a cone).
        for r in 2..=24 {
            let fill = unique(&circle_fill_points(0, 0, r));
            let mut prev_half = i32::MAX;
            for y in 0..=r {
                let half = row_half_width(&fill, y, r);
                assert!(
                    half <= prev_half,
                    "r={r} y={y}: silhouette grew {prev_half} -> {half}"
                );
                let true_half = true_circle_half(r, y);
                assert!(
                    (half - true_half).abs() <= 1,
                    "r={r} y={y}: half={half} too far from true {true_half}"
                );
                prev_half = half;
            }
            // Equator reaches the full radius.
            assert_eq!(
                row_half_width(&fill, 0, r),
                r,
                "r={r} equator not full width"
            );
            // Small circles keep a blunt tip (no 1px spike).
            if (2..=8).contains(&r) {
                let top_half = row_half_width(&fill, r, r);
                assert!(top_half >= 1, "r={r} pole too pointy (half={top_half})");
            }
        }
    }

    #[test]
    fn large_circle_is_not_a_cone() {
        // A global "drop at most 1 per row" floor would leave a huge blunt
        // tip on large disks (r=256 → half-width ~100). The fill must stay
        // within 1px of the true circle instead.
        for r in [32, 64, 128] {
            let fill = unique(&circle_fill_points(0, 0, r));
            let tip = row_half_width(&fill, r, r);
            assert!(tip <= 2, "r={r} tip half={tip} is conical, not circular");
            let mid = row_half_width(&fill, r / 2, r);
            let true_half = true_circle_half(r, r / 2);
            assert!(
                (mid - true_half).abs() <= 1,
                "r={r} at y={}: half={mid} vs true {true_half}",
                r / 2
            );
        }
    }

    #[test]
    fn flat_ellipse_is_not_a_rectangle() {
        // A wide flat ellipse must stay a thin lens, not a filled box.
        for &(rx, ry) in &[(20, 1), (20, 2), (15, 4), (7, 4)] {
            let fill = unique(&ellipse_fill_points(0, 0, rx, ry));
            let mid = row_half_width(&fill, 0, rx);
            assert_eq!(mid, rx, "{rx}x{ry} equator not full width");
            let top = row_half_width(&fill, ry, rx);
            // True ellipse is a single point at the pole (half=0); allow
            // the 1px bead-art tip at most.
            assert!(
                top <= 1,
                "{rx}x{ry} top half={top} too wide (true tip is 0/1)"
            );
            // Not a box: the top row is much narrower than the equator.
            assert!(top < rx / 2 + 1, "{rx}x{ry} looks rectangular (top={top})");
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
        let fill = sorted(ellipse_fill_points(4, 4, 3, 1));
        assert!(fill.contains(&(4, 4)));
        assert!(fill.contains(&(1, 4)));
        assert!(fill.contains(&(7, 4)));
        assert!(fill.contains(&(4, 5)));
        assert!(!fill.contains(&(4, 6)));
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

    #[test]
    fn circle_stroke_is_a_thin_ring_not_a_disk() {
        let r = 5;
        let stroke = unique(&circle_stroke_points(8, 8, r));
        assert!(!stroke.contains(&(8, 8)), "stroke filled the center");
        assert!(!stroke.contains(&(8, 9)), "stroke filled near-center");
        assert!(!stroke.contains(&(8, 10)), "stroke filled interior");
        assert!(is_8_connected_region(&stroke), "ring has gaps");
        // Thin ring: circumference order, not a disk.
        let fill = unique(&circle_fill_points(8, 8, r));
        assert!(fill.contains(&(8, 8)));
        assert!(
            stroke.len() * 2 < fill.len(),
            "stroke looks filled: {} vs {}",
            stroke.len(),
            fill.len()
        );
        assert!(
            (4 * r as usize..=12 * r as usize).contains(&stroke.len()),
            "ring unnaturally sparse or thick: {}",
            stroke.len()
        );
        // Every stroke cell sits on a ~1px annulus around radius r
        // (midpoint stroke and analytic fill may differ on the boundary pixel).
        for &(x, y) in &stroke {
            let dist2 = (x - 8) as i64 * (x - 8) as i64 + (y - 8) as i64 * (y - 8) as i64;
            let lo = (r as i64 - 1) * (r as i64 - 1);
            let hi = (r as i64 + 1) * (r as i64 + 1);
            assert!(
                (lo..=hi).contains(&dist2),
                "stroke cell ({x},{y}) is off the rim (dist²={dist2})"
            );
        }
        // Stroke is the contour of the same fill: never outside the disk.
        assert!(
            stroke.is_subset(&fill),
            "stroke cells outside fill: {:?}",
            stroke.difference(&fill).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ellipse_stroke_is_hollow_and_connected() {
        let (cx, cy, rx, ry) = (10, 8, 7, 4);
        let stroke = unique(&ellipse_stroke_points(cx, cy, rx, ry));
        assert!(!stroke.contains(&(cx, cy)), "ellipse stroke filled center");
        assert!(
            !stroke.contains(&(cx, cy + 1)),
            "ellipse stroke filled interior"
        );
        assert!(is_8_connected_region(&stroke), "ellipse ring has gaps");
        let fill = unique(&ellipse_fill_points(cx, cy, rx, ry));
        assert!(fill.contains(&(cx, cy)));
        assert!(stroke.len() * 2 < fill.len());
        assert!(
            stroke.is_subset(&fill),
            "ellipse stroke cells outside fill: {:?}",
            stroke.difference(&fill).collect::<Vec<_>>()
        );
        // Rim cells stay near the ellipse boundary (normalized distance ≈ 1).
        for &(x, y) in &stroke {
            let dx = (x - cx) as f64 / rx as f64;
            let dy = (y - cy) as f64 / ry as f64;
            let n = (dx * dx + dy * dy).sqrt();
            assert!(
                (0.5..=1.5).contains(&n),
                "stroke cell ({x},{y}) is off the ellipse rim (norm={n:.2})"
            );
        }
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
