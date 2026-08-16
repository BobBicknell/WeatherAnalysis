//! Reads every CSV file in ../Data (produced by pull_data) and combines them
//! into a single Parquet file, also written to ../Data.
//!
//! Each input CSV is expected to have the columns: date, datatype, station, value
//! (the long/tidy format written by pull_data). Files are simply stacked on
//! top of each other -- the `station` column already distinguishes rows from
//! different stations (Corvallis, Eugene, Salem, etc.).

use glob::glob;
use polars::prelude::*;
use std::path::PathBuf;

const DATA_DIR: &str = "../Data";
const OUTFILE: &str = "../Data/weather_all.parquet";

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
            LazyCsvReader::new(p)
                .with_has_header(true)
                .finish()
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", p.display()))
        })
        .collect();

    // Stack all frames on top of each other (same schema: date, datatype, station, value).
    let combined = concat(&lazy_frames, UnionArgs::default())?;

    let mut df = combined.collect()?;
    println!("Combined shape: {:?}", df.shape());

    let mut file = std::fs::File::create(OUTFILE)?;
    ParquetWriter::new(&mut file).finish(&mut df)?;

    println!("Wrote {} rows to {}", df.height(), OUTFILE);
    Ok(())
}
