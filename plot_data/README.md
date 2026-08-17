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
  Plotly.js loads from a CDN. `api.js` is the only file that talks to the
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
