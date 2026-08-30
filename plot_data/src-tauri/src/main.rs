#[tauri::command]
fn get_stations() -> Result<Vec<plot_data_core::StationInfo>, String> {
    plot_data_core::get_stations().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_datatypes() -> Result<Vec<String>, String> {
    plot_data_core::get_datatypes().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_series(
    datatype: String,
    station: Option<String>,
    window: Option<i64>,
    low_pass: Option<i64>,
) -> Result<Vec<plot_data_core::StationSeries>, String> {
    plot_data_core::get_series(&datatype, station.as_deref(), window, low_pass)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_hot_days_per_year(
    threshold: f64,
    station: Option<String>,
) -> Result<plot_data_core::HotDaysResult, String> {
    plot_data_core::get_hot_days_per_year(threshold, station.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_growing_season(
    station: Option<String>,
) -> Result<plot_data_core::GrowingSeasonResult, String> {
    plot_data_core::get_growing_season(station.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_mean_temp_trend(
    period: String,
    station: Option<String>,
    window: Option<i64>,
    low_pass: Option<i64>,
) -> Result<Vec<plot_data_core::StationSeries>, String> {
    plot_data_core::get_mean_temp_trend(&period, station.as_deref(), window, low_pass)
        .map_err(|e| e.to_string())
}

fn main() {
    // Surface the resolved data path at startup so a missing/misplaced
    // Parquet file shows up immediately in the terminal instead of as a
    // silent empty UI.
    let path = plot_data_core::resolved_data_path();
    if path.exists() {
        eprintln!("[plot_data] using data file: {}", path.display());
    } else {
        eprintln!(
            "[plot_data] WARNING: data file not found at {} -- dropdowns will be empty. \
             Set WEATHER_DATA_PATH to override.",
            path.display()
        );
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_stations,
            get_datatypes,
            get_series,
            get_hot_days_per_year,
            get_growing_season,
            get_mean_temp_trend
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
