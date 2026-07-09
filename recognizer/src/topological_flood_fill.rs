//! Direct port of service/vendor/wha/src/parser/topologicalFloodFill.js.
//!
//! Closure is a topology test first, then a circle-quality test. Rasterize
//! the strokes as ink, flood-fill the outside empty space, and look for
//! unreachable dry cells. A ring is closed only when the dry area is large
//! enough and its outside ink edge is still circular enough to count as the
//! spell boundary.

use crate::config::RingConfig;
use crate::geometry::{bounds_for_point_lists, clamp01, distance, mean, stddev, Bounds, Point};
use crate::stroke_cleaner::CleanedStroke;
use std::collections::{HashSet, VecDeque};

const PADDING_PX: f64 = 18.0;
const STROKE_RADIUS_PX: f64 = 3.0;
const STROKE_SAMPLE_STEP_PX: f64 = 0.75;
const MIN_ENCLOSED_AREA_PX: f64 = 3500.0;
const MIN_ENCLOSED_AREA_RATIO: f64 = 0.08;
const MAX_NORMALIZED_RMSE: f64 = 0.18;
const MIN_PERFECTION: f64 = 0.28;
// CELL_SIZE_PX is always 1 in the JS (never configured otherwise) — inlined
// throughout instead of threaded as a parameter.

struct Raster {
    width: i64,
    height: i64,
    size: usize,
    offset_x: f64,
    offset_y: f64,
    blocked: Vec<u8>,
    water: Vec<u8>,
    outside_edge: Vec<u8>,
    stroke_ids_by_cell: Vec<Option<HashSet<String>>>,
}

fn grid_index(x: i64, y: i64, width: i64) -> usize {
    (y * width + x) as usize
}

fn cell_center(index: usize, raster: &Raster) -> Point {
    let x = index as i64 % raster.width;
    let y = index as i64 / raster.width;
    Point { x: raster.offset_x + (x as f64 + 0.5), y: raster.offset_y + (y as f64 + 0.5) }
}

fn create_raster(bounds: Bounds) -> Raster {
    let padding = PADDING_PX + STROKE_RADIUS_PX + 2.0;
    let offset_x = (bounds.min_x - padding).floor();
    let offset_y = (bounds.min_y - padding).floor();
    let max_x = (bounds.max_x + padding).ceil();
    let max_y = (bounds.max_y + padding).ceil();
    let width = (3i64).max((max_x - offset_x).ceil() as i64 + 1);
    let height = (3i64).max((max_y - offset_y).ceil() as i64 + 1);
    let size = (width * height) as usize;

    Raster {
        width,
        height,
        size,
        offset_x,
        offset_y,
        blocked: vec![0u8; size],
        water: vec![0u8; size],
        outside_edge: vec![0u8; size],
        stroke_ids_by_cell: vec![None; size],
    }
}

fn mark_blocked_cell(raster: &mut Raster, x: i64, y: i64, stroke_id: &str) {
    if x < 0 || y < 0 || x >= raster.width || y >= raster.height {
        return;
    }
    let index = grid_index(x, y, raster.width);
    raster.blocked[index] = 1;
    raster.stroke_ids_by_cell[index].get_or_insert_with(HashSet::new).insert(stroke_id.to_string());
}

fn mark_ink_disk(raster: &mut Raster, point: Point, radius_px: f64, stroke_id: &str) {
    let gx = point.x - raster.offset_x;
    let gy = point.y - raster.offset_y;
    let radius = radius_px;
    let min_x = (gx - radius).floor() as i64;
    let max_x = (gx + radius).ceil() as i64;
    let min_y = (gy - radius).floor() as i64;
    let max_y = (gy + radius).ceil() as i64;
    let radius_squared = radius * radius;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f64 + 0.5 - gx;
            let dy = y as f64 + 0.5 - gy;
            if dx * dx + dy * dy <= radius_squared {
                mark_blocked_cell(raster, x, y, stroke_id);
            }
        }
    }
}

fn rasterize_strokes(strokes: &[CleanedStroke], raster: &mut Raster) {
    for stroke in strokes {
        if stroke.points.is_empty() {
            continue;
        }
        mark_ink_disk(raster, stroke.points[0], STROKE_RADIUS_PX, &stroke.id);
        for index in 1..stroke.points.len() {
            let previous = stroke.points[index - 1];
            let current = stroke.points[index];
            let segment_length = distance(previous, current);
            let steps = (1i64).max((segment_length / STROKE_SAMPLE_STEP_PX).ceil() as i64);
            for step in 1..=steps {
                let t = step as f64 / steps as f64;
                let p = Point { x: previous.x + (current.x - previous.x) * t, y: previous.y + (current.y - previous.y) * t };
                mark_ink_disk(raster, p, STROKE_RADIUS_PX, &stroke.id);
            }
        }
    }
}

fn enqueue_water(index: usize, raster: &mut Raster, queue: &mut VecDeque<usize>) {
    if raster.blocked[index] != 0 || raster.water[index] != 0 {
        return;
    }
    raster.water[index] = 1;
    queue.push_back(index);
}

const DIRECTIONS: [(i64, i64); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

// Same idea as the bucket fill tool in MS Paint: start filling empty space
// from the outside border. Ink blocks the fill, so any empty cells left dry
// afterward are enclosed by the drawn boundary.
fn flood_exterior(raster: &mut Raster) {
    let mut queue: VecDeque<usize> = VecDeque::new();

    for x in 0..raster.width {
        enqueue_water(grid_index(x, 0, raster.width), raster, &mut queue);
        enqueue_water(grid_index(x, raster.height - 1, raster.width), raster, &mut queue);
    }
    for y in 1..(raster.height - 1) {
        enqueue_water(grid_index(0, y, raster.width), raster, &mut queue);
        enqueue_water(grid_index(raster.width - 1, y, raster.width), raster, &mut queue);
    }

    while let Some(index) = queue.pop_front() {
        let x = index as i64 % raster.width;
        let y = index as i64 / raster.width;

        for (dx, dy) in DIRECTIONS {
            let nx = x + dx;
            let ny = y + dy;
            if nx < 0 || ny < 0 || nx >= raster.width || ny >= raster.height {
                continue;
            }
            let neighbor = grid_index(nx, ny, raster.width);
            if raster.blocked[neighbor] != 0 {
                raster.outside_edge[neighbor] = 1;
            } else {
                enqueue_water(neighbor, raster, &mut queue);
            }
        }
    }
}

struct DryComponents {
    component_count: usize,
    largest: Vec<usize>,
}

fn find_dry_components(raster: &Raster) -> DryComponents {
    let mut visited = vec![0u8; raster.size];
    let mut component_count = 0usize;
    let mut largest: Vec<usize> = vec![];

    for start in 0..raster.size {
        if raster.blocked[start] != 0 || raster.water[start] != 0 || visited[start] != 0 {
            continue;
        }

        component_count += 1;
        let mut cells = vec![];
        let mut queue: VecDeque<usize> = VecDeque::new();
        visited[start] = 1;
        queue.push_back(start);

        while let Some(index) = queue.pop_front() {
            cells.push(index);
            let x = index as i64 % raster.width;
            let y = index as i64 / raster.width;

            for (dx, dy) in DIRECTIONS {
                let nx = x + dx;
                let ny = y + dy;
                if nx < 0 || ny < 0 || nx >= raster.width || ny >= raster.height {
                    continue;
                }
                let neighbor = grid_index(nx, ny, raster.width);
                if raster.blocked[neighbor] == 0 && raster.water[neighbor] == 0 && visited[neighbor] == 0 {
                    visited[neighbor] = 1;
                    queue.push_back(neighbor);
                }
            }
        }

        if cells.len() > largest.len() {
            largest = cells;
        }
    }

    DryComponents { component_count, largest }
}

struct OutsideEdge {
    edge_pixels: Vec<Point>,
    stroke_ids: Vec<String>,
}

fn collect_outside_edge(raster: &Raster) -> OutsideEdge {
    let mut edge_pixels = vec![];
    let mut stroke_ids: HashSet<String> = HashSet::new();

    for index in 0..raster.size {
        if raster.outside_edge[index] == 0 {
            continue;
        }
        edge_pixels.push(cell_center(index, raster));
        if let Some(ids) = &raster.stroke_ids_by_cell[index] {
            for id in ids {
                stroke_ids.insert(id.clone());
            }
        }
    }

    let mut stroke_ids: Vec<String> = stroke_ids.into_iter().collect();
    stroke_ids.sort(); // JS Set iteration is insertion-order; sort for determinism instead.
    OutsideEdge { edge_pixels, stroke_ids }
}

struct CircleScore {
    center: Point,
    radius: f64,
    rmse: f64,
    normalized_rmse: f64,
    perfection: f64,
}

fn score_circle(edge_pixels: &[Point]) -> CircleScore {
    if edge_pixels.len() < 8 {
        return CircleScore { center: Point { x: 0.0, y: 0.0 }, radius: 0.0, rmse: f64::INFINITY, normalized_rmse: f64::INFINITY, perfection: 0.0 };
    }

    let center = Point {
        x: mean(&edge_pixels.iter().map(|p| p.x).collect::<Vec<_>>()),
        y: mean(&edge_pixels.iter().map(|p| p.y).collect::<Vec<_>>()),
    };
    let distances: Vec<f64> = edge_pixels.iter().map(|p| distance(*p, center)).collect();
    let radius = mean(&distances);

    // root mean square of radial errors against the average radius. Dividing
    // it by radius makes the circle score scale-invariant across paper sizes.
    let rmse = stddev(&distances);
    let normalized_rmse = rmse / (1.0_f64).max(radius);
    let perfection = clamp01(1.0 - normalized_rmse / MAX_NORMALIZED_RMSE);

    CircleScore { center, radius, rmse, normalized_rmse, perfection }
}

fn count_cells(mask: &[u8]) -> usize {
    mask.iter().map(|&v| v as usize).sum()
}

#[derive(Debug, Clone)]
pub struct RasterStats {
    pub width: i64,
    pub height: i64,
    pub cell_size: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub blocked_pixel_count: usize,
    pub water_pixel_count: usize,
    pub dry_pixel_count: usize,
}

#[derive(Debug, Clone)]
pub struct ClosureResult {
    pub closed: bool,
    pub enclosed_area_px: f64,
    pub min_enclosed_area_px: f64,
    pub component_count: usize,
    pub center: Point,
    pub radius: f64,
    pub rmse: f64,
    pub normalized_rmse: f64,
    pub perfection: f64,
    pub edge_pixel_count: usize,
    pub edge_pixels: Vec<Point>,
    pub stroke_ids: Vec<String>,
    pub raster: RasterStats,
}

pub fn analyze_topological_closure(strokes: &[CleanedStroke], ring_config: &RingConfig) -> ClosureResult {
    if strokes.is_empty() {
        return ClosureResult {
            closed: false,
            enclosed_area_px: 0.0,
            min_enclosed_area_px: 0.0,
            component_count: 0,
            center: Point { x: 0.0, y: 0.0 },
            radius: 0.0,
            rmse: 0.0,
            normalized_rmse: 0.0,
            perfection: 0.0,
            edge_pixel_count: 0,
            edge_pixels: vec![],
            stroke_ids: vec![],
            raster: RasterStats { width: 0, height: 0, cell_size: 1.0, offset_x: 0.0, offset_y: 0.0, blocked_pixel_count: 0, water_pixel_count: 0, dry_pixel_count: 0 },
        };
    }

    let point_lists: Vec<Vec<Point>> = strokes.iter().map(|s| s.points.clone()).collect();
    let source_bounds = bounds_for_point_lists(&point_lists);
    let mut raster = create_raster(source_bounds);
    rasterize_strokes(strokes, &mut raster);
    flood_exterior(&mut raster);

    let dry = find_dry_components(&raster);

    // The largest empty area the outside fill could not reach (cellSize=1, so area == cell count).
    let enclosed_area_px = dry.largest.len() as f64;
    let bounds_area_px = (raster.width * raster.height) as f64;
    let min_area_px = MIN_ENCLOSED_AREA_PX.max(bounds_area_px * MIN_ENCLOSED_AREA_RATIO);
    let edge = collect_outside_edge(&raster);
    let circle = score_circle(&edge.edge_pixels);
    let closed = enclosed_area_px >= min_area_px && circle.radius >= ring_config.min_radius && circle.perfection >= MIN_PERFECTION;

    ClosureResult {
        closed,
        enclosed_area_px,
        min_enclosed_area_px: min_area_px,
        component_count: dry.component_count,
        center: circle.center,
        radius: circle.radius,
        rmse: circle.rmse,
        normalized_rmse: circle.normalized_rmse,
        perfection: circle.perfection,
        edge_pixel_count: edge.edge_pixels.len(),
        edge_pixels: edge.edge_pixels,
        stroke_ids: edge.stroke_ids,
        raster: RasterStats {
            width: raster.width,
            height: raster.height,
            cell_size: 1.0,
            offset_x: raster.offset_x,
            offset_y: raster.offset_y,
            blocked_pixel_count: count_cells(&raster.blocked),
            water_pixel_count: count_cells(&raster.water),
            dry_pixel_count: dry.largest.len(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{INPUT, RING};
    use crate::stroke_cleaner::{clean_strokes, RawStroke};

    fn ring_stroke(cx: f64, cy: f64, radius: f64) -> RawStroke {
        let points = (0..=130)
            .map(|i| {
                let angle = (i as f64 / 128.0) * std::f64::consts::TAU;
                Point { x: cx + angle.cos() * radius, y: cy + angle.sin() * radius }
            })
            .collect();
        RawStroke { id: "ring".into(), points }
    }

    #[test]
    fn empty_strokes_are_not_closed() {
        let result = analyze_topological_closure(&[], &RING);
        assert!(!result.closed);
        assert_eq!(result.enclosed_area_px, 0.0);
    }

    #[test]
    fn open_stroke_is_not_closed() {
        // A straight line has no enclosed area at all.
        let raw = vec![RawStroke { id: "s1".into(), points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 200.0, y: 0.0 }] }];
        let strokes = clean_strokes(&raw, &INPUT);
        let result = analyze_topological_closure(&strokes, &RING);
        assert!(!result.closed);
    }

    #[test]
    fn closed_ring_is_detected() {
        let raw = vec![ring_stroke(300.0, 300.0, 150.0)];
        let strokes = clean_strokes(&raw, &INPUT);
        let result = analyze_topological_closure(&strokes, &RING);
        assert!(result.closed, "a clean 150px-radius ring should close");
        assert!((result.radius - 150.0).abs() < 5.0, "detected radius {} should be close to 150", result.radius);
        assert!((result.center.x - 300.0).abs() < 5.0);
        assert!((result.center.y - 300.0).abs() < 5.0);
    }

    #[test]
    fn too_small_ring_fails_min_radius() {
        // Below config.ring.minRadius (70), even if topologically closed.
        let raw = vec![ring_stroke(100.0, 100.0, 30.0)];
        let strokes = clean_strokes(&raw, &INPUT);
        let result = analyze_topological_closure(&strokes, &RING);
        assert!(!result.closed, "a 30px ring is below minRadius=70 and should not count as closed");
    }
}
