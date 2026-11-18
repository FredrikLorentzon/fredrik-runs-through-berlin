//! Readers for the TCX, GPX and FIT activity files in a Strava export.

use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use chrono::{DateTime, NaiveDateTime};
use fitparser::de::{DecodeOption, from_bytes_with_options};
use fitparser::profile::MesgNum;
use fitparser::{FitDataField, FitDataRecord, Value};
use flate2::read::MultiGzDecoder;
use roxmltree::{Document, Node, ParsingOptions};

use crate::track::TrackPoint;

pub type ActivityResult<T> = Result<T, Box<dyn Error>>;

const SEMICIRCLE_TO_DEGREES: f64 = 180.0 / 2_147_483_648.0;
const NULL_ISLAND_DEGREES: f64 = 0.001;
const PYTHON_WHITESPACE: &[u8] = b" \t\n\r\x0b\x0c";
const BYTE_ORDER_MARK: char = '\u{feff}';

/// A timestamped position, with the time in microseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fix {
    micros: i64,
    lat: f64,
    lon: f64,
}

/// Parse an ISO 8601 timestamp as written by GPX and TCX files, reading a missing offset as UTC.
fn parse_iso_time(text: &str) -> ActivityResult<i64> {
    let text = text.trim().replace('Z', "+00:00");
    if let Ok(moment) = DateTime::parse_from_rfc3339(&text) {
        return Ok(moment.timestamp_micros());
    }
    Ok(
        NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S%.f")?
            .and_utc()
            .timestamp_micros(),
    )
}

/// Read a possibly gzip compressed activity file.
fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let raw = fs::read(path)?;
    if path.extension().is_none_or(|extension| extension != "gz") {
        return Ok(raw);
    }
    let mut data = Vec::new();
    MultiGzDecoder::new(raw.as_slice()).read_to_end(&mut data)?;
    Ok(data)
}

/// Return the XML text of a document with surrounding whitespace and a byte order mark removed.
fn xml_text(data: &[u8]) -> ActivityResult<&str> {
    let start = data
        .iter()
        .position(|byte| !PYTHON_WHITESPACE.contains(byte))
        .unwrap_or(data.len());
    let end = data
        .iter()
        .rposition(|byte| !PYTHON_WHITESPACE.contains(byte))
        .map_or(start, |index| index + 1);
    let text = std::str::from_utf8(&data[start..end])?;
    Ok(text.strip_prefix(BYTE_ORDER_MARK).unwrap_or(text))
}

/// Parse an XML document, accepting a document type declaration.
fn parse_xml(text: &str) -> ActivityResult<Document<'_>> {
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    Ok(Document::parse_with_options(text, options)?)
}

/// Tell whether a node is an element with the given name in any namespace.
fn is_element_named(node: &Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

/// Parse the text of an element as a float.
fn element_float(node: &Node) -> ActivityResult<f64> {
    let text = node.text().ok_or("coordinate element without text")?;
    Ok(text.trim().parse()?)
}

/// Return timestamped positions from a TCX document.
fn parse_tcx(data: &[u8]) -> ActivityResult<Vec<Fix>> {
    let text = xml_text(data)?;
    let document = parse_xml(text)?;
    let mut fixes = Vec::new();
    for trackpoint in document
        .descendants()
        .filter(|node| is_element_named(node, "Trackpoint"))
    {
        let mut time_text = None;
        let mut lat = None;
        let mut lon = None;
        for child in trackpoint.descendants().filter(Node::is_element) {
            match child.tag_name().name() {
                "Time" => time_text = child.text(),
                "LatitudeDegrees" => lat = Some(element_float(&child)?),
                "LongitudeDegrees" => lon = Some(element_float(&child)?),
                _ => {}
            }
        }
        if let (Some(time), Some(lat), Some(lon)) =
            (time_text.filter(|time| !time.is_empty()), lat, lon)
        {
            fixes.push(Fix {
                micros: parse_iso_time(time)?,
                lat,
                lon,
            });
        }
    }
    Ok(fixes)
}

/// Read a coordinate attribute of a GPX track point.
fn coordinate_attribute(node: &Node, name: &str) -> ActivityResult<f64> {
    let text = node
        .attribute(name)
        .ok_or_else(|| format!("trkpt without {name}"))?;
    Ok(text.trim().parse()?)
}

/// Return timestamped positions from a GPX document.
fn parse_gpx(data: &[u8]) -> ActivityResult<Vec<Fix>> {
    let text = xml_text(data)?;
    let document = parse_xml(text)?;
    let mut fixes = Vec::new();
    for trackpoint in document
        .descendants()
        .filter(|node| is_element_named(node, "trkpt"))
    {
        let time_text = trackpoint
            .children()
            .find(|child| is_element_named(child, "time"))
            .and_then(|time| time.text());
        if let Some(time) = time_text.filter(|time| !time.is_empty()) {
            fixes.push(Fix {
                micros: parse_iso_time(time)?,
                lat: coordinate_attribute(&trackpoint, "lat")?,
                lon: coordinate_attribute(&trackpoint, "lon")?,
            });
        }
    }
    Ok(fixes)
}

/// Return the value of a named field of a FIT message, when the field is present and valid.
fn fit_field<'a>(record: &'a FitDataRecord, name: &str) -> Option<&'a Value> {
    record
        .fields()
        .iter()
        .find(|field| field.name() == name)
        .map(FitDataField::value)
}

/// Convert a FIT position in semicircles to degrees.
fn semicircles_to_degrees(value: &Value) -> ActivityResult<f64> {
    let semicircles: f64 = value.clone().try_into()?;
    Ok(semicircles * SEMICIRCLE_TO_DEGREES)
}

/// Return timestamped positions from a FIT file, reading it even when a checksum does not match.
fn parse_fit(data: &[u8]) -> ActivityResult<Vec<Fix>> {
    let options = HashSet::from([
        DecodeOption::SkipHeaderCrcValidation,
        DecodeOption::SkipDataCrcValidation,
    ]);
    let mut fixes = Vec::new();
    for record in from_bytes_with_options(data, &options)? {
        if record.kind() != MesgNum::Record {
            continue;
        }
        let fields = (
            fit_field(&record, "timestamp"),
            fit_field(&record, "position_lat"),
            fit_field(&record, "position_long"),
        );
        if let (Some(Value::Timestamp(moment)), Some(lat), Some(lon)) = fields {
            fixes.push(Fix {
                micros: moment.timestamp_micros(),
                lat: semicircles_to_degrees(lat)?,
                lon: semicircles_to_degrees(lon)?,
            });
        }
    }
    Ok(fixes)
}

/// Drop fixes at null island, sort the rest by time and measure time from the first fix.
fn to_points(mut fixes: Vec<Fix>) -> Vec<TrackPoint> {
    fixes.retain(|fix| fix.lat.abs() > NULL_ISLAND_DEGREES || fix.lon.abs() > NULL_ISLAND_DEGREES);
    fixes.sort_by_key(|fix| fix.micros);
    let Some(start) = fixes.first().map(|fix| fix.micros) else {
        return Vec::new();
    };
    fixes
        .into_iter()
        .map(|fix| TrackPoint {
            seconds: (fix.micros - start) as f64 / 1_000_000.0,
            lat: fix.lat,
            lon: fix.lon,
        })
        .collect()
}

/// Parse any supported activity file into points relative to its first fix.
pub fn parse_activity(path: &Path) -> ActivityResult<Vec<TrackPoint>> {
    let data = read_bytes(path)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let fixes = if name.contains(".tcx") {
        parse_tcx(&data)?
    } else if name.contains(".gpx") {
        parse_gpx(&data)?
    } else if name.contains(".fit") {
        parse_fit(&data)?
    } else {
        return Ok(Vec::new());
    };
    Ok(to_points(fixes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCX: &str = r#"
  <?xml version="1.0" encoding="UTF-8"?>
<TrainingCenterDatabase xmlns="http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2">
  <Activities><Activity><Lap><Track>
    <Trackpoint><Time>2024-05-01T06:00:00.000Z</Time><Position><LatitudeDegrees>52.5</LatitudeDegrees><LongitudeDegrees>13.4</LongitudeDegrees></Position></Trackpoint>
    <Trackpoint><Time>2024-05-01T06:00:01.500Z</Time></Trackpoint>
    <Trackpoint><Time>2024-05-01T08:00:02+02:00</Time><Position><LatitudeDegrees>52.6</LatitudeDegrees><LongitudeDegrees>13.5</LongitudeDegrees></Position></Trackpoint>
  </Track></Lap></Activity></Activities>
</TrainingCenterDatabase>
"#;

    const GPX: &str = r#"<?xml version="1.0"?>
<gpx xmlns="http://www.topografix.com/GPX/1/1"><trk><trkseg>
  <trkpt lat="52.5" lon="13.4"><ele>40</ele><time>2024-05-01T06:00:00Z</time></trkpt>
  <trkpt lat="52.6" lon="13.5"></trkpt>
  <trkpt lat=" 52.7 " lon="13.6"><time>2024-05-01T06:00:05Z</time></trkpt>
</trkseg></trk></gpx>"#;

    /// Track points without a position are skipped and offsets are honoured.
    #[test]
    fn parses_tcx_with_leading_whitespace() {
        let fixes = parse_tcx(TCX.as_bytes()).unwrap();
        assert_eq!(fixes.len(), 2);
        assert_eq!(fixes[1].micros - fixes[0].micros, 2_000_000);
        assert_eq!((fixes[1].lat, fixes[1].lon), (52.6, 13.5));
    }

    /// Track points without a time are skipped.
    #[test]
    fn parses_gpx() {
        let fixes = parse_gpx(GPX.as_bytes()).unwrap();
        assert_eq!(
            fixes.iter().map(|fix| fix.lat).collect::<Vec<_>>(),
            [52.5, 52.7]
        );
        assert_eq!(fixes[1].micros - fixes[0].micros, 5_000_000);
    }

    /// Broken XML is an error, not an empty track.
    #[test]
    fn rejects_broken_xml() {
        assert!(parse_gpx(b"<gpx><trk>").is_err());
    }

    /// Fractions beyond microseconds are truncated and a missing offset reads as UTC.
    #[test]
    fn parses_iso_variants() {
        let utc = parse_iso_time("2024-05-01T06:00:00Z").unwrap();
        assert_eq!(parse_iso_time(" 2024-05-01T08:00:00+02:00 ").unwrap(), utc);
        assert_eq!(
            parse_iso_time("2024-05-01T06:00:00.1234569Z").unwrap(),
            utc + 123_456
        );
        assert_eq!(parse_iso_time("2024-05-01T06:00:00").unwrap(), utc);
        assert!(parse_iso_time("yesterday").is_err());
    }

    /// Fixes at null island go and the rest are sorted and timed from the first fix.
    #[test]
    fn to_points_filters_sorts_and_times() {
        let fixes = vec![
            Fix {
                micros: 3_000_000,
                lat: 52.6,
                lon: 13.5,
            },
            Fix {
                micros: 0,
                lat: 0.0,
                lon: 0.0005,
            },
            Fix {
                micros: 1_500_000,
                lat: 52.5,
                lon: 13.4,
            },
        ];
        let points = to_points(fixes);
        assert_eq!(
            points.iter().map(|p| p.seconds).collect::<Vec<_>>(),
            [0.0, 1.5]
        );
        assert_eq!(points[0].lat, 52.5);
    }

    /// Surrounding whitespace and a byte order mark do not reach the XML parser.
    #[test]
    fn xml_text_strips_whitespace_and_bom() {
        assert_eq!(xml_text(b"\x0b \n\xef\xbb\xbf<a/>\r\n").unwrap(), "<a/>");
        assert_eq!(xml_text(b"   ").unwrap(), "");
    }
}
