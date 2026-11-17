# Fredrik runs through Berlin

An animated map of every run recorded in Berlin between December 2022 and November 2026. All runs start at the same moment, and one second of playback equals one minute of real running. Each runner glows orange on a black and white map of the city, and the streets run most often build up to white.

## Layout

| Path | Contents |
|---|---|
| `site/index.html` | The finished page with the run tracks embedded. This folder is what gets deployed. |
| `src/template.html` | Page source with a placeholder for the run data. |
| `data/runs.json` | Compact Berlin run tracks extracted from the export. |
| `scripts/parse_runs.py` | Reads a Strava bulk export and writes `data/runs.json`. |
| `scripts/build_page.py` | Embeds `data/runs.json` into the template and writes `site/index.html`. |

## Rebuild

Unzip the Strava bulk export into a folder outside the repository, then run:

```bash
uv run python scripts/parse_runs.py /path/to/export data/runs.json
```

```bash
uv run python scripts/build_page.py src/template.html data/runs.json site/index.html
```

A run counts as a Berlin run when it starts inside the Berlin bounding box and at least 90 percent of its points stay inside that box. Runs without GPS are skipped.

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
