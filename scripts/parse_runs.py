"""Extract Berlin runs from a Strava bulk export into a compact JSON track file."""

import argparse
import csv
import gzip
import io
import json
import math
import sys
import xml.etree.ElementTree as ElementTree
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

import fitdecode

BERLIN_BOUNDS = (52.338, 52.675, 13.088, 13.761)
MIN_BERLIN_SHARE = 0.9
SAMPLE_SECONDS = 3.0
SIMPLIFY_METRES = 2.0
COORD_SCALE = 100000
SEMICIRCLE_TO_DEGREES = 180.0 / 2**31
EARTH_RADIUS_METRES = 6371000.0


@dataclass
class TrackPoint:
    """A single GPS fix with seconds since the start of the run."""

    seconds: float
    lat: float
    lon: float


def parse_iso_time(text: str) -> datetime:
    """Parse an ISO 8601 timestamp as written by GPX and TCX files."""
    return datetime.fromisoformat(text.strip().replace("Z", "+00:00"))


def read_bytes(path: Path) -> bytes:
    """Read a possibly gzip compressed activity file."""
    raw = path.read_bytes()
    if path.suffix == ".gz":
        return gzip.decompress(raw)
    return raw


def local_name(tag: str) -> str:
    """Strip the XML namespace from an element tag."""
    return tag.rsplit("}", 1)[-1]


def parse_tcx(data: bytes) -> list[tuple[datetime, float, float]]:
    """Return timestamped positions from a TCX document."""
    root = ElementTree.fromstring(data.strip())
    fixes: list[tuple[datetime, float, float]] = []
    for element in root.iter():
        if local_name(element.tag) != "Trackpoint":
            continue
        time_text = None
        lat = None
        lon = None
        for child in element.iter():
            name = local_name(child.tag)
            if name == "Time":
                time_text = child.text
            elif name == "LatitudeDegrees":
                lat = float(child.text)
            elif name == "LongitudeDegrees":
                lon = float(child.text)
        if time_text and lat is not None and lon is not None:
            fixes.append((parse_iso_time(time_text), lat, lon))
    return fixes


def parse_gpx(data: bytes) -> list[tuple[datetime, float, float]]:
    """Return timestamped positions from a GPX document."""
    root = ElementTree.fromstring(data.strip())
    fixes: list[tuple[datetime, float, float]] = []
    for element in root.iter():
        if local_name(element.tag) != "trkpt":
            continue
        time_text = next(
            (child.text for child in element if local_name(child.tag) == "time"),
            None,
        )
        if time_text:
            fixes.append(
                (
                    parse_iso_time(time_text),
                    float(element.attrib["lat"]),
                    float(element.attrib["lon"]),
                )
            )
    return fixes


def parse_fit(data: bytes) -> list[tuple[datetime, float, float]]:
    """Return timestamped positions from a FIT file."""
    fixes: list[tuple[datetime, float, float]] = []
    with fitdecode.FitReader(io.BytesIO(data)) as reader:
        for frame in reader:
            if not isinstance(frame, fitdecode.FitDataMessage) or frame.name != "record":
                continue
            if not (
                frame.has_field("timestamp")
                and frame.has_field("position_lat")
                and frame.has_field("position_long")
            ):
                continue
            lat = frame.get_value("position_lat")
            lon = frame.get_value("position_long")
            if lat is None or lon is None:
                continue
            fixes.append(
                (
                    frame.get_value("timestamp"),
                    lat * SEMICIRCLE_TO_DEGREES,
                    lon * SEMICIRCLE_TO_DEGREES,
                )
            )
    return fixes


def parse_activity(path: Path) -> list[TrackPoint]:
    """Parse any supported activity file into points relative to its first fix."""
    data = read_bytes(path)
    name = path.name.lower()
    if ".tcx" in name:
        fixes = parse_tcx(data)
    elif ".gpx" in name:
        fixes = parse_gpx(data)
    elif ".fit" in name:
        fixes = parse_fit(data)
    else:
        return []
    fixes = [fix for fix in fixes if abs(fix[1]) > 0.001 or abs(fix[2]) > 0.001]
    if not fixes:
        return []
    fixes.sort(key=lambda fix: fix[0])
    start = fixes[0][0]
    return [
        TrackPoint((moment - start).total_seconds(), lat, lon)
        for moment, lat, lon in fixes
    ]


def in_berlin(point: TrackPoint) -> bool:
    """Tell whether a point lies inside the Berlin bounding box."""
    south, north, west, east = BERLIN_BOUNDS
    return south <= point.lat <= north and west <= point.lon <= east


def haversine_metres(a: TrackPoint, b: TrackPoint) -> float:
    """Great circle distance between two points in metres."""
    phi_a = math.radians(a.lat)
    phi_b = math.radians(b.lat)
    d_phi = phi_b - phi_a
    d_lambda = math.radians(b.lon - a.lon)
    h = math.sin(d_phi / 2) ** 2 + math.cos(phi_a) * math.cos(phi_b) * math.sin(d_lambda / 2) ** 2
    return 2 * EARTH_RADIUS_METRES * math.asin(math.sqrt(h))


def decimate(points: list[TrackPoint]) -> list[TrackPoint]:
    """Keep at most one point per sampling interval, always keeping the last point."""
    kept = [points[0]]
    for point in points[1:-1]:
        if point.seconds - kept[-1].seconds >= SAMPLE_SECONDS:
            kept.append(point)
    if len(points) > 1:
        kept.append(points[-1])
    return kept


def perpendicular_metres(point: TrackPoint, start: TrackPoint, end: TrackPoint) -> float:
    """Distance from a point to the segment between two other points, in metres."""
    scale_x = math.cos(math.radians(start.lat)) * EARTH_RADIUS_METRES * math.pi / 180
    scale_y = EARTH_RADIUS_METRES * math.pi / 180
    px = (point.lon - start.lon) * scale_x
    py = (point.lat - start.lat) * scale_y
    ex = (end.lon - start.lon) * scale_x
    ey = (end.lat - start.lat) * scale_y
    length_sq = ex * ex + ey * ey
    if length_sq == 0:
        return math.hypot(px, py)
    ratio = max(0.0, min(1.0, (px * ex + py * ey) / length_sq))
    return math.hypot(px - ratio * ex, py - ratio * ey)


def simplify(points: list[TrackPoint]) -> list[TrackPoint]:
    """Douglas Peucker simplification that keeps the timing of every kept vertex."""
    if len(points) < 3:
        return points
    keep = [False] * len(points)
    keep[0] = True
    keep[-1] = True
    stack = [(0, len(points) - 1)]
    while stack:
        first, last = stack.pop()
        best_index = -1
        best_distance = 0.0
        for index in range(first + 1, last):
            distance = perpendicular_metres(points[index], points[first], points[last])
            if distance > best_distance:
                best_distance = distance
                best_index = index
        if best_index != -1 and best_distance > SIMPLIFY_METRES:
            keep[best_index] = True
            stack.append((first, best_index))
            stack.append((best_index, last))
    return [point for point, flag in zip(points, keep) if flag]


def encode_track(points: list[TrackPoint]) -> list[int]:
    """Delta encode a track as a flat list of seconds, latitude and longitude integers."""
    encoded: list[int] = []
    previous = (0, 0, 0)
    for point in points:
        current = (
            round(point.seconds),
            round(point.lat * COORD_SCALE),
            round(point.lon * COORD_SCALE),
        )
        encoded.extend(value - before for value, before in zip(current, previous))
        previous = current
    return encoded


def load_runs(export_dir: Path) -> list[dict[str, str]]:
    """Read the run rows from the export's activity index."""
    with (export_dir / "activities.csv").open(encoding="utf-8", newline="") as handle:
        reader = csv.reader(handle)
        header = next(reader)
        type_index = header.index("Activity Type")
        file_index = header.index("Filename")
        date_index = header.index("Activity Date")
        return [
            {"file": row[file_index], "date": row[date_index]}
            for row in reader
            if row[type_index] == "Run" and row[file_index]
        ]


def main() -> None:
    """Parse every run, keep the Berlin ones and write the compact track file."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("export_dir", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    tracks: list[dict[str, object]] = []
    skipped = {"no_gps": 0, "outside_berlin": 0, "unreadable": 0}
    for run in load_runs(args.export_dir):
        try:
            points = parse_activity(args.export_dir / run["file"])
        except (ElementTree.ParseError, fitdecode.FitError, OSError, ValueError):
            skipped["unreadable"] += 1
            continue
        if len(points) < 10:
            skipped["no_gps"] += 1
            continue
        share = sum(in_berlin(point) for point in points) / len(points)
        if not in_berlin(points[0]) or share < MIN_BERLIN_SHARE:
            skipped["outside_berlin"] += 1
            continue
        distance = sum(haversine_metres(a, b) for a, b in zip(points, points[1:]))
        compact = simplify(decimate(points))
        date = datetime.strptime(run["date"], "%b %d, %Y, %I:%M:%S %p")
        tracks.append(
            {
                "date": date.strftime("%Y-%m-%d"),
                "km": round(distance / 1000, 2),
                "track": encode_track(compact),
            }
        )

    tracks.sort(key=lambda track: track["date"])
    args.output.write_text(json.dumps({"scale": COORD_SCALE, "runs": tracks}, separators=(",", ":")))
    total_points = sum(len(track["track"]) // 3 for track in tracks)
    print(f"kept {len(tracks)} runs, {total_points} points, skipped {skipped}", file=sys.stderr)


if __name__ == "__main__":
    main()
