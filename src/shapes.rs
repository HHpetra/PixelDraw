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
    (bx as i64 - ax as i64) * (py as i64 - ay as i64) - (by as i64 - ay as i64) * (px as i64 - ax as i64)
}

pub fn triangle_stroke_points(x0: i32, y0: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> Vec<(i32, i32)> {
    let mut points = line_points(x0, y0, x1, y1);
    points.extend(line_points(x1, y1, x2, y2));
    points.extend(line_points(x2, y2, x0, y0));
    points
}

pub fn triangle_fill_points(x0: i32, y0: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> Vec<(i32, i32)> {
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
        assert_eq!(line_points(0, 0, 3, 0), vec![(0, 0), (1, 0), (2, 0), (3, 0)]);
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
}
