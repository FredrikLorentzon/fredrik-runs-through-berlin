//! Geometry, sampling and encoding of GPS tracks.

use std::f64::consts::PI;

pub const COORD_SCALE: i64 = 100_000;
const SAMPLE_SECONDS: f64 = 3.0;
const SIMPLIFY_METRES: f64 = 2.0;
const EARTH_RADIUS_METRES: f64 = 6_371_000.0;

/// A single GPS fix with seconds since the start of the run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackPoint {
    pub seconds: f64,
    pub lat: f64,
    pub lon: f64,
}

/// Great circle distance between two points in metres.
fn haversine_metres(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let phi_a = a.lat.to_radians();
    let phi_b = b.lat.to_radians();
    let d_phi = phi_b - phi_a;
    let d_lambda = (b.lon - a.lon).to_radians();
    let h =
        (d_phi / 2.0).sin().powi(2) + phi_a.cos() * phi_b.cos() * (d_lambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_METRES * h.sqrt().asin()
}

/// Sum floats with Neumaier compensation, the way Python's built in sum does.
fn compensated_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in values {
        let total = sum + value;
        if f64::abs(sum) >= value.abs() {
            compensation += (sum - total) + value;
        } else {
            compensation += (value - total) + sum;
        }
        sum = total;
    }
    if compensation != 0.0 && compensation.is_finite() {
        sum += compensation;
    }
    sum
}

/// Length of the full track along the surface of the earth, in metres.
pub fn track_metres(points: &[TrackPoint]) -> f64 {
    compensated_sum(
        points
            .windows(2)
            .map(|pair| haversine_metres(&pair[0], &pair[1])),
    )
}

/// Keep at most one point per sampling interval, always keeping the first and the last point.
pub fn decimate(points: &[TrackPoint]) -> Vec<TrackPoint> {
    let Some((first, rest)) = points.split_first() else {
        return Vec::new();
    };
    let mut kept = vec![*first];
    if let Some((last, middle)) = rest.split_last() {
        for point in middle {
            if point.seconds - kept[kept.len() - 1].seconds >= SAMPLE_SECONDS {
                kept.push(*point);
            }
        }
        kept.push(*last);
    }
    kept
}

/// Clamp a ratio to the unit interval, sending NaN to 1 like Python's min and max do.
fn clamp_unit(value: f64) -> f64 {
    let capped = if value < 1.0 { value } else { 1.0 };
    if capped > 0.0 { capped } else { 0.0 }
}

/// Distance from a point to the segment between two other points, in metres.
fn perpendicular_metres(point: &TrackPoint, start: &TrackPoint, end: &TrackPoint) -> f64 {
    let scale_x = start.lat.to_radians().cos() * EARTH_RADIUS_METRES * PI / 180.0;
    let scale_y = EARTH_RADIUS_METRES * PI / 180.0;
    let px = (point.lon - start.lon) * scale_x;
    let py = (point.lat - start.lat) * scale_y;
    let ex = (end.lon - start.lon) * scale_x;
    let ey = (end.lat - start.lat) * scale_y;
    let length_sq = ex * ex + ey * ey;
    if length_sq == 0.0 {
        return px.hypot(py);
    }
    let ratio = clamp_unit((px * ex + py * ey) / length_sq);
    (px - ratio * ex).hypot(py - ratio * ey)
}

/// Douglas Peucker simplification that keeps the timing of every kept vertex.
pub fn simplify(points: Vec<TrackPoint>) -> Vec<TrackPoint> {
    if points.len() < 3 {
        return points;
    }
    let last_index = points.len() - 1;
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[last_index] = true;
    let mut stack = vec![(0, last_index)];
    while let Some((first, last)) = stack.pop() {
        let mut best_index = None;
        let mut best_distance = 0.0;
        for index in first + 1..last {
            let distance = perpendicular_metres(&points[index], &points[first], &points[last]);
            if distance > best_distance {
                best_distance = distance;
                best_index = Some(index);
            }
        }
        if let Some(best) = best_index
            && best_distance > SIMPLIFY_METRES
        {
            keep[best] = true;
            stack.push((first, best));
            stack.push((best, last));
        }
    }
    points
        .into_iter()
        .zip(keep)
        .filter_map(|(point, kept)| kept.then_some(point))
        .collect()
}

/// Round half to even and convert to an integer, the way Python's round does.
fn round_even(value: f64) -> i64 {
    value.round_ties_even() as i64
}

/// Delta encode a track as a flat list of seconds, latitude and longitude integers.
pub fn encode_track(points: &[TrackPoint]) -> Vec<i64> {
    let scale = COORD_SCALE as f64;
    let mut encoded = Vec::with_capacity(points.len() * 3);
    let mut previous = [0; 3];
    for point in points {
        let current = [
            round_even(point.seconds),
            round_even(point.lat * scale),
            round_even(point.lon * scale),
        ];
        encoded.extend(
            current
                .iter()
                .zip(previous)
                .map(|(value, before)| value - before),
        );
        previous = current;
    }
    encoded
}

/// Round to two decimals through the exact decimal value, the way Python's round does.
pub fn round_hundredths(value: f64) -> f64 {
    format!("{value:.2}").parse().unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a point from seconds, latitude and longitude.
    fn point(seconds: f64, lat: f64, lon: f64) -> TrackPoint {
        TrackPoint { seconds, lat, lon }
    }

    /// One degree of latitude is about 111.2 km.
    #[test]
    fn haversine_measures_one_degree_of_latitude() {
        let metres = haversine_metres(&point(0.0, 52.0, 13.0), &point(0.0, 53.0, 13.0));
        assert!((metres - 111_194.9).abs() < 0.1, "{metres}");
    }

    /// Compensated summation recovers the rounding a plain fold loses.
    #[test]
    fn compensated_sum_matches_python() {
        assert_eq!(compensated_sum([0.1; 10]), 1.0);
        assert_ne!([0.1; 10].iter().sum::<f64>(), 1.0);
    }

    /// Points closer than the sampling interval are dropped, the last point never is.
    #[test]
    fn decimate_keeps_one_point_per_interval() {
        let points: Vec<_> = [0.0, 1.0, 3.0, 4.0, 7.0, 8.0]
            .iter()
            .map(|&s| point(s, 52.5, 13.4))
            .collect();
        let seconds: Vec<_> = decimate(&points).iter().map(|p| p.seconds).collect();
        assert_eq!(seconds, [0.0, 3.0, 7.0, 8.0]);
    }

    /// A single point survives decimation and an empty track stays empty.
    #[test]
    fn decimate_handles_short_tracks() {
        assert_eq!(decimate(&[point(0.0, 52.5, 13.4)]).len(), 1);
        assert!(decimate(&[]).is_empty());
    }

    /// Points on a straight line go, a corner stays.
    #[test]
    fn simplify_keeps_corners_only() {
        let points = vec![
            point(0.0, 52.5, 13.4),
            point(1.0, 52.5005, 13.4),
            point(2.0, 52.501, 13.4),
            point(3.0, 52.501, 13.401),
            point(4.0, 52.501, 13.402),
        ];
        let seconds: Vec<_> = simplify(points).iter().map(|p| p.seconds).collect();
        assert_eq!(seconds, [0.0, 2.0, 4.0]);
    }

    /// NaN clamps to one and values outside the unit interval clamp to its ends.
    #[test]
    fn clamp_unit_follows_python_min_max() {
        assert_eq!(clamp_unit(f64::NAN), 1.0);
        assert_eq!(clamp_unit(-0.5), 0.0);
        assert_eq!(clamp_unit(2.0), 1.0);
        assert_eq!(clamp_unit(0.25), 0.25);
    }

    /// Values are delta encoded and rounded half to even.
    #[test]
    fn encode_track_deltas_and_rounds_half_even() {
        let encoded = encode_track(&[point(0.5, 52.0, 13.0), point(2.5, 52.000_01, 13.000_02)]);
        assert_eq!(encoded, [0, 5_200_000, 1_300_000, 2, 1, 2]);
    }

    /// Rounding to two decimals follows the exact binary value.
    #[test]
    fn round_hundredths_uses_exact_value() {
        assert_eq!(round_hundredths(2.675), 2.67);
        assert_eq!(round_hundredths(5.236), 5.24);
        assert_eq!(round_hundredths(10.0), 10.0);
    }
}
