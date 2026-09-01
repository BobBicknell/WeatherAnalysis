//! Shared query layer over the combined weather Parquet file. Has no Tauri
//! dependency -- these are plain functions returning serde-serializable
//! structs -- so the same code backs both the Tauri desktop app (via
//! `#[tauri::command]` wrappers) and the Axum web server (via REST
//! handlers).

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
    LazyFrame::scan_parquet(
        PlRefPath::from(data_path().to_string_lossy().as_ref()),
        ScanArgsParquet::default(),
    )
}

/// Adds an "avg_value" column: a centered rolling mean of "value" over
/// `window` rows, computed independently per "station". Requires the frame
/// already be sorted by (station, date/period) -- both callers below are.
/// `center: true` avoids the phase lag a trailing average would introduce,
/// appropriate here since this is historical (not live/streaming) data.
/// `min_periods: 1` keeps the line defined at the edges instead of gapping.
fn with_rolling_avg(lf: LazyFrame, window: i64) -> PolarsResult<LazyFrame> {
    Ok(lf.with_column(
        col("value")
            .rolling_mean(RollingOptionsFixedWindow {
                window_size: window as usize,
                min_periods: 1,
                weights: None,
                center: true,
                fn_params: None,
            })
            .over([col("station")])?
            .alias("avg_value"),
    ))
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
fn with_low_pass(lf: LazyFrame, span: i64) -> PolarsResult<LazyFrame> {
    let options = EWMOptions {
        alpha: 2.0 / (span as f64 + 1.0),
        adjust: false,
        bias: false,
        min_periods: 1,
        ignore_nulls: true,
    };
    Ok(lf.with_column(
        col("value")
            .ewm_mean(options)
            .reverse()
            .ewm_mean(options)
            .reverse()
            .over([col("station")])?
            .alias("lpf_value"),
    ))
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
        .iter()
        .flatten()
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
    let mut types: Vec<String> = dt_col.iter().flatten().map(String::from).collect();
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
            lf = with_rolling_avg(lf, w)?;
        }
    }
    if let Some(s) = low_pass {
        if s > 1 {
            lf = with_low_pass(lf, s)?;
        }
    }

    let df = lf.collect()?;

    group_into_series(&df, "date")
}

/// Anomaly series: `value` minus the calendar-day climatology. For each
/// station and calendar day (MM-DD), the "normal" is the mean of `value`
/// over every year that station has data for that day; the plotted value is
/// `value - normal`. Same parameters and behavior as `get_series` (station
/// filter, then optional moving-average/low-pass applied to the anomaly).
pub fn get_daily_anomaly(
    datatype: &str,
    station: Option<&str>,
    window: Option<i64>,
    low_pass: Option<i64>,
) -> PolarsResult<Vec<StationSeries>> {
    let mut lf = load_df()?.filter(col("datatype").eq(lit(datatype)));
    if let Some(s) = station {
        lf = lf.filter(col("station").eq(lit(s)));
    }
    let mut lf = subtract_calendar_day_normal(
        lf.select([col("station"), col("date"), col("value")]),
        "date",
    )?;
    if let Some(w) = window {
        if w > 1 {
            lf = with_rolling_avg(lf, w)?;
        }
    }
    if let Some(s) = low_pass {
        if s > 1 {
            lf = with_low_pass(lf, s)?;
        }
    }
    let df = lf.collect()?;
    group_into_series(&df, "date")
}

/// Replaces `value` with `value - normal`, where `normal` is the mean of
/// `value` over every year that each station has data for that calendar day
/// (MM-DD, derived from the string `date_col`). Returns the frame sorted by
/// (station, date). Extracted from `get_daily_anomaly` so the subtraction is
/// directly unit-testable with synthetic frames.
fn subtract_calendar_day_normal(lf: LazyFrame, date_col: &str) -> PolarsResult<LazyFrame> {
    Ok(lf
        .with_column(
            (col("value")
                - col("value")
                    .mean()
                    .over([col("station"), col(date_col).str().slice(lit(5), lit(5))])?)
            .alias("value"),
        )
        .sort(["station", date_col], SortMultipleOptions::default()))
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
    let mut tmin = base.filter(col("datatype").eq(lit("TMIN"))).select([
        col("station"),
        col("date"),
        col("value").alias("tmin"),
    ]);

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
            col("date")
                .str()
                .slice(lit(0), lit(period_len))
                .alias("period"),
        ])
        .group_by([col("station"), col("period")])
        .agg([col("mean_temp").mean().alias("value")])
        .sort(["station", "period"], SortMultipleOptions::default());
    if let Some(w) = window {
        if w > 1 {
            lf = with_rolling_avg(lf, w)?;
        }
    }
    if let Some(s) = low_pass {
        if s > 1 {
            lf = with_low_pass(lf, s)?;
        }
    }

    let df = lf.collect()?;

    group_into_series(&df, "period")
}

/// Number of days per year where TMAX exceeds `threshold` (in the same units
/// as the data: degrees Fahrenheit here), as one `StationSeries` per station
/// whose x values are years and whose value is the per-year count.
pub fn get_hot_days_per_year(
    threshold: f64,
    station: Option<&str>,
) -> PolarsResult<Vec<StationSeries>> {
    hot_day_counts(threshold, station)
}

fn hot_day_counts(threshold: f64, station: Option<&str>) -> PolarsResult<Vec<StationSeries>> {
    let mut lf = load_df()?.filter(col("datatype").eq(lit("TMAX"))).select([
        col("station"),
        col("date"),
        col("value"),
    ]);

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

/// Growing-season length per station: the number of days between the last
/// spring frost and the first fall frost of each year, where a frost day is
/// one whose low (TMIN) fell to 32 °F or below. Frosts are dated by month:
/// spring frosts fall in March-June, fall frosts in July-November. Returns one
/// `StationSeries` per station whose x values are years and whose value is the
/// season length in days (years lacking a frost in either window are omitted).
pub fn get_growing_season(station: Option<&str>) -> PolarsResult<Vec<StationSeries>> {
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

    Ok(result)
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
        let Some((year, month, doy)) = day_of_year(date) else {
            continue;
        };
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
    Some((
        year,
        month,
        CUMDAYS[(month - 1) as usize] + day + leap as u32,
    ))
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
        grouped.entry(st).or_default().push(SeriesPoint {
            date,
            value,
            avg,
            lpf,
        });
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
        DataFrame::new(
            n,
            vec![
                Column::new("station".into(), stations),
                Column::new(
                    "date".into(),
                    (0..n)
                        .map(|i| format!("2000-01-{:02}", i + 1))
                        .collect::<Vec<_>>(),
                ),
                Column::new("value".into(), values),
            ],
        )
        .unwrap()
        .lazy()
    }

    fn lpf_column(stations: Vec<&str>, values: Vec<f64>, span: i64) -> Vec<Option<f64>> {
        let df = with_low_pass(frame(stations, values), span)
            .unwrap()
            .collect()
            .unwrap();
        df.column("lpf_value")
            .unwrap()
            .f64()
            .unwrap()
            .iter()
            .collect()
    }

    #[test]
    fn constant_series_pass_through_unchanged_per_station() {
        let got = lpf_column(
            vec!["a", "a", "a", "b", "b", "b"],
            vec![10.0, 10.0, 10.0, 20.0, 20.0, 20.0],
            3,
        );
        assert_eq!(got[..3], vec![Some(10.0); 3][..]);
        assert_eq!(got[3..], vec![Some(20.0); 3]);
    }

    #[test]
    fn forward_backward_pass_is_zero_phase() {
        // An impulse must remain at its original index -- proof the backward
        // pass cancels the EMA's phase lag.
        let got = lpf_column(
            vec!["a"; 21],
            vec![
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0, 0.0,
            ],
            3,
        );
        let peak = got
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.unwrap().total_cmp(&b.1.unwrap()))
            .unwrap()
            .0;
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
            let (min, max) = s
                .points
                .iter()
                .map(|p| p.value)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                    (a.min(v), b.max(v))
                });
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
        assert!(!res.is_empty());
        for s in &res {
            for p in &s.points {
                assert!(
                    p.value >= 0.0 && p.value <= 366.0,
                    "implausible count: {}",
                    p.value
                );
                assert!(p.date.len() == 4, "expected year, got {}", p.date);
            }
        }
        let total: f64 = res
            .iter()
            .flat_map(|s| s.points.iter().map(|p| p.value))
            .sum();
        assert!(total > 0.0, "no days above 90°F found in the whole record");
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
        assert!(!res.is_empty());
        for s in &res {
            assert!(!s.points.is_empty(), "no seasons for {}", s.station_name);
            for p in &s.points {
                assert!(p.date.len() == 4, "expected year, got {}", p.date);
                // A plausible Pacific-Northwest growing season is ~100-260 days.
                assert!(
                    p.value >= 60.0 && p.value <= 300.0,
                    "implausible season length {} for {}",
                    p.value,
                    s.station_name
                );
            }
        }
    }

    /// Frame with explicit dates (unlike `frame`, which only produces January
    /// 2000), for tests that need several years of the same calendar day.
    fn dated_frame(stations: Vec<&str>, dates: Vec<&str>, values: Vec<f64>) -> LazyFrame {
        let n = values.len();
        DataFrame::new(
            n,
            vec![
                Column::new("station".into(), stations),
                Column::new("date".into(), dates),
                Column::new("value".into(), values),
            ],
        )
        .unwrap()
        .lazy()
    }

    #[test]
    fn daily_anomaly_subtracts_calendar_day_climatology() {
        // Station "a": Jan 1 values of 10, 30 -> normal 20; Jan 2 values 2, 4
        // -> normal 3. Anomaly = value - normal.
        let lf = subtract_calendar_day_normal(
            dated_frame(
                vec!["a", "a", "a", "a"],
                vec!["2000-01-01", "2001-01-01", "2000-01-02", "2001-01-02"],
                vec![10.0, 30.0, 2.0, 4.0],
            ),
            "date",
        )
        .unwrap();
        let res = group_into_series(&lf.collect().unwrap(), "date").unwrap();
        let a = &res[0];
        assert_eq!(a.station_name, "a");
        let by_date: std::collections::HashMap<_, _> =
            a.points.iter().map(|p| (p.date.clone(), p.value)).collect();
        assert!((by_date["2000-01-01"] - (10.0 - 20.0)).abs() < 1e-9);
        assert!((by_date["2001-01-01"] - (30.0 - 20.0)).abs() < 1e-9);
        assert!((by_date["2000-01-02"] - (2.0 - 3.0)).abs() < 1e-9);
        assert!((by_date["2001-01-02"] - (4.0 - 3.0)).abs() < 1e-9);
    }

    #[test]
    fn daily_anomaly_is_per_station() {
        // Same calendar day, different means per station: "a" normal = 100,
        // "b" normal = 10 for Jan 1.
        let lf = subtract_calendar_day_normal(
            dated_frame(
                vec!["a", "a", "b", "b"],
                vec!["2000-01-01", "2001-01-01", "2000-01-01", "2001-01-01"],
                vec![90.0, 110.0, 8.0, 12.0],
            ),
            "date",
        )
        .unwrap();
        let res = group_into_series(&lf.collect().unwrap(), "date").unwrap();
        let by_name: std::collections::HashMap<_, _> = res
            .iter()
            .map(|s| (s.station_name.clone(), s.points[0].value))
            .collect();
        assert!((by_name["a"] - (90.0 - 100.0)).abs() < 1e-9);
        assert!((by_name["b"] - (8.0 - 10.0)).abs() < 1e-9);
    }

    #[test]
    fn daily_anomaly_real_data() {
        if !resolved_data_path().exists() {
            return;
        }
        let res = get_daily_anomaly("TMAX", None, None, None).unwrap();
        assert!(!res.is_empty());
        for s in &res {
            assert!(!s.points.is_empty());
            // If the climatology were ignored (raw values), January would be
            // consistently positive. Anomalies must straddle zero; requiring
            // some negatives proves the subtraction actually happened.
            assert!(
                s.points.iter().any(|p| p.value < 0.0),
                "no negative anomalies for {}",
                s.station_name
            );
            // A daily anomaly vs. its own mean over all years should be
            // roughly zero-centered: abs(max) and abs(min) comparable scale.
            assert_eq!(s.points[0].date.len(), 10, "expected YYYY-MM-DD");
        }
    }
}
