//! Client-side compute for the web app, run in a Web Worker so it never blocks the main
//! thread (spec §2.1, §3 rule 1). The gateway already aggregates and caps what it sends; this
//! handles what remains on the client:
//!
//! - [`lttb`]: Largest-Triangle-Three-Buckets downsampling of a series to what the chart can
//!   show at its current zoom, so line charts stay at 60 fps however many points they hold.
//! - [`PointIndex`]: a uniform grid over a scatter plot's points for nearest-point hover and
//!   brush counts over a million points in well under a frame.
//! - [`histogram`]: equal-width bins of values already in the browser.
//!
//! The algorithms are plain Rust over slices (tested natively); the `wasm_bindgen` wrappers
//! only move typed arrays across.

use wasm_bindgen::prelude::*;

/// Indices of the points LTTB keeps out of `xs`/`ys`, at most `threshold` of them, always
/// including the first and last. Returns every index when the series is already small enough.
#[wasm_bindgen]
pub fn lttb(xs: &[f64], ys: &[f64], threshold: usize) -> Vec<u32> {
    lttb_indices(xs, ys, threshold)
}

/// [`lttb`] over the window `start..end` (a zoomed view); indices refer to the full series.
#[wasm_bindgen]
pub fn lttb_range(xs: &[f64], ys: &[f64], start: usize, end: usize, threshold: usize) -> Vec<u32> {
    let n = xs.len().min(ys.len());
    let (start, end) = (start.min(n), end.min(n));
    if start >= end {
        return Vec::new();
    }
    let offset = u32::try_from(start).unwrap_or(u32::MAX);
    lttb_indices(&xs[start..end], &ys[start..end], threshold)
        .into_iter()
        .map(|i| i + offset)
        .collect()
}

pub fn lttb_indices(xs: &[f64], ys: &[f64], threshold: usize) -> Vec<u32> {
    let n = xs.len().min(ys.len());
    let all = || {
        (0..n)
            .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
            .collect()
    };
    if threshold >= n || threshold < 3 {
        return all();
    }
    let every = (n - 2) as f64 / (threshold - 2) as f64;
    let mut out = Vec::with_capacity(threshold);
    out.push(0u32);
    let mut a = 0usize;
    for i in 0..threshold - 2 {
        // Average of the next bucket: the third corner of the triangle.
        let next_start = ((i + 1) as f64 * every) as usize + 1;
        let next_end = (((i + 2) as f64 * every) as usize + 1).min(n);
        let len = (next_end - next_start).max(1) as f64;
        let (mut avg_x, mut avg_y) = (0.0, 0.0);
        for j in next_start..next_end {
            avg_x += xs[j];
            avg_y += ys[j];
        }
        avg_x /= len;
        avg_y /= len;

        // The point of this bucket forming the largest triangle with `a` and the average.
        let start = (i as f64 * every) as usize + 1;
        let end = (((i + 1) as f64 * every) as usize + 1).min(n - 1);
        let (ax, ay) = (xs[a], ys[a]);
        let mut best = start;
        let mut best_area = -1.0;
        for j in start..end {
            let area = ((ax - avg_x) * (ys[j] - ay) - (ax - xs[j]) * (avg_y - ay)).abs();
            if area > best_area {
                best_area = area;
                best = j;
            }
        }
        out.push(u32::try_from(best).unwrap_or(u32::MAX));
        a = best;
    }
    out.push(u32::try_from(n - 1).unwrap_or(u32::MAX));
    out
}

/// Counts of `values` in `bins` equal-width bins over `[min, max]`; values outside the range
/// or not finite are ignored, and `max` itself falls in the last bin.
#[wasm_bindgen]
pub fn histogram(values: &[f64], bins: u32, min: f64, max: f64) -> Vec<u32> {
    let mut counts = vec![0u32; bins as usize];
    // Written so NaN bounds also bail out.
    if bins == 0 || max.partial_cmp(&min) != Some(std::cmp::Ordering::Greater) {
        return counts;
    }
    let scale = f64::from(bins) / (max - min);
    for &v in values {
        if v.is_finite() && v >= min && v <= max {
            let b = (((v - min) * scale) as usize).min(bins as usize - 1);
            counts[b] += 1;
        }
    }
    counts
}

/// A uniform grid over 2-D points: each cell lists the points inside it, so a lookup scans
/// only the cells under the search window.
#[wasm_bindgen]
pub struct PointIndex {
    xs: Vec<f32>,
    ys: Vec<f32>,
    min_x: f32,
    min_y: f32,
    cell_w: f32,
    cell_h: f32,
    nx: usize,
    ny: usize,
    /// `starts[c]..starts[c + 1]` is cell `c`'s slice of `order`.
    starts: Vec<u32>,
    order: Vec<u32>,
}

#[wasm_bindgen]
impl PointIndex {
    /// Indexes the points; non-finite ones are left out of every lookup.
    #[wasm_bindgen(constructor)]
    pub fn new(xs: &[f32], ys: &[f32]) -> PointIndex {
        let n = xs.len().min(ys.len());
        let (xs, ys) = (xs[..n].to_vec(), ys[..n].to_vec());
        let finite = |i: usize| xs[i].is_finite() && ys[i].is_finite();
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for i in (0..n).filter(|&i| finite(i)) {
            min_x = min_x.min(xs[i]);
            max_x = max_x.max(xs[i]);
            min_y = min_y.min(ys[i]);
            max_y = max_y.max(ys[i]);
        }
        if min_x > max_x {
            (min_x, max_x, min_y, max_y) = (0.0, 1.0, 0.0, 1.0);
        }
        // About four points per cell on average, up to 1024 × 1024 cells.
        let side = (((n / 4) as f64).sqrt() as usize).clamp(1, 1024);
        let span = |lo: f32, hi: f32| {
            if hi > lo {
                (hi - lo) / side as f32
            } else {
                1.0
            }
        };
        let mut index = PointIndex {
            min_x,
            min_y,
            cell_w: span(min_x, max_x),
            cell_h: span(min_y, max_y),
            nx: side,
            ny: side,
            starts: vec![0; side * side + 1],
            order: Vec::with_capacity(n),
            xs: Vec::new(),
            ys: Vec::new(),
        };
        // Counting sort of point ids by cell.
        let cells: Vec<Option<usize>> = (0..n)
            .map(|i| finite(i).then(|| index.cell(xs[i], ys[i])))
            .collect();
        for c in cells.iter().flatten() {
            index.starts[c + 1] += 1;
        }
        for c in 0..side * side {
            index.starts[c + 1] += index.starts[c];
        }
        let mut cursor = index.starts.clone();
        index.order = vec![0; index.starts[side * side] as usize];
        for (i, c) in cells.iter().enumerate() {
            if let Some(c) = c {
                index.order[cursor[*c] as usize] = u32::try_from(i).unwrap_or(u32::MAX);
                cursor[*c] += 1;
            }
        }
        index.xs = xs;
        index.ys = ys;
        index
    }

    /// Indexed points.
    pub fn len(&self) -> u32 {
        u32::try_from(self.order.len()).unwrap_or(u32::MAX)
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The point closest to `(x, y)` within the ellipse of radii `rx`, `ry` (data units, so a
    /// pixel radius works on axes of any scale), or -1.
    pub fn nearest(&self, x: f32, y: f32, rx: f32, ry: f32) -> i32 {
        if rx.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            || ry.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        {
            return -1;
        }
        let mut best = -1i32;
        let mut best_d = 1.0f32;
        self.scan(x - rx, y - ry, x + rx, y + ry, |i| {
            let dx = (self.xs[i] - x) / rx;
            let dy = (self.ys[i] - y) / ry;
            let d = dx * dx + dy * dy;
            if d <= best_d {
                best_d = d;
                best = i32::try_from(i).unwrap_or(-1);
            }
        });
        best
    }

    /// Points inside the rectangle (a brush).
    pub fn count_in(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> u32 {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        let mut count = 0u32;
        self.scan(x0, y0, x1, y1, |i| {
            if (x0..=x1).contains(&self.xs[i]) && (y0..=y1).contains(&self.ys[i]) {
                count += 1;
            }
        });
        count
    }
}

impl PointIndex {
    fn cell_coords(&self, x: f32, y: f32) -> (usize, usize) {
        let cx = ((x - self.min_x) / self.cell_w).floor();
        let cy = ((y - self.min_y) / self.cell_h).floor();
        let clamp = |v: f32, n: usize| (v.max(0.0) as usize).min(n - 1);
        (clamp(cx, self.nx), clamp(cy, self.ny))
    }

    fn cell(&self, x: f32, y: f32) -> usize {
        let (cx, cy) = self.cell_coords(x, y);
        cy * self.nx + cx
    }

    /// Calls `visit` with every point id in the cells overlapping the rectangle.
    fn scan(&self, x0: f32, y0: f32, x1: f32, y1: f32, mut visit: impl FnMut(usize)) {
        let (cx0, cy0) = self.cell_coords(x0, y0);
        let (cx1, cy1) = self.cell_coords(x1, y1);
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                let c = cy * self.nx + cx;
                for &i in &self.order[self.starts[c] as usize..self.starts[c + 1] as usize] {
                    visit(i as usize);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random numbers in [0, 1).
    fn noise(n: usize, seed: u64) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 40) as f32) / (1u64 << 24) as f32
            })
            .collect()
    }

    #[test]
    fn lttb_keeps_the_ends_the_count_and_the_peaks() {
        let n = 10_000;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let mut ys = vec![0.0; n];
        ys[4_321] = 100.0;
        ys[7_777] = -50.0;
        let kept = lttb_indices(&xs, &ys, 200);
        assert_eq!(kept.len(), 200);
        assert_eq!((kept[0], kept[199]), (0, (n - 1) as u32));
        assert!(kept.windows(2).all(|w| w[0] < w[1]), "ordered and unique");
        assert!(
            kept.contains(&4_321) && kept.contains(&7_777),
            "spikes survive"
        );
        assert_eq!(
            lttb_indices(&xs[..50], &ys[..50], 200).len(),
            50,
            "small series pass through"
        );
    }

    #[test]
    fn lttb_range_offsets_into_the_full_series() {
        let xs: Vec<f64> = (0..1000).map(f64::from).collect();
        let ys: Vec<f64> = xs.iter().map(|x| (x / 10.0).sin()).collect();
        let kept = lttb_range(&xs, &ys, 500, 900, 50);
        assert_eq!(kept.len(), 50);
        assert_eq!((kept[0], kept[49]), (500, 899));
        assert!(lttb_range(&xs, &ys, 900, 500, 50).is_empty());
    }

    #[test]
    fn histogram_counts_every_value_in_range() {
        let values = [0.0, 0.5, 1.0, 9.99, 10.0, -1.0, f64::NAN, 11.0];
        assert_eq!(
            histogram(&values, 10, 0.0, 10.0),
            vec![2, 1, 0, 0, 0, 0, 0, 0, 0, 2]
        );
        assert_eq!(histogram(&values, 4, 5.0, 5.0), vec![0; 4], "empty range");
    }

    #[test]
    fn nearest_and_brush_agree_with_brute_force() {
        let n = 50_000;
        let xs: Vec<f32> = noise(n, 1).iter().map(|v| v * 1000.0).collect();
        let ys: Vec<f32> = noise(n, 2).iter().map(|v| v * 3.0).collect();
        let index = PointIndex::new(&xs, &ys);
        assert_eq!(index.len() as usize, n);

        let (rx, ry) = (8.0, 0.02);
        for (qx, qy) in noise(200, 3).iter().zip(noise(200, 4).iter()) {
            let (x, y) = (qx * 1000.0, qy * 3.0);
            let brute = (0..n)
                .map(|i| (i, ((xs[i] - x) / rx).powi(2) + ((ys[i] - y) / ry).powi(2)))
                .filter(|(_, d)| *d <= 1.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(-1, |(i, _)| i as i32);
            let found = index.nearest(x, y, rx, ry);
            if brute == -1 {
                assert_eq!(found, -1);
            } else {
                // Ties may pick either point; the distance must match.
                let d = |i: i32| {
                    ((xs[i as usize] - x) / rx).powi(2) + ((ys[i as usize] - y) / ry).powi(2)
                };
                assert!((d(found) - d(brute)).abs() < 1e-6);
            }
        }

        let brute = (0..n)
            .filter(|&i| (200.0..=450.0).contains(&xs[i]) && (1.0..=2.5).contains(&ys[i]))
            .count() as u32;
        assert_eq!(index.count_in(450.0, 2.5, 200.0, 1.0), brute);
        assert_eq!(index.count_in(-1e9, -1e9, 1e9, 1e9), n as u32);
    }

    #[test]
    fn non_finite_points_are_ignored() {
        let index = PointIndex::new(&[1.0, f32::NAN, 3.0], &[1.0, 2.0, f32::INFINITY]);
        assert_eq!(index.len(), 1);
        assert_eq!(index.nearest(1.0, 1.0, 0.5, 0.5), 0);
        let empty = PointIndex::new(&[], &[]);
        assert!(empty.is_empty());
        assert_eq!(empty.nearest(0.0, 0.0, 1.0, 1.0), -1);
    }
}
