//! Reads every CSV file in ../Data (produced by pull_data) and combines them
//! into a single Parquet file, written to the user's config directory
//! (~/.config/RobertBicknell/Weatheranalysis/weather_all.parquet on Linux)
//! rather than the project tree, so it survives independently of wherever
//! the repo happens to be checked out.
//!
//! Each input CSV is expected to have the columns: date, datatype, station, value
//! (the long/tidy format written by pull_data). Files are simply stacked on
//! top of each other -- the `station` column already distinguishes rows from
//! different stations (Corvallis, Eugene, Salem, etc.).

use glob::glob;
use polars::prelude::*;
use std::path::PathBuf;

const DATA_DIR: &str = "../Data";

/// Resolves to ~/.config/RobertBicknell/Weatheranalysis (on Linux/macOS via
/// XDG_CONFIG_HOME or ~/.config; %APPDATA%\RobertBicknell\Weatheranalysis
/// on Windows). Can be overridden with WEATHER_DATA_PATH (set to the full
/// file path, not just the directory) if you want the Parquet file
/// somewhere else -- plot_data respects the same override.
fn output_path() -> PolarsResult<PathBuf> {
    if let Ok(p) = std::env::var("WEATHER_DATA_PATH") {
        return Ok(PathBuf::from(p));
    }
    let config_dir = dirs::config_dir()
        .ok_or_else(|| PolarsError::ComputeError("could not resolve config directory".into()))?;
    let dir = config_dir.join("RobertBicknell").join("Weatheranalysis");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("weather_all.parquet"))
}

fn main() -> PolarsResult<()> {
    let pattern = format!("{DATA_DIR}/*.csv");
    let paths: Vec<PathBuf> = glob(&pattern)
        .expect("invalid glob pattern")
        .filter_map(Result::ok)
        .collect();

    if paths.is_empty() {
        eprintln!("No CSV files found matching {pattern}");
        return Ok(());
    }

    println!("Found {} CSV file(s):", paths.len());
    for p in &paths {
        println!("  {}", p.display());
    }

    let lazy_frames: Vec<LazyFrame> = paths
        .iter()
        .map(|p| {
            LazyCsvReader::new(PlRefPath::from(p.to_string_lossy().as_ref()))
                .with_has_header(true)
                .finish()
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", p.display()))
        })
        .collect();

    // Stack all frames on top of each other (same schema: date, datatype, station, value).
    let combined = concat(&lazy_frames, UnionArgs::default())?;

    let mut df = combined.collect()?;
    println!("Combined shape: {:?}", df.shape());

    let outfile = output_path()?;
    let mut file = std::fs::File::create(&outfile)?;
    ParquetWriter::new(&mut file).finish(&mut df)?;

    println!("Wrote {} rows to {}", df.height(), outfile.display());
    Ok(())
}
