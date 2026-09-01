# Changelog

All notable changes to this project are documented in this file, loosely
following [Keep a Changelog](https://keepachangelog.com/). Dates are the
commit date; `Unreleased` covers work in the working tree not yet committed.

## [Unreleased]

### Fixed
- **CI audit jobs failing on warnings**: `rustsec/audit-check@v2` runs
  `cargo audit --deny warnings`, which force-enables informational
  advisories — the v0.2.1 audit jobs were red on exactly this. Each crate's
  `.cargo/audit.toml` now ignores the informational advisories it truly
  can't upgrade past (bincode, the Tauri GTK/glib/unic stack in `src-tauri`)
  by ID, disables the ID-less `chacha20` (yanked, via `rand`) check, and the
  pre-push hook now mirrors CI by running `--deny warnings` too.

## [0.2.2] - 2026-08-30

### Changed
- **Polars 0.44 -> 0.55** in `cleanup_data` and `plot_data/data-core` (the
  desktop app and web server pick it up transitively through `data-core`).
  Fixes driven by the new API: `PlRefPath` for `scan_parquet`/`LazyCsvReader`,
  `Expr::over` now returning `PolarsResult` (propagated through the
  rolling-mean / low-pass helpers), numeric-only `into_no_null_iter`, and
  `DataFrame::new(height, columns)`. This clears the earlier TODO to revisit
  on the next polars upgrade.
- **Audit suppressions trimmed**: the polars bump drops fast-float from the
  tree and moves pyo3 0.21 -> 0.39, clearing RUSTSEC-2025-0003, 2025-0020 and
  2026-0177. The two quick-xml entries (RUSTSEC-2026-0194/0195) remain:
  polars still pins quick-xml 0.39 (< the 0.41 fix).

## [0.2.1] - 2026-08-30

### Added
- **`pull_data` test coverage**: pagination/retry internals factored into a
  `PullConfig` (base URL + sleep timing) so tests can drive `fetch_year`
  against a mock server; `mockito`-based suites covering page-boundary
  offsets, retry-on-rate-limit, error propagation, and empty results.
- **CI backstop** (`.github/workflows/ci.yml`): GitHub Actions mirror of the
  pre-push hook — `cargo fmt --check`, `cargo clippy --all-targets -- -D
  warnings`, `cargo test`, and `cargo audit` for every crate on push to `main`
  and PRs (a safety net for pushes made without the local hook).
- **`LICENSE`** (MIT).

### Changed
- **Public deploy**: automatic HTTPS via Caddy — the site block switched from
  plain `http://` to a DuckDNS-certified HTTPS site; `deploy/README.md` and
  `install.sh` updated accordingly.
- **Audit suppressions**: the nullified polars transitive advisories in each
  crate's `.cargo/audit.toml` annotated with the dependency-fix version each
  can be cleared at on the next upgrade.

## [0.2.0] - 2026-08-30

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

[Unreleased]: https://github.com/BobBicknell/CorvallisWeather/compare/v0.2.2...HEAD
[0.2.2]: https://github.com/BobBicknell/CorvallisWeather/releases/tag/v0.2.2
[0.2.1]: https://github.com/BobBicknell/CorvallisWeather/releases/tag/v0.2.1
[0.2.0]: https://github.com/BobBicknell/CorvallisWeather/releases/tag/v0.2.0