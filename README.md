# Fredrik runs through Berlin

An animated map of every run recorded in Berlin between December 2022 and November 2026. All runs start at the same moment, and one second of playback equals one minute of real running. Each runner glows orange on a black and white map of the city, and the streets run most often build up to white.

## Layout

| Path | Contents |
|---|---|
| `site/index.html` | The finished page with the run tracks embedded. This folder is what gets deployed. |
| `src/template.html` | Page source with a placeholder for the run data. |
| `data/runs.json` | Compact Berlin run tracks extracted from the export. |
| `src/bin/parse_runs/` | Rust tool that reads a Strava bulk export and writes `data/runs.json`. It reads TCX, GPX and FIT activity files, plain or gzipped. |
| `src/bin/build_page.rs` | Rust tool that embeds `data/runs.json` into the template and writes `site/index.html`. |

## Rebuild

The build tools need Rust 1.88 or later. Unzip the Strava bulk export into a folder outside the repository, then run:

```bash
cargo run --release --bin parse_runs -- /path/to/export data/runs.json
```

```bash
cargo run --release --bin build_page -- src/template.html data/runs.json site/index.html
```

A run counts as a Berlin run when it starts inside the Berlin bounding box and at least 90 percent of its points stay inside that box. The box is `BERLIN_BOUNDS` in `src/bin/parse_runs/main.rs`. Runs without GPS are skipped.

The unit tests of both tools run with:

```bash
cargo test
```

## Preview

```bash
python3 -m http.server 8765 --directory site
```

Then open http://localhost:8765 in a browser.

## Deploy

Deploy with Netlify Drop. Open app.netlify.com/drop, drag the `site` folder onto the page and sign in to keep the site. The site name can be changed under Site configuration.

## Privacy

`data/runs.json` and `site/index.html` contain the GPS tracks of every run. Anyone who can open the page can see where the runs start and end.

## Credits

Runs recorded with Polar. Map data from OpenFreeMap, OpenMapTiles and OpenStreetMap contributors. Rendering with MapLibre GL JS and deck.gl.
