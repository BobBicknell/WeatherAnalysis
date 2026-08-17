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
pub fn get_series(datatype: &str, station: Option<&str>) -> PolarsResult<Vec<StationSeries>> {
    let mut lf = load_df()?.filter(col("datatype").eq(lit(datatype)));
    if let Some(s) = station {
        lf = lf.filter(col("station").eq(lit(s)));
    }

    let df = lf
        .select([col("station"), col("date"), col("value")])
        .sort(["station", "date"], SortMultipleOptions::default())
        .collect()?;

    group_into_series(&df, "date")
}

/// Monthly or yearly average of (TMAX + TMIN) / 2 -- a much smoother trend
/// line than plotting daily TMAX/TMIN directly, which is dominated by
/// day-to-day noise. `period` is "monthly" or "yearly"; anything else
/// defaults to monthly. Optionally filtered to a single station.
pub fn get_mean_temp_trend(period: &str, station: Option<&str>) -> PolarsResult<Vec<StationSeries>> {
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

    let df = tmax
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
        .sort(["station", "period"], SortMultipleOptions::default())
        .collect()?;

    group_into_series(&df, "period")
}

/// Shared helper: turns a DataFrame with columns [station, <date_col>, value]
/// into one StationSeries per distinct station.
fn group_into_series(df: &DataFrame, date_col_name: &str) -> PolarsResult<Vec<StationSeries>> {
    let names = station_names();

    let station_col = df.column("station")?.str()?;
    let date_col = df.column(date_col_name)?.str()?;
    let value_col = df.column("value")?.f64()?;

    let mut grouped: HashMap<String, Vec<SeriesPoint>> = HashMap::new();
    for i in 0..df.height() {
        let st = station_col.get(i).unwrap_or_default().to_string();
        let date = date_col.get(i).unwrap_or_default().to_string();
        let value = value_col.get(i).unwrap_or(f64::NAN);
        grouped.entry(st).or_default().push(SeriesPoint { date, value });
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