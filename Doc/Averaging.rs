use polars::prelude::*;

/// Extracts the long-term climate trend from daily temperature data by
/// eliminating seasonality and applying a 10-year rolling low-pass filter.
fn extract_climate_trend(lf: LazyFrame) -> Result<DataFrame, PolarsError> {
    // 1. Extract calendar day-of-year (1 to 366) to capture the seasonal cycle
    let lf_with_doy = lf.with_column(
        col("date").dt().ordinal_day().alias("day_of_year")
    );

    // 2. Calculate the 100-year average for each day of the year
    let climatology = lf_with_doy.clone()
        .group_by([col("day_of_year")])
        .agg([col("temperature").mean().alias("mean_seasonal_temp")]);

    // 3. Join the seasonal baseline back to the main time series
    let joined = lf_with_doy
        .join(climatology, &[col("day_of_year")], &[col("day_of_year")], JoinType::Left.into());

    // 4. Calculate daily anomalies (Raw Temperature - Seasonal Baseline)
    let anomalies = joined.with_column(
        (col("temperature") - col("mean_seasonal_temp")).alias("anomaly")
    );

    // 5. Configure a 10-year centered low-pass filter (approx 3,652 days)
    let trend_options = RollingOptionsFixedWindow {
        window_size: 3652,
        min_periods: 1826, // Requires 5 years of data to prevent severe edge loss
        weights: None,
        center: true,      // Centers the window to prevent chronological lag
        fn_options: Default::default(),
    };

    // 6. Sort chronologically and apply the rolling mean to isolate the trend
    let final_trends = anomalies
        .sort(["date"], Default::default())
        .with_column(
            col("anomaly")
                .rolling_mean(trend_options)
                .alias("long_term_trend")
        );

    // 7. Execute the lazy graph and return the materialized DataFrame
    final_trends.collect()
}

fn main() -> Result<(), PolarsError> {
    // 1. Load the data efficiently from a Parquet file using LazyFrame
    // Assumes columns: "date" (Polars Date/Datetime type) and "temperature" (f64)
    let lf = LazyFrame::scan_parquet("daily_temperatures.parquet", Default::default())?;

    println!("Processing 100 years of climate data...");

    // 2. Pass the LazyFrame into the calculation function
    let processed_df = extract_climate_trend(lf)?;

    // 3. Save the results back out to a new Parquet file
    let mut file = std::fs::File::create("climate_trend_output.parquet")?;
    ParquetWriter::new(&mut file).finish(&processed_df)?;

    println!("Success! Extracted trend saved to climate_trend_output.parquet");
    println!("Columns available: {:?}", processed_df.get_column_names());

    Ok(())
}
