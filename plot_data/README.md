# plot_data

Weather viewer for the Parquet file produced by `cleanup_data`
(`~/.config/RobertBicknell/Weatheranalysis/weather_all.parquet`, or
`$WEATHER_DATA_PATH`). Ships two front ends over one shared query core:
a Tauri **desktop** app and an Axum **web** server serving the same page.

## Architecture

- **`data-core/`** — `plot-data-core` crate, the actual query logic (reading
  the Parquet, filtering by datatype/station, rolling averages, low-pass
  filter, polynomial fits). Plain Rust functions returning serde-serializable
  structs with **no** Tauri dependency, so one copy backs both ships.
- **`src-tauri/`** — Tauri desktop app. `main.rs` is thin
  `#[tauri::command]` wrappers around `plot-data-core`.
- **`server/`** — `weather-server` crate: an Axum HTTP server that exposes the
  same queries under `/api` and serves the frontend statically. Binds to
  `127.0.0.1`; intended to sit behind a reverse proxy (see
  [`deploy/README.md`](../deploy/README.md)).
- **`src/`** — plain HTML/JS/CSS frontend, no build step, no framework.
  Plotly.js is vendored locally (`plotly-2.35.2.min.js`), so no internet
  connection is needed at runtime. `api.js` is the only file that talks to a
  backend; it picks the transport at runtime — `invoke(...)` when running
  under Tauri, `fetch()` against `/api` otherwise. Everything else
  (`main.js`, `index.html`) just calls `getStations()` / `getDatatypes()` /
  `getSeries()`.

The HTTP API mirrors the Tauri commands one-to-one:

| Endpoint | Params |
|---|---|
| `GET /api/stations` | — |
| `GET /api/datatypes` | — |
| `GET /api/series` | `datatype`, `station`, `window`, `low_pass` |
| `GET /api/mean-temp-trend` | `period`, `station`, `window`, `low_pass` |
| `GET /api/hot-days` | `threshold`, `station` |
| `GET /api/growing-season` | `station` |

## Web server

Environment:

| Variable | Default | Meaning |
|---|---|---|
| `WEATHER_SERVER_PORT` | `9002` | bind port (loopback) |
| `WEATHER_STATIC_DIR` | `../src` (relative to the server crate) | frontend files directory |
| `WEATHER_DATA_PATH` | `~/.config/.../weather_all.parquet` | data file (see below) |

Run (dev):

```bash
cd plot_data/server
cargo run
# -> http://127.0.0.1:9002
```

Because `api.js` resolves `/api` relative to the page's base path, the same
`src/` files work at the site root and under a path prefix such as
`/CorvallisWeather/`.

## Combo boxes

- **Field** — populated from whatever `datatype` values exist in the
  Parquet file (TMAX, TMIN, PRCP, and anything else `pull_data` has
  fetched). Adding a new field to the data automatically makes it
  selectable here — no code change needed.
- **City** — populated from distinct `station` values, with friendly names
  from a small lookup table in `data-core`. "All" (empty selection) plots
  every station as a separate line.

## Moving average

Both the Daily and Trends views have a "Moving avg" number input (days for
Daily, periods/months-or-years for Trends). Setting it to a value > 1 adds a
dashed overlay line per station: a centered rolling mean of `value`
computed server-side via polars `rolling_mean` (`with_rolling_avg` in
`data-core`), partitioned per station with `.over()`. Leaving it blank/0
keeps the raw-only behavior.

## Low-pass filter

Each view also has a "Low-pass" number input (same units as the moving avg).
Setting it to a value > 1 adds a dotted overlay line per station: a zero-phase
exponential low-pass filter (first-order IIR applied forward then backward,
`with_low_pass` in `data-core`, polars `ewm_mean`). The value is the EMA span —
alpha = 2/(span+1) — so similar numbers give similar smoothing scale to the
moving average, but with a gentler rolloff and no phase lag. The two controls
are independent; both overlays can be shown at once.

## Hot days

The **Hot days** tab plots, per station, the number of days each year where
TMAX exceeds a temperature you type in (in the data's native units, degrees
Fahrenheit here). Alongside the raw count it draws a dashed quadratic fit of
that count over the modern record (years >= 1980), computed server-side via
least squares in `get_hot_days_per_year` / `poly_fit` in `data-core` (years
are centered before solving the normal equations to keep the system
well-conditioned).

## Growing season

The **Growing season** tab plots, per station, the number of days between the
last spring frost and the first fall frost of each year — the typical
"frost-free" growing season. A frost day is one whose low (TMIN) reached 32 °F
or below; spring frosts are those in March-June, fall frosts those in
July-November (`get_growing_season` / `season_lengths` in `data-core`).
Mid-winter frosts don't bound a season, which also keeps years with an
incomplete station record (e.g. only a January and a December frost logged)
from producing a bogus year-long season. Alongside the raw line it draws a
dashed **cubic** least-squares fit of the season length over the full record.

The polynomial fits on both pages share one solver: `poly_fit(points, degree)`
in `data-core` solves the centered normal equations (Gauss-Jordan with partial
pivoting), so hot days gets its quadratic (degree 2) and growing season its
cubic (degree 3) from the same code.

## Setup

Requires the Tauri CLI for the desktop app:

```bash
cargo install tauri-cli --version "^2"
```

Data path: by default the app looks for
`~/.config/RobertBicknell/Weatheranalysis/weather_all.parquet` (via
`dirs::config_dir()` — `%APPDATA%\RobertBicknell\Weatheranalysis` on
Windows). This is the same location `cleanup_data` writes to. Override with:

```bash
export WEATHER_DATA_PATH=/absolute/path/to/weather_all.parquet
```

## Run (dev, desktop)

```bash
cd plot_data
cargo tauri dev
```

## Run (dev, web)

```bash
cd plot_data/server
cargo run
# open http://127.0.0.1:9002
```

## Build

```bash
cd plot_data
cargo tauri build        # desktop bundles
```

For the web server's release binary and the production install (systemd +
Caddy), see [`deploy/README.md`](../deploy/README.md).

## Tests

The query core has unit tests (including real-data end-to-end ones that skip
if the Parquet file is missing):

```bash
cd plot_data/data-core
cargo test
```

## Notes

- The polars API surface used here (`LazyFrame::scan_parquet`, `.str()`,
  `.f64()`, `into_no_null_iter()`, `ewm_mean`, `rolling_mean`) matches
  polars 0.44 at time of writing; if you're on a different version, check for
  renamed methods.
- Station friendly names are hardcoded in `data-core/src/lib.rs`. If you pull
  data for a new station, add it there or the UI will just show the raw
  `GHCND:...` ID.