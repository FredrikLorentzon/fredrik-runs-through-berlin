//! Extract Berlin runs from a Strava bulk export into a compact JSON track file.

mod activity;
mod track;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::NaiveDateTime;
use clap::Parser;
use serde::Serialize;

use crate::activity::parse_activity;
use crate::track::{
    COORD_SCALE, TrackPoint, decimate, encode_track, round_hundredths, simplify, track_metres,
};

const BERLIN_BOUNDS: Bounds = Bounds {
    south: 52.338,
    north: 52.675,
    west: 13.088,
    east: 13.761,
};
const MIN_BERLIN_SHARE: f64 = 0.9;
const MIN_POINTS: usize = 10;
const STRAVA_DATE_FORMAT: &str = "%b %d, %Y, %I:%M:%S %p";

/// Extract Berlin runs from a Strava bulk export into a compact JSON track file.
#[derive(Parser)]
struct Args {
    /// Folder holding the unzipped Strava bulk export.
    export_dir: PathBuf,
    /// Compact run track file to write.
    output: PathBuf,
}

/// A latitude and longitude box.
struct Bounds {
    south: f64,
    north: f64,
    west: f64,
    east: f64,
}

impl Bounds {
    /// Tell whether a point lies inside the box.
    fn contains(&self, point: &TrackPoint) -> bool {
        self.south <= point.lat
            && point.lat <= self.north
            && self.west <= point.lon
            && point.lon <= self.east
    }
}

/// A run listed in the export's activity index.
struct RunRow {
    file: String,
    date: String,
}

/// One run in the compact track file.
#[derive(Serialize)]
struct Track {
    date: String,
    km: f64,
    track: Vec<i64>,
}

/// The compact track file.
#[derive(Serialize)]
struct Payload<'a> {
    scale: i64,
    runs: &'a [Track],
}

/// Counts of the runs left out of the track file, by reason.
#[derive(Default)]
struct Skipped {
    no_gps: usize,
    outside_berlin: usize,
    unreadable: usize,
}

/// Tell whether a run starts in Berlin and keeps most of its points there.
fn is_berlin_run(points: &[TrackPoint]) -> bool {
    let inside = points
        .iter()
        .filter(|point| BERLIN_BOUNDS.contains(point))
        .count();
    let share = inside as f64 / points.len() as f64;
    points
        .first()
        .is_some_and(|start| BERLIN_BOUNDS.contains(start))
        && share >= MIN_BERLIN_SHARE
}

/// Read the run rows from the export's activity index.
fn load_runs(export_dir: &Path) -> Result<Vec<RunRow>, Box<dyn Error>> {
    let index_path = export_dir.join("activities.csv");
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(&index_path)?;
    let header = reader.headers()?.clone();
    let column = |name: &str| {
        header
            .iter()
            .position(|title| title == name)
            .ok_or_else(|| format!("{} has no {name} column", index_path.display()))
    };
    let (type_index, file_index, date_index) = (
        column("Activity Type")?,
        column("Filename")?,
        column("Activity Date")?,
    );
    let mut runs = Vec::new();
    for record in reader.records() {
        let record = record?;
        if record.get(type_index) == Some("Run")
            && let Some(file) = record.get(file_index).filter(|file| !file.is_empty())
        {
            runs.push(RunRow {
                file: file.to_owned(),
                date: record.get(date_index).unwrap_or_default().to_owned(),
            });
        }
    }
    Ok(runs)
}

/// Turn a Strava activity date into an ISO calendar date.
fn calendar_date(text: &str) -> Result<String, Box<dyn Error>> {
    let moment = NaiveDateTime::parse_from_str(text, STRAVA_DATE_FORMAT)
        .map_err(|error| format!("activity date {text:?}: {error}"))?;
    Ok(moment.format("%Y-%m-%d").to_string())
}

/// Parse every run, keep the Berlin ones and write the compact track file.
fn run(args: &Args) -> Result<(), Box<dyn Error>> {
    let mut tracks = Vec::new();
    let mut skipped = Skipped::default();
    for row in load_runs(&args.export_dir)? {
        let Ok(points) = parse_activity(&args.export_dir.join(&row.file)) else {
            skipped.unreadable += 1;
            continue;
        };
        if points.len() < MIN_POINTS {
            skipped.no_gps += 1;
            continue;
        }
        if !is_berlin_run(&points) {
            skipped.outside_berlin += 1;
            continue;
        }
        tracks.push(Track {
            date: calendar_date(&row.date)?,
            km: round_hundredths(track_metres(&points) / 1000.0),
            track: encode_track(&simplify(decimate(&points))),
        });
    }

    tracks.sort_by(|a, b| a.date.cmp(&b.date));
    let payload = serde_json::to_string(&Payload {
        scale: COORD_SCALE,
        runs: &tracks,
    })?;
    fs::write(&args.output, payload)
        .map_err(|error| format!("{}: {error}", args.output.display()))?;
    let total_points: usize = tracks.iter().map(|track| track.track.len() / 3).sum();
    eprintln!(
        "kept {} runs, {total_points} points, skipped no_gps {}, outside_berlin {}, unreadable {}",
        tracks.len(),
        skipped.no_gps,
        skipped.outside_berlin,
        skipped.unreadable
    );
    Ok(())
}

/// Run the extraction and report any error on standard error.
fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a point at a latitude and longitude.
    fn at(lat: f64, lon: f64) -> TrackPoint {
        TrackPoint {
            seconds: 0.0,
            lat,
            lon,
        }
    }

    /// A run must start in Berlin and keep at least nine in ten points inside it.
    #[test]
    fn berlin_run_needs_start_and_share() {
        let berlin = at(52.5, 13.4);
        let potsdam = at(52.39, 13.0);
        let mut points = vec![berlin; 9];
        points.push(potsdam);
        assert!(is_berlin_run(&points));
        points.push(potsdam);
        assert!(!is_berlin_run(&points));
        let mut from_potsdam = vec![potsdam];
        from_potsdam.extend(vec![berlin; 20]);
        assert!(!is_berlin_run(&from_potsdam));
    }

    /// Strava activity dates without zero padding parse to ISO dates.
    #[test]
    fn calendar_date_reads_strava_format() {
        assert_eq!(
            calendar_date("Dec 3, 2022, 7:12:45 AM").unwrap(),
            "2022-12-03"
        );
        assert_eq!(
            calendar_date("Nov 17, 2026, 11:05:00 PM").unwrap(),
            "2026-11-17"
        );
        assert!(calendar_date("3 December 2022").is_err());
    }
}
