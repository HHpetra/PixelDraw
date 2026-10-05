//! Rasterize pixel-art primitives to center-line / interior points.
//! Coordinates are signed so thick brushes can expand past the origin;
//! the canvas clips when stamping.

pub fn line_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let mut points = Vec::new();
    let mut x = x0;
    let mut y = y0;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        points.push((x, y));
        if x == x1 && y == y1 {
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

pub fn circle_stroke_points(cx: i32, cy: i32, r: i32) -> Vec<(i32, i32)> {
    if r <= 0 {
        return vec![(cx, cy)];
    }
    let mut points = Vec::new();
    let mut x = 0;
    let mut y = r;
    let mut d = 1 - r;
    while x <= y {
        points.extend([
            (cx + x, cy + y),
            (cx - x, cy + y),
            (cx + x, cy - y),
            (cx - x, cy - y),
            (cx + y, cy + x),
            (cx - y, cy + x),
            (cx + y, cy - x),
            (cx - y, cy - x),
        ]);
        if d < 0 {
            d += 2 * x + 3;
        } else {
            d += 2 * (x - y) + 5;
            y -= 1;
        }
        x += 1;
    }
    points
}

pub fn circle_fill_points(cx: i32, cy: i32, r: i32) -> Vec<(i32, i32)> {
    if r <= 0 {
        return vec![(cx, cy)];
    }
    let mut points = Vec::new();
    let rr = (r as i64) * (r as i64);
    for dy in -r..=r {
        for dx in -r..=r {
            if (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64) <= rr {
                points.push((cx + dx, cy + dy));
            }
        }
    }
    points
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
    let mut points = Vec::new();
    // Walk x along the implicit ellipse equation and emit the nearest integer y.
    let mut x = -rx;
    let rx2 = (rx as i64) * (rx as i64);
    let ry2 = (ry as i64) * (ry as i64);
    while x <= rx {
        let yy = ((ry2 * (rx2 - (x as i64) * (x as i64))) as f64 / rx2 as f64).sqrt();
        let y_up = (yy.round() as i32).clamp(0, ry);
        points.push((cx + x, cy + y_up));
        points.push((cx + x, cy - y_up));
        x += 1;
    }
    let mut y = -ry;
    while y <= ry {
        let xx = ((rx2 * (ry2 - (y as i64) * (y as i64))) as f64 / ry2 as f64).sqrt();
        let x_right = (xx.round() as i32).clamp(0, rx);
        points.push((cx + x_right, cy + y));
        points.push((cx - x_right, cy + y));
        y += 1;
    }
    points
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
        return triangle_stroke_points(x0, y0, x1, y1, x2, y2);
    }
    let left = x0.min(x1).min(x2);
    let right = x0.max(x1).max(x2);
    let top = y0.min(y1).min(y2);
    let bottom = y0.max(y1).max(y2);
    let mut points = Vec::new();
    for y in top..=bottom {
        for x in left..=right {
            let w0 = edge(x1, y1, x2, y2, x, y);
            let w1 = edge(x2, y2, x0, y0, x, y);
            let w2 = edge(x0, y0, x1, y1, x, y);
            let same_sign = (w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0);
            if same_sign {
                points.push((x, y));
            }
        }
    }
    points
}

pub fn ellipse_fill_points(cx: i32, cy: i32, rx: i32, ry: i32) -> Vec<(i32, i32)> {
    if rx <= 0 && ry <= 0 {
        return vec![(cx, cy)];
    }
    let mut points = Vec::new();
    let rx = rx.max(0);
    let ry = ry.max(0);
    let rx2 = (rx as i64) * (rx as i64);
    let ry2 = (ry as i64) * (ry as i64);
    let denom = (rx2 * ry2).max(1);
    for dy in -ry..=ry {
        for dx in -rx..=rx {
            let lhs = (dx as i64) * (dx as i64) * ry2 + (dy as i64) * (dy as i64) * rx2;
            if lhs <= denom {
                points.push((cx + dx, cy + dy));
            }
        }
    }
    points
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
        x0 as f64,
        y0 as f64,
        cx as f64,
        cy as f64,
        x1 as f64,
        y1 as f64,
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

    fn sorted(mut points: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
        points.sort_unstable();
        points.dedup();
        points
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
}
