# Fredrik runs through Berlin

An animated map of every run recorded in Berlin between December 2022 and November 2026. All runs start at the same moment, and one second of playback equals one minute of real running. Each runner glows orange on a black and white map of the city, and the streets run most often build up to white.

The repository holds the page source and the build tools, but no run data. To build the page, request your own archive from Strava and run the two tools on it.

## Layout

| Path | Contents |
|---|---|
| `src/template.html` | Page source with a placeholder for the run data. |
| `src/bin/parse_runs/` | Rust tool that reads a Strava archive and writes `data/runs.json`. It reads TCX, GPX and FIT activity files, plain or gzipped. |
| `src/bin/build_page.rs` | Rust tool that embeds `data/runs.json` into the template and writes `site/index.html`. |
| `data/runs.json` | Compact Berlin run tracks extracted from the archive. Written by `parse_runs` and kept out of the repository. |
| `site/index.html` | The finished page with the run tracks embedded. Written by `build_page` and kept out of the repository. The `site` folder is what gets deployed. |

## Get your Strava archive

Strava sends the full account archive by email. The request works on the Strava website only, not in the app.

1. Log in at strava.com.
2. Hover over your name in the upper right corner, choose Settings and open the My Account tab on the left.
3. Under Download your account, select Get Started.
4. On the next page, select Request download.
5. Wait for the email with the download link. It goes to the email address of your Strava account and can take a few hours to arrive.

The archive is a zip file named `export_<number>.zip`. Unzip it into a folder outside the repository. The tools read `activities.csv` and the activity files in the `activities` folder and ignore the rest of the archive.

## Build

The build tools need Rust 1.88 or later. With the archive unzipped at `/path/to/export`, run:

```bash
cargo run --release --bin parse_runs -- /path/to/export data/runs.json
```

```bash
cargo run --release --bin build_page -- src/template.html data/runs.json site/index.html
```

Both tools create the `data` and `site` folders when they are missing. A run counts as a Berlin run when it starts inside the Berlin bounding box and at least 90 percent of its points stay inside that box. The box is `BERLIN_BOUNDS` in `src/bin/parse_runs/main.rs`. Runs without GPS are skipped.

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

`data/runs.json` and `site/index.html` contain the GPS tracks of every run, which is why both stay out of the repository. Deploying the page publishes them. Anyone who can open the page can see where the runs start and end.

## Credits

Runs recorded with Polar. Map data from OpenFreeMap, OpenMapTiles and OpenStreetMap contributors. Rendering with MapLibre GL JS and deck.gl.
