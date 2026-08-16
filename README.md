# corvallis_weather

Pulls the full available daily weather record for Corvallis, OR from NOAA's
Climate Data Online (CDO) API v2, and writes it to CSV.

- **Station:** `GHCND:USC00351862` (Corvallis State University, OR)
- **Period of record:** 1893-01-01 to present
- **Fields:** everything the station reports (TMAX, TMIN, PRCP, and whatever
  else is available — SNOW, SNWD, TOBS, WT## weather-type flags, etc.). No
  `datatypeid` filter is applied.

Output is long/tidy format: one row per `(date, datatype, station, value)`.

## Setup

1. Get a free CDO API token: https://www.ncdc.noaa.gov/cdo-web/token
2. Set it as an env var:
   ```
   export NOAA_TOKEN="your_token_here"
   ```
3. Run:
   ```
   cargo run --release
   ```

Output is written to `corvallis_weather_full.csv` in the working directory.

## Notes

- The CDO API caps requests at 1 year of data and 1000 records per call, and
  rate-limits to 5 req/sec / 10,000 req/day. This tool pages within each
  year and sleeps briefly between calls to stay well under those limits.
- A Python version of the same fetch logic (100-year window, TMAX/TMIN/PRCP
  only) also exists as a starting point if you'd rather iterate in Python.

## Nearby stations

If you want to compare against or swap in a different station:

| City | Station | GHCND ID | Period of record |
|---|---|---|---|
| Corvallis | Corvallis State University | `GHCND:USC00351862` | 1893-01-01 to present |
| Eugene | Mahlon Sweet Field (airport) | `GHCND:USW00024221` | 1938-06-01 to present |
| Salem | McNary Field (airport) | `GHCND:USW00024232` | 1892-12-01 to present |

The airport stations (Eugene, Salem) are ASOS sites and more likely to carry
sunshine/cloud-cover fields (PSUN, ACMH) than Corvallis' manual COOP station.
