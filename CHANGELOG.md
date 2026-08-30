# Changelog

All notable changes to this project are documented in this file, loosely
following [Keep a Changelog](https://keepachangelog.com/). Dates are the
commit date; `Unreleased` covers work in the working tree not yet committed.

## [Unreleased]

### Added
- **Web server** (`plot_data/server/`): Axum HTTP server exposing the same
  queries the desktop app uses as REST endpoints (`/api/stations`,
  `/api/datatypes`, `/api/series`, `/api/mean-temp-trend`, `/api/hot-days`,
  `/api/growing-season`), plus static serving of the frontend. New env vars
  `WEATHER_SERVER_PORT` (default 9002) and `WEATHER_STATIC_DIR`.
- **Shared query core** (`plot_data/data-core/`): the query layer moved out of
  the Tauri app into a Tauri-free library crate used by both the desktop app
  and the web server.
- **Dual-mode frontend** (`plot_data/src/api.js`): picks the backend at
  runtime — `invoke(...)` under Tauri, `fetch()` against `/api` on the web.
  API URLs resolve relative to the page base path, so the same files work at
  the site root and under a path prefix.
- **Deployment** (`deploy/`): systemd unit, Caddy site block, and
  install/uninstall scripts for serving the app on this machine at
  `http://bicknellfamily.duckdns.org/CorvallisWeather/` (plain HTTP, mirroring
  the Mealie setup), plus a dedicated `deploy/README.md`.
- Root `CHANGELOG.md`.

### Changed

- **Development tooling** (`hooks/pre-push`): git pre-push hook running
  `cargo audit`, `cargo fmt --check`, `cargo clippy --all-targets -- -D
  warnings`, and `cargo test` in every crate (enable with
  `git config core.hooksPath hooks`). All crates were `cargo fmt`-normalized
  to pass it.
- **Dependency fixes:** bumped `h2` to 0.4.19 (fixes RUSTSEC-2026-0258);
  nullified remaining unreachable transitive advisories (fast-float, pyo3,
  quick-xml, all via polars) in each crate's `cargo-audit.toml`, with a TODO
  to revisit on the next polars upgrade.

## [2026-08-30]

### Added
- **Hot days tab**: per-year count of days where TMAX exceeds a typed-in
  threshold, with a dashed least-squares **quadratic** fit over the modern
  record (years >= 1980).
- **Growing season tab**: days between the last spring frost and first fall
  frost each year (TMIN <= 32 °F; spring = Mar–Jun, fall = Jul–Nov, so
  mid-winter frosts can't fabricate a season), with a dashed **cubic** fit
  over the full record.
- Shared polynomial-fit solver (`poly_fit`/`poly_value`) since both tabs need
  one; new unit tests for the fitter, day-of-year math, and season lengths.
- Tracked the project display name in `.idea/.name`.

## [2026-08-23]

### Added
- **Moving average overlay** on the Daily and Trends views: a dashed, centered
  rolling mean per station (polars `rolling_mean` with `.over()`), controlled
  by a "Moving avg" number input.

## [2026-08-16]

Initial release.

### Added
- `pull_data/`: NOAA CDO fetch tool — pages the full daily record (all
  reported datatypes, no `datatypeid` filter) per station into a tidy CSV,
  staying under the API's rate limits.
- `cleanup_data/`: combines the per-station CSVs into
  `weather_all.parquet` under `~/.config/RobertBicknell/Weatheranalysis/`
  (override with `WEATHER_DATA_PATH`).
- `plot_data/`: Tauri 2 + Plotly.js viewer with **Daily** lines (raw plus
  low-pass filter overlay) and **Trends** (monthly/yearly mean-temperature,
  `(TMAX+TMIN)/2`) tabs, plus a zero-phase exponential low-pass filter
  (`ewm_mean` forward + backward per station).
- Vendored Plotly.js so the app needs no internet at runtime.
- READMEs for the pipeline and the viewer.