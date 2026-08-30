# WeatherAnalysis

Historical daily weather for Oregon cities — Corvallis, Eugene, Salem —
fetched from NOAA's Climate Data Online (CDO) API, combined into a single
Parquet file, and visualized as a Tauri desktop app **and** as a web app
served from this machine.

Public web app: `http://bicknellfamily.duckdns.org/CorvallisWeather/`

## Repository layout

| Path | What it is |
|---|---|
| `pull_data/` | Fetches the full daily record per station from NOAA CDO → CSV |
| `cleanup_data/` | Combines the per-station CSVs into `weather_all.parquet` |
| `plot_data/data-core/` | Shared query layer over the Parquet (used by the desktop app and the web server) |
| `plot_data/src-tauri/` | Tauri desktop app (thin command wrappers + frontend) |
| `plot_data/server/` | Axum web server exposing the same queries over REST |
| `plot_data/src/` | Static frontend, shared verbatim by desktop and web |
| `deploy/` | systemd unit + Caddy config + install scripts for the public web app |

More detail:
- Query core, desktop app, web server, and frontend: [plot_data/README.md](plot_data/README.md)
- Public deployment on this machine: [deploy/README.md](deploy/README.md)
- Release history: [CHANGELOG.md](CHANGELOG.md)

## Development

A git pre-push hook (`hooks/pre-push`) runs `cargo audit`, `cargo fmt
--check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` in
every crate and aborts the push if any fail. Same pattern as the RustyMealie
repo; enable it with:

```bash
git config core.hooksPath hooks
```

`cargo audit` blocks on real advisories. A few unfixable transitive ones
(through polars) are nullified in each crate's `.cargo/audit.toml` with a
`# TODO` comment — re-check them when polars gets upgraded.

## Data

- **Stations**

  | City | Station | GHCND ID | Period of record |
  |---|---|---|---|
  | Corvallis | Corvallis State University | `GHCND:USC00351862` | 1893-01-01 to present |
  | Eugene | Mahlon Sweet Field (airport) | `GHCND:USW00024221` | 1938-06-01 to present |
  | Salem | McNary Field (airport) | `GHCND:USW00024232` | 1892-12-01 to present |

  The airport stations (Eugene, Salem) are ASOS sites and more likely to carry
  sunshine/cloud-cover fields (PSUN, ACMH) than Corvallis' manual COOP station.

- **Fields:** everything a station reports — TMAX, TMIN, PRCP, and whatever
  else is available (SNOW, SNWD, TOBS, WT## weather-type flags, ASOS
  sunshine/cloud fields, ...). No `datatypeid` filter is applied.
- **Format:** long/tidy CSV rows, one per `(date, datatype, station, value)`;
  `cleanup_data` combines these into Parquet.
- **Location:** `weather_all.parquet` at
  `~/.config/RobertBicknell/Weatheranalysis/weather_all.parquet` (override
  with `WEATHER_DATA_PATH`). Same file feeds the desktop app, the web server,
  and the same location `cleanup_data` writes to.

## Pulling data (`pull_data`)

The CDO API caps requests at 1 year of data and 1000 records per call, and
rate-limits to 5 req/sec / 10,000 req/day. The tool pages within each year
and sleeps briefly between calls to stay well under those limits.

### Setup

1. Get a free CDO API token: https://www.ncdc.noaa.gov/cdo-web/token
2. Set it as an env var:
   ```
   export NOAA_TOKEN="your_token_here"
   ```
3. Run:
   ```
   cargo run --release
   ```
4. To switch which station it fetches, edit the `STATION_ID`,
   `STATION_START_YEAR`, and `OUTFILE` constants at the top of
   `pull_data/src/main.rs`.

Output is written to the configured CSV in the working directory.

A Python version of the same fetch logic (100-year window, TMAX/TMIN/PRCP
only) also exists as a starting point if you'd rather iterate in Python.