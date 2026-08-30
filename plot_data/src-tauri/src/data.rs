//! Query layer over the combined weather Parquet file. Deliberately has no
//! Tauri dependency -- these are plain functions returning serde-serializable
//! structs, so the same code could back a REST endpoint (e.g. an Axum
//! server) later without changes, just a different caller than main.rs.

use polars::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

/// Friendly names for known stations. Extend this as pull_data adds more.
fn station_names() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("GHCND:USC00351862", "Corvallis"),
        ("GHCND:USW00024221", "Eugene"),
        ("GHCND:USW00024232", "Salem"),
    ])
}

#[derive(Serialize, Clone)]
pub struct StationInfo {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct SeriesPoint {
    pub date: String,
    pub value: f64,
    pub avg: Option<f64>,
    pub lpf: Option<f64>,
}

#[derive(Serialize)]
pub struct StationSeries {
    pub station_id: String,
    pub station_name: String,
    pub points: Vec<SeriesPoint>,
}

/// Resolve the path to weather_all.parquet. Checks WEATHER_DATA_PATH env var
/// first (set this to override, or for a future web server deployment).
/// Otherwise defaults to ~/.config/RobertBicknell/Weatheranalysis/weather_all.parquet
/// (via dirs::config_dir(), so this also works on macOS/Windows) -- the same
/// location cleanup_data writes to, so the two stay in sync without either
/// one depending on the project's checkout location.
fn data_path() -> PathBuf {
    if let Ok(p) = env::var("WEATHER_DATA_PATH") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .expect("could not resolve config directory")
        .join("RobertBicknell")
        .join("Weatheranalysis")
        .join("weather_all.parquet")
}

/// Public wrapper so main.rs can print the resolved path at startup.
pub fn resolved_data_path() -> PathBuf {
    data_path()
}

fn load_df() -> PolarsResult<LazyFrame> {
    LazyFrame::scan_parquet(data_path(), ScanArgsParquet::default())
}

/// Adds an "avg_value" column: a centered rolling mean of "value" over
/// `window` rows, computed independently per "station". Requires the frame
/// already be sorted by (station, date/period) -- both callers below are.
/// `center: true` avoids the phase lag a trailing average would introduce,
/// appropriate here since this is historical (not live/streaming) data.
/// `min_periods: 1` keeps the line defined at the edges instead of gapping.
fn with_rolling_avg(lf: LazyFrame, window: i64) -> LazyFrame {
    lf.with_column(
        col("value")
            .rolling_mean(RollingOptionsFixedWindow {
                window_size: window as usize,
                min_periods: 1,
                weights: None,
                center: true,
                fn_params: None,
            })
            .over([col("station")])
            .alias("avg_value"),
    )
}

/// Adds an "lpf_value" column: a zero-phase exponential low-pass filter of
/// "value" (first-order IIR, applied forward then backward). `span` is the
/// EMA span in rows: alpha = 2/(span+1), so a span behaves like a moving
/// average of similar length scale. The backward pass cancels the phase lag
/// a one-sided EMA would introduce -- same reasoning as center:true above.
/// Like with_rolling_avg, requires sorting by (station, date) and uses
/// `.over()` per station; min_periods 1 keeps edges defined. adjust:false
/// makes both passes identical recursions (seeded with their own endpoint),
/// so the composite response is exactly symmetric; each edge degrades into
/// a plain one-sided EMA anchored at its end of the series.
fn with_low_pass(lf: LazyFrame, span: i64) -> LazyFrame {
    let options = EWMOptions {
        alpha: 2.0 / (span as f64 + 1.0),
        adjust: false,
        bias: false,
        min_periods: 1,
        ignore_nulls: true,
    };
    lf.with_column(
        col("value")
            .ewm_mean(options)
            .reverse()
            .ewm_mean(options)
            .reverse()
            .over([col("station")])
            .alias("lpf_value"),
    )
}

/// All distinct stations present in the data, with friendly names where known.
pub fn get_stations() -> PolarsResult<Vec<StationInfo>> {
    let names = station_names();
    let df = load_df()?
        .select([col("station")])
        .unique(None, UniqueKeepStrategy::First)
        .collect()?;

    let station_col = df.column("station")?.str()?;
    let mut stations: Vec<StationInfo> = station_col
        .into_no_null_iter()
        .map(|id| StationInfo {
            id: id.to_string(),
            name: names.get(id).unwrap_or(&id).to_string(),
        })
        .collect();
    stations.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(stations)
}

/// All distinct datatype codes present (TMAX, TMIN, PRCP, and anything else
/// pull_data has fetched -- SNOW, SNWD, future sunshine/cloud fields, etc.).
pub fn get_datatypes() -> PolarsResult<Vec<String>> {
    let df = load_df()?
        .select([col("datatype")])
        .unique(None, UniqueKeepStrategy::First)
        .collect()?;

    let dt_col = df.column("datatype")?.str()?;
    let mut types: Vec<String> = dt_col.into_no_null_iter().map(String::from).collect();
    types.sort();
    Ok(types)
}

/// Time series for one datatype, optionally filtered to a single station.
/// When `station` is None ("All"), returns one series per station so the
/// caller can plot them as separate traces.
pub fn get_series(
    datatype: &str,
    station: Option<&str>,
    window: Option<i64>,
    low_pass: Option<i64>,
) -> PolarsResult<Vec<StationSeries>> {
    let mut lf = load_df()?.filter(col("datatype").eq(lit(datatype)));
    if let Some(s) = station {
        lf = lf.filter(col("station").eq(lit(s)));
    }

    let mut lf = lf
        .select([col("station"), col("date"), col("value")])
        .sort(["station", "date"], SortMultipleOptions::default());
    if let Some(w) = window {
        if w > 1 {
            lf = with_rolling_avg(lf, w);
        }
    }
    if let Some(s) = low_pass {
        if s > 1 {
            lf = with_low_pass(lf, s);
        }
    }

    let df = lf.collect()?;

    group_into_series(&df, "date")
}

/// Monthly or yearly average of (TMAX + TMIN) / 2 -- a much smoother trend
/// line than plotting daily TMAX/TMIN directly, which is dominated by
/// day-to-day noise. `period` is "monthly" or "yearly"; anything else
/// defaults to monthly. Optionally filtered to a single station.
pub fn get_mean_temp_trend(
    period: &str,
    station: Option<&str>,
    window: Option<i64>,
    low_pass: Option<i64>,
) -> PolarsResult<Vec<StationSeries>> {
    let base = load_df()?;

    let mut tmax = base
        .clone()
        .filter(col("datatype").eq(lit("TMAX")))
        .select([col("station"), col("date"), col("value").alias("tmax")]);
    let mut tmin = base
        .filter(col("datatype").eq(lit("TMIN")))
        .select([col("station"), col("date"), col("value").alias("tmin")]);

    if let Some(s) = station {
        tmax = tmax.filter(col("station").eq(lit(s)));
        tmin = tmin.filter(col("station").eq(lit(s)));
    }

    // Dates are stored as "YYYY-MM-DD"; slice to "YYYY" for yearly buckets
    // or "YYYY-MM" for monthly buckets.
    let period_len: i64 = if period == "yearly" { 4 } else { 7 };

    let mut lf = tmax
        .join(
            tmin,
            [col("station"), col("date")],
            [col("station"), col("date")],
            JoinArgs::new(JoinType::Inner),
        )
        .with_columns([
            ((col("tmax") + col("tmin")) / lit(2.0)).alias("mean_temp"),
            col("date").str().slice(lit(0), lit(period_len)).alias("period"),
        ])
        .group_by([col("station"), col("period")])
        .agg([col("mean_temp").mean().alias("value")])
        .sort(["station", "period"], SortMultipleOptions::default());
    if let Some(w) = window {
        if w > 1 {
            lf = with_rolling_avg(lf, w);
        }
    }
    if let Some(s) = low_pass {
        if s > 1 {
            lf = with_low_pass(lf, s);
        }
    }

    let df = lf.collect()?;

    group_into_series(&df, "period")
}

/// Number of days per year where TMAX exceeds `threshold` (in the same units
/// as the data: degrees Fahrenheit here), plus a least-squares quadratic fit
/// of that count over the modern record (>= 1980). Returns both sets as
/// StationSeries: `days` covers the full record, `fit` one fitted curve per
/// station whose x values are years and whose value is the fitted count.
pub fn get_hot_days_per_year(
    threshold: f64,
    station: Option<&str>,
) -> PolarsResult<HotDaysResult> {
    let days = hot_day_counts(threshold, station)?;

    let fit = days
        .iter()
        .map(|s| {
            let mut pts: Vec<(f64, f64)> = s
                .points
                .iter()
                .filter_map(|p| {
                    let year: f64 = p.date.parse().ok()?;
                    (year >= FIT_START_YEAR).then_some((year, p.value))
                })
                .collect();
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));

            let mut points = Vec::new();
            if let Some((center, coeffs)) = poly_fit(&pts, 2) {
                for (year, _) in &pts {
                    points.push(SeriesPoint {
                        date: year.to_string(),
                        value: poly_value(&coeffs, center, *year),
                        avg: None,
                        lpf: None,
                    });
                }
            }
            StationSeries {
                station_id: s.station_id.clone(),
                station_name: s.station_name.clone(),
                points,
            }
        })
        .collect();

    Ok(HotDaysResult { days, fit })
}

fn hot_day_counts(threshold: f64, station: Option<&str>) -> PolarsResult<Vec<StationSeries>> {
    let mut lf = load_df()?
        .filter(col("datatype").eq(lit("TMAX")))
        .select([col("station"), col("date"), col("value")]);

    if let Some(s) = station {
        lf = lf.filter(col("station").eq(lit(s)));
    }

    let lf = lf
        .filter(col("value").gt(lit(threshold)))
        .with_column(col("date").str().slice(lit(0), lit(4)).alias("year"))
        .group_by([col("station"), col("year")])
        .agg([col("value").count().cast(DataType::Float64).alias("value")])
        .sort(["station", "year"], SortMultipleOptions::default());

    let df = lf.collect()?;

    group_into_series(&df, "year")
}

/// Year threshold for the hot-days quadratic fit ("modern" record).
const FIT_START_YEAR: f64 = 1980.0;

/// Least-squares polynomial fit of `degree` (1 = line, 2 = quadratic,
/// 3 = cubic, ...) to the given (x, y) points, where x is a year. Returns
/// the coefficients of y = c[0] + c[1]*t + c[2]*t^2 + ... with t = x - center
/// and center the mean x, plus the center itself. Years are centered at
/// their mean before solving the normal equations to keep the system
/// well-conditioned (raw years ~2000 give x^4 ~ 1.6e13). Returns None when
/// degenerate: fewer than degree+1 distinct points, or a (numerically)
/// singular normal matrix -- e.g. all x identical.
fn poly_fit(points: &[(f64, f64)], degree: usize) -> Option<(f64, Vec<f64>)> {
    let order = degree + 1;
    if points.len() < order {
        return None;
    }
    let center = points.iter().map(|(x, _)| x).sum::<f64>() / points.len() as f64;

    // Normal equations: M c = rhs with M[i][j] = sum of t^(i+j) and
    // rhs[i] = sum of y * t^i over the centered samples.
    let mut m = vec![vec![0.0; order]; order];
    let mut rhs = vec![0.0; order];
    let mut powers = vec![1.0; 2 * order];
    for (x, y) in points {
        let t = x - center;
        for i in 1..powers.len() {
            powers[i] = powers[i - 1] * t;
        }
        for i in 0..order {
            rhs[i] += y * powers[i];
            for (j, row) in m.iter_mut().enumerate() {
                row[i] += powers[i + j];
            }
        }
    }

    // Solve by Gauss-Jordan elimination with partial pivoting.
    for col in 0..order {
        let mut piv = col;
        for row in col + 1..order {
            if m[row][col].abs() > m[piv][col].abs() {
                piv = row;
            }
        }
        if m[piv][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, piv);
        rhs.swap(col, piv);
        let inv = 1.0 / m[col][col];
        for entry in m[col][col..order].iter_mut() {
            *entry *= inv;
        }
        rhs[col] *= inv;
        for row in 0..order {
            if row != col {
                let factor = m[row][col];
                if factor != 0.0 {
                    let normalized = m[col][col..order].to_vec();
                    for (target, n) in m[row][col..order].iter_mut().zip(normalized.iter()) {
                        *target -= factor * n;
                    }
                    rhs[row] -= factor * rhs[col];
                }
            }
        }
    }

    Some((center, rhs))
}

/// Evaluate a centered polynomial (from [`poly_fit`]) at `x` by Horner's rule.
fn poly_value(coeffs: &[f64], center: f64, x: f64) -> f64 {
    let t = x - center;
    coeffs.iter().rev().fold(0.0, |acc, c| acc * t + c)
}

/// Outcome of [`get_hot_days_per_year`]: the per-year counts plus the
/// quadratic fit over the modern record, one series per station each.
#[derive(Serialize)]
pub struct HotDaysResult {
    pub days: Vec<StationSeries>,
    pub fit: Vec<StationSeries>,
}

/// Outcome of [`get_growing_season`]: the per-year season lengths plus the
/// cubic fit over the full record, one series per station each.
#[derive(Serialize)]
pub struct GrowingSeasonResult {
    pub days: Vec<StationSeries>,
    pub fit: Vec<StationSeries>,
}

/// Temperature (°F) at or below which a day counts as a frost day (TMIN).
const FREEZE_TEMP: f64 = 32.0;

/// Month window (1-based, inclusive) bounding the "last spring frost".
/// Frosts before March are mid-winter, not spring -- excluding them keeps
/// years with incomplete records (e.g. an early station logging only a
/// January and a December frost) from producing a bogus year-long season.
const SPRING_WINDOW: std::ops::RangeInclusive<u32> = 3..=6;

/// Month window (1-based, inclusive) bounding the "first fall frost".
const FALL_WINDOW: std::ops::RangeInclusive<u32> = 7..=11;

/// Growing-season accumulator: station -> year -> (last spring frost
/// day-of-year, first fall frost day-of-year).
type SeasonAccumulator = HashMap<String, HashMap<u32, (Option<f64>, Option<f64>)>>;

/// Growing-season length per station plus a cubic least-squares fit of it
/// over the full record: the number of days between the last spring frost and
/// the first fall frost of each year, where a frost day is one whose low
/// (TMIN) fell to 32 °F or below. Frosts are dated by month: spring frosts
/// fall in March-June, fall frosts in July-November. `days` holds one
/// StationSeries per station whose x values are years and whose value is the
/// season length in days (years lacking a frost in either window are
/// omitted); `fit` holds the corresponding cubic trend line.
pub fn get_growing_season(station: Option<&str>) -> PolarsResult<GrowingSeasonResult> {
    let mut lf = load_df()?
        .filter(col("datatype").eq(lit("TMIN")))
        .filter(col("value").lt_eq(lit(FREEZE_TEMP)))
        .select([col("station"), col("date")]);
    if let Some(s) = station {
        lf = lf.filter(col("station").eq(lit(s)));
    }

    let df = lf.collect()?;
    let station_col = df.column("station")?.str()?;
    let date_col = df.column("date")?.str()?;

    let mut records = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        if let (Some(st), Some(d)) = (station_col.get(i), date_col.get(i)) {
            records.push((st.to_string(), d.to_string()));
        }
    }

    let names = station_names();
    let seasons = season_lengths(&records);

    let mut result: Vec<StationSeries> = seasons
        .into_iter()
        .map(|(id, mut years)| {
            years.sort_by_key(|(y, _)| *y);
            StationSeries {
                station_id: id.clone(),
                station_name: names.get(id.as_str()).unwrap_or(&id.as_str()).to_string(),
                points: years
                    .into_iter()
                    .map(|(y, len)| SeriesPoint {
                        date: y.to_string(),
                        value: len,
                        avg: None,
                        lpf: None,
                    })
                    .collect(),
            }
        })
        .collect();
    result.retain(|s| !s.points.is_empty());
    result.sort_by(|a, b| a.station_name.cmp(&b.station_name));

    // Cubic least-squares fit of the season length over the full record,
    // one fitted curve per station (mirrors the hot-days quadratic fit).
    let fit = result
        .iter()
        .map(|s| {
            let mut pts: Vec<(f64, f64)> = s
                .points
                .iter()
                .filter_map(|p| p.date.parse::<f64>().ok().map(|y| (y, p.value)))
                .collect();
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));

            let mut points = Vec::new();
            if let Some((center, coeffs)) = poly_fit(&pts, 3) {
                for (year, _) in &pts {
                    points.push(SeriesPoint {
                        date: year.to_string(),
                        value: poly_value(&coeffs, center, *year),
                        avg: None,
                        lpf: None,
                    });
                }
            }
            StationSeries {
                station_id: s.station_id.clone(),
                station_name: s.station_name.clone(),
                points,
            }
        })
        .collect();

    Ok(GrowingSeasonResult { days: result, fit })
}

/// Pure helper: given (station, date) rows for freezing days, compute each
/// station's growing-season length per year. Spring frosts are freezing days
/// in March-June; fall frosts those in July-November. A year contributes
/// only when it has both a spring and a fall frost (and the fall comes after
/// the spring).
fn season_lengths(records: &[(String, String)]) -> HashMap<String, Vec<(u32, f64)>> {
    // station -> year -> (last spring frost doy, first fall frost doy)
    let mut by_year: SeasonAccumulator = HashMap::new();
    for (station, date) in records {
        let Some((year, month, doy)) = day_of_year(date) else { continue };
        let in_spring = SPRING_WINDOW.contains(&month);
        let in_fall = FALL_WINDOW.contains(&month);
        if !in_spring && !in_fall {
            continue; // mid-winter frosts don't bound the growing season
        }
        let entry = by_year
            .entry(station.clone())
            .or_default()
            .entry(year)
            .or_insert((None, None));
        let doy_f = doy as f64;
        if in_fall {
            entry.1 = Some(match entry.1 {
                Some(v) => v.min(doy_f),
                None => doy_f,
            });
        } else {
            entry.0 = Some(match entry.0 {
                Some(v) => v.max(doy_f),
                None => doy_f,
            });
        }
    }

    by_year
        .into_iter()
        .map(|(station, years)| {
            let mut result = Vec::new();
            for (year, (spring, fall)) in years {
                if let (Some(s), Some(f)) = (spring, fall) {
                    if f > s {
                        result.push((year, f - s));
                    }
                }
            }
            (station, result)
        })
        .collect()
}

/// Day-of-year in [1, 366] for a "YYYY-MM-DD" string, plus the year and
/// month (computing the ordinal day needs only the calendar, not datetime
/// support from polars).
fn day_of_year(date: &str) -> Option<(u32, u32, u32)> {
    if date.len() < 10 {
        return None;
    }
    let year: u32 = date[0..4].parse().ok()?;
    let month: u32 = date[5..7].parse().ok()?;
    let day: u32 = date[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    const CUMDAYS: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = is_leap_year(year) && month > 2;
    Some((year, month, CUMDAYS[(month - 1) as usize] + day + leap as u32))
}

/// Gregorian leap-year rule.
fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// Shared helper: turns a DataFrame with columns [station, <date_col>, value]
/// into one StationSeries per distinct station.
fn group_into_series(df: &DataFrame, date_col_name: &str) -> PolarsResult<Vec<StationSeries>> {
    let names = station_names();

    let station_col = df.column("station")?.str()?;
    let date_col = df.column(date_col_name)?.str()?;
    let value_col = df.column("value")?.f64()?;
    let avg_col = df.column("avg_value").ok().and_then(|c| c.f64().ok());
    let lpf_col = df.column("lpf_value").ok().and_then(|c| c.f64().ok());

    let mut grouped: HashMap<String, Vec<SeriesPoint>> = HashMap::new();
    for i in 0..df.height() {
        let st = station_col.get(i).unwrap_or_default().to_string();
        let date = date_col.get(i).unwrap_or_default().to_string();
        let value = value_col.get(i).unwrap_or(f64::NAN);
        let avg = avg_col.and_then(|c| c.get(i));
        let lpf = lpf_col.and_then(|c| c.get(i));
        grouped.entry(st).or_default().push(SeriesPoint { date, value, avg, lpf });
    }

    let mut result: Vec<StationSeries> = grouped
        .into_iter()
        .map(|(id, points)| {
            let name = names.get(id.as_str()).unwrap_or(&id.as_str()).to_string();
            StationSeries {
                station_id: id,
                station_name: name,
                points,
            }
        })
        .collect();
    result.sort_by(|a, b| a.station_name.cmp(&b.station_name));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(stations: Vec<&str>, values: Vec<f64>) -> LazyFrame {
        let n = values.len();
        DataFrame::new(vec![
            Column::new("station".into(), stations),
            Column::new(
                "date".into(),
                (0..n).map(|i| format!("2000-01-{:02}", i + 1)).collect::<Vec<_>>(),
            ),
            Column::new("value".into(), values),
        ])
        .unwrap()
        .lazy()
    }

    fn lpf_column(stations: Vec<&str>, values: Vec<f64>, span: i64) -> Vec<Option<f64>> {
        let df = with_low_pass(frame(stations, values), span).collect().unwrap();
        df.column("lpf_value")
            .unwrap()
            .f64()
            .unwrap()
            .into_iter()
            .collect()
    }

    #[test]
    fn constant_series_pass_through_unchanged_per_station() {
        let got = lpf_column(vec!["a", "a", "a", "b", "b", "b"],
                             vec![10.0, 10.0, 10.0, 20.0, 20.0, 20.0], 3);
        assert_eq!(got[..3], vec![Some(10.0); 3][..]);
        assert_eq!(got[3..], vec![Some(20.0); 3]);
    }

    #[test]
    fn forward_backward_pass_is_zero_phase() {
        // An impulse must remain at its original index -- proof the backward
        // pass cancels the EMA's phase lag.
        let got = lpf_column(vec!["a"; 21],
                             vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                                  1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                                  0.0, 0.0], 3);
        let peak = got.iter()
            .enumerate()
            .max_by(|a, b| a.1.unwrap().total_cmp(&b.1.unwrap()))
            .unwrap().0;
        assert_eq!(peak, 10);
        // Response decays symmetrically around the peak -- adjust:false makes
        // both passes identical recursions, so any residual is the EWM
        // kernel's own float accumulation. Compare against peak height, not
        // per-sample size: tail samples are near zero and would inflate any
        // relative-error metric.
        let peak_val = got[peak].unwrap();
        for d in 1..6 {
            let l = got[10 - d].unwrap();
            let r = got[10 + d].unwrap();
            assert!(
                (l - r).abs() < 1e-4 * peak_val,
                "asymmetry at d={d}: {l} vs {r}"
            );
        }
    }

    #[test]
    fn edges_are_defined() {
        let got = lpf_column(vec!["a"; 5], vec![1.0, 2.0, 3.0, 4.0, 5.0], 4);
        assert!(got.iter().all(|v| v.is_some()));
    }

    #[test]
    fn real_data_end_to_end() {
        if !resolved_data_path().exists() {
            return;
        }
        let series = get_series("TMAX", None, Some(30), Some(365)).unwrap();
        assert!(!series.is_empty());
        for s in &series {
            assert!(s.points.iter().all(|p| p.lpf.is_some()));
            // Filtered series must never exceed the record max by much or
            // dip below the record min: sanity on scale.
            let (min, max) = s.points.iter()
                .map(|p| p.value)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
            for p in &s.points {
                let l = p.lpf.unwrap();
                assert!(l >= min - 25.0 && l <= max + 25.0, "lpf out of range: {l}");
            }
        }
    }

    #[test]
    fn hot_days_per_year_real_data() {
        if !resolved_data_path().exists() {
            return;
        }
        // 90°F is a typical heat-wave threshold; counts must be non-zero for
        // at least one summer-leaning year somewhere, and bounded by 366.
        let res = get_hot_days_per_year(90.0, None).unwrap();
        assert!(!res.days.is_empty());
        for s in &res.days {
            for p in &s.points {
                assert!(p.value >= 0.0 && p.value <= 366.0, "implausible count: {}", p.value);
                assert!(p.date.len() == 4, "expected year, got {}", p.date);
            }
        }
        let total: f64 = res.days.iter().flat_map(|s| s.points.iter().map(|p| p.value)).sum();
        assert!(total > 0.0, "no days above 90°F found in the whole record");

        // Every station with data must get a fit over the modern record --
        // all three stations predate 1980, so each should have >= 3 points.
        assert_eq!(res.fit.len(), res.days.len());
        for f in &res.fit {
            assert!(f.points.len() >= 3, "fit too short for {}", f.station_name);
            for p in &f.points {
                assert!(p.date.parse::<f64>().unwrap() >= FIT_START_YEAR);
            }
        }
    }

    #[test]
    fn poly_fit_reproduces_parabola() {
        // y = 5 - 2x + 0.5x^2 sampled over x in 1..=10 must come back out.
        let pts: Vec<(f64, f64)> = (1..=10)
            .map(|x| {
                let x = x as f64;
                (x, 5.0 - 2.0 * x + 0.5 * x * x)
            })
            .collect();
        let (center, c) = poly_fit(&pts, 2).unwrap();
        for (x, y) in &pts {
            let got = poly_value(&c, center, *x);
            assert!((got - y).abs() < 1e-9, "fit at x={x}: {got} vs {y}");
        }
    }

    #[test]
    fn poly_fit_reproduces_cubic() {
        // y = x^3 - 2x^2 + 0.5x + 3 sampled over x in 1..=20 must come back
        // out (degree-3 fit, the growing-season default).
        let pts: Vec<(f64, f64)> = (1..=20)
            .map(|x| {
                let x = x as f64;
                (x, x * x * x - 2.0 * x * x + 0.5 * x + 3.0)
            })
            .collect();
        let (center, c) = poly_fit(&pts, 3).unwrap();
        for (x, y) in &pts {
            let got = poly_value(&c, center, *x);
            assert!((got - y).abs() < 1e-8, "cubic fit at x={x}: {got} vs {y}");
        }
    }

    #[test]
    fn poly_fit_needs_enough_points() {
        assert!(poly_fit(&[(2000.0, 5.0), (2001.0, 6.0)], 2).is_none());
        assert!(poly_fit(&[], 0).is_none());
    }

    #[test]
    fn day_of_year_counts_days_and_leap_years() {
        assert_eq!(day_of_year("2000-01-01").unwrap(), (2000, 1, 1));
        // Non-leap-year ordinal days.
        assert_eq!(day_of_year("2001-02-28").unwrap().2, 59);
        assert_eq!(day_of_year("2001-03-01").unwrap().2, 60);
        assert_eq!(day_of_year("2001-12-31").unwrap().2, 365);
        // Leap year adds a day from March onward.
        assert_eq!(day_of_year("2000-02-28").unwrap().2, 59);
        assert_eq!(day_of_year("2000-02-29").unwrap().2, 60);
        assert_eq!(day_of_year("2000-03-01").unwrap().2, 61);
        assert_eq!(day_of_year("2000-12-31").unwrap().2, 366);
        // Century rule: 1900 is not a leap year, 2000 is.
        assert_eq!(day_of_year("1900-03-01").unwrap().2, 60);
        assert_eq!(day_of_year("2000-03-01").unwrap().2, 61);
        // Garbage in, None out.
        assert!(day_of_year("not-a-date").is_none());
        assert!(day_of_year("2000-13-01").is_none());
    }

    #[test]
    fn season_lengths_from_frost_days() {
        // 2000 (leap): Apr 15 = doy 106, Oct 1 = doy 275 -> 169 days.
        // A later fall frost in the same year is ignored (first fall frost).
        let records = vec![
            ("a".to_string(), "2000-04-15".to_string()), // spring
            ("a".to_string(), "2000-10-01".to_string()), // fall
            ("a".to_string(), "2000-11-05".to_string()), // later fall, ignored
            ("a".to_string(), "2000-01-10".to_string()), // winter frost, not a spring frost
            // A year with no fall frost is omitted entirely.
            ("a".to_string(), "1999-05-01".to_string()),
            // A second station gets its own series.
            ("b".to_string(), "2000-04-01".to_string()),
            ("b".to_string(), "2000-09-15".to_string()),
            // Frosts only in mid-winter (Feb) / December are outside the
            // growing-season windows, so these years don't count -- the
            // artifacts that a partial station record would otherwise create.
            ("c".to_string(), "1901-01-01".to_string()),
            ("c".to_string(), "1901-12-12".to_string()),
            ("c".to_string(), "1915-02-15".to_string()),
            ("c".to_string(), "1915-12-29".to_string()),
            // The old 1901/1915 artifact pattern must NOT appear.
            ("d".to_string(), "1901-04-01".to_string()), // spring OK...
            ("d".to_string(), "1901-09-15".to_string()), // ...and fall OK -> real season
        ];
        let seasons = season_lengths(&records);
        let a = seasons.get("a").unwrap();
        assert_eq!(a, &vec![(2000u32, 169.0)]);
        // day_of_year(2000-04-01) = 92, day_of_year(2000-09-15) = 259 -> 167.
        let b = seasons.get("b").unwrap();
        assert_eq!(b, &vec![(2000u32, 167.0)]);
        // Winter-only frosts never form a season.
        assert!(!seasons.contains_key("c"));
        // A genuinely complete (if sparse) season still counts.
        let d = seasons.get("d").unwrap();
        assert_eq!(d, &vec![(1901u32, 167.0)]);
    }

    #[test]
    fn growing_season_real_data() {
        if !resolved_data_path().exists() {
            return;
        }
        let res = get_growing_season(None).unwrap();
        assert!(!res.days.is_empty());
        for s in &res.days {
            assert!(!s.points.is_empty(), "no seasons for {}", s.station_name);
            for p in &s.points {
                assert!(p.date.len() == 4, "expected year, got {}", p.date);
                // A plausible Pacific-Northwest growing season is ~100-260 days.
                assert!(
                    p.value >= 60.0 && p.value <= 300.0,
                    "implausible season length {} for {}", p.value, s.station_name
                );
            }
        }
        // Every station with enough years must get a full-record cubic fit
        // spanning the same years as its raw data.
        assert_eq!(res.fit.len(), res.days.len());
        for (d, f) in res.days.iter().zip(&res.fit) {
            assert_eq!(d.station_name, f.station_name);
            assert!(
                f.points.len() >= 4,
                "cubic fit too short for {}", f.station_name
            );
            assert_eq!(
                f.points.len(),
                d.points.len(),
                "fit must span the same years for {}",
                f.station_name
            );
        }
    }
}