# plot_data

Tauri app that plots weather data from `../Data/weather_all.parquet`
(produced by `cleanup_data`) using Plotly.js.

## Architecture

- **`src-tauri/src/data.rs`** — all the actual query logic (reading the
  Parquet file, filtering by datatype/station). Plain Rust functions, no
  Tauri dependency. Reusable as-is if this ever moves behind a web API
  (e.g. an Axum server) instead of a desktop app.
- **`src-tauri/src/main.rs`** — thin `#[tauri::command]` wrappers around
  `data.rs`.
- **`src/`** — plain HTML/JS/CSS frontend, no build step, no framework.
  Plotly.js is vendored locally (`plotly-2.35.2.min.js`), so no internet
  connection is needed at runtime. `api.js` is the only file that talks to the
  Tauri backend (`invoke(...)`); everything else (`main.js`, `index.html`)
  just calls `getStations()` / `getDatatypes()` / `getSeries()`. If you
  later want this as a plain webpage backed by a REST API, `api.js` is the
  only file that needs to change — swap the `invoke()` calls for `fetch()`
  calls to your API.

## Combo boxes

- **Field** — populated from whatever `datatype` values exist in the
  Parquet file (TMAX, TMIN, PRCP, and anything else `pull_data` has
  fetched). Adding a new field to the data automatically makes it
  selectable here — no code change needed.
- **City** — populated from distinct `station` values, with friendly names
  from a small lookup table in `data.rs`. "All" (empty selection) plots
  every station as a separate line.

## Moving average

Both the Daily and Trends views have a "Moving avg" number input (days for
Daily, periods/months-or-years for Trends). Setting it to a value > 1 adds a
dashed overlay line per station: a centered rolling mean of `value`
computed server-side via polars `rolling_mean` (`with_rolling_avg` in
`data.rs`), partitioned per station with `.over()`. Leaving it blank/0 keeps
the current raw-only behavior.

## Low-pass filter

Each view also has a "Low-pass" number input (same units as the moving avg).
Setting it to a value > 1 adds a dotted overlay line per station: a zero-phase
exponential low-pass filter (first-order IIR applied forward then backward,
`with_low_pass` in `data.rs`, polars `ewm_mean`). The value is the EMA span --
alpha = 2/(span+1) -- so similar numbers give similar smoothing scale to the
moving average, but with a gentler rolloff and no phase lag. The two controls
are independent; both overlays can be shown at once.

## Hot days

The **Hot days** tab plots, per station, the number of days each year where
TMAX exceeds a temperature you type in (in the data's native units, degrees
Fahrenheit here). Alongside the raw count it draws a dashed quadratic fit of
that count over the modern record (years >= 1980), computed server-side via
least squares in `get_hot_days_per_year` / `quadratic_fit` in `data.rs`
(years are centered before solving the normal equations to keep the system
well-conditioned).

## Growing season

The **Growing season** tab plots, per station, the number of days between the
last spring frost and the first fall frost of each year — the typical
"frost-free" growing season. A frost day is one whose low (TMIN) reached 32 °F
or below; spring frosts are those in March-June, fall frosts those in
July-November (`get_growing_season` / `season_lengths` in `data.rs`).
Mid-winter frosts don't bound a season, which also keeps years with an
incomplete station record (e.g. only a January and a December frost logged)
from producing a bogus year-long season. Alongside the raw line it draws a
dashed **cubic** least-squares fit of the season length over the full record.

The polynomial fits on both pages share one solver: `poly_fit(points, degree)`
in `data.rs` solves the centered normal equations (Gauss-Jordan with partial
pivoting), so hot days gets its quadratic (degree 2) and growing season its
cubic (degree 3) from the same code.

## Setup

Requires the Tauri CLI:

```bash
cargo install tauri-cli --version "^2"
```

Data path: by default the app looks for
`~/.config/RobertBicknell/Weatheranalysis/weather_all.parquet` (via
`dirs::config_dir()` -- `%APPDATA%\RobertBicknell\Weatheranalysis` on
Windows). This is the same location `cleanup_data` writes to. Override with:

```bash
export WEATHER_DATA_PATH=/absolute/path/to/weather_all.parquet
```

## Run (dev)

```bash
cd plot_data
cargo tauri dev
```

## Build

```bash
cargo tauri build
```

## Notes

- Untested in this environment (no Rust/Tauri toolchain available) — do a
  `cargo tauri dev` locally before relying on it. The polars API surface
  used here (`LazyFrame::scan_parquet`, `.str()`, `.f64()`,
  `into_no_null_iter()`) matches polars 0.44 at time of writing; if you're
  on a different version, check for renamed methods.
- Station friendly names are hardcoded in `data.rs`. If you pull data for
  a new station, add it there or the UI will just show the raw
  `GHCND:...` ID.
