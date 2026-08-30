//! HTTP server for the weather plots frontend. Serves the static files in
//! `plot_data/src/` and exposes the data-core query functions under `/api`.
//! Intended to run on loopback behind a reverse proxy (e.g. Caddy).
//!
//! Environment:
//!   WEATHER_SERVER_PORT   bind port (default 9002)
//!   WEATHER_STATIC_DIR    directory of frontend files (default ../src)
//!   WEATHER_DATA_PATH     path to weather_all.parquet (see plot-data-core)

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use plot_data_core::{GrowingSeasonResult, HotDaysResult, StationInfo, StationSeries};
use serde::Deserialize;
use std::env;
use std::path::PathBuf;
use tower_http::services::ServeDir;

type ApiError = (StatusCode, String);

fn bad_request(msg: &str) -> ApiError {
    (StatusCode::BAD_REQUEST, msg.to_string())
}

/// Run a blocking data-core query on the blocking pool; polars results and
/// join errors both become 500s.
async fn blocking<T, E, F>(f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// Query params accepted by the /api endpoints; every field is optional and
/// each endpoint requires the ones it needs.
#[derive(Deserialize, Default)]
struct Params {
    datatype: Option<String>,
    period: Option<String>,
    station: Option<String>,
    window: Option<i64>,
    low_pass: Option<i64>,
    threshold: Option<f64>,
}

async fn get_stations() -> Result<Json<Vec<StationInfo>>, ApiError> {
    Ok(Json(blocking(plot_data_core::get_stations).await?))
}

async fn get_datatypes() -> Result<Json<Vec<String>>, ApiError> {
    Ok(Json(blocking(plot_data_core::get_datatypes).await?))
}

async fn get_series(Query(p): Query<Params>) -> Result<Json<Vec<StationSeries>>, ApiError> {
    let datatype = p
        .datatype
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad_request("missing datatype parameter"))?;
    Ok(Json(
        blocking(move || {
            plot_data_core::get_series(&datatype, p.station.as_deref(), p.window, p.low_pass)
        })
        .await?,
    ))
}

async fn get_mean_temp_trend(
    Query(p): Query<Params>,
) -> Result<Json<Vec<StationSeries>>, ApiError> {
    let period = p
        .period
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad_request("missing period parameter"))?;
    Ok(Json(
        blocking(move || {
            plot_data_core::get_mean_temp_trend(&period, p.station.as_deref(), p.window, p.low_pass)
        })
        .await?,
    ))
}

async fn get_hot_days_per_year(Query(p): Query<Params>) -> Result<Json<HotDaysResult>, ApiError> {
    let threshold = p
        .threshold
        .ok_or_else(|| bad_request("missing threshold parameter"))?;
    Ok(Json(
        blocking(move || plot_data_core::get_hot_days_per_year(threshold, p.station.as_deref()))
            .await?,
    ))
}

async fn get_growing_season(
    Query(p): Query<Params>,
) -> Result<Json<GrowingSeasonResult>, ApiError> {
    Ok(Json(
        blocking(move || plot_data_core::get_growing_season(p.station.as_deref())).await?,
    ))
}

#[tokio::main]
async fn main() {
    let port: u16 = env::var("WEATHER_SERVER_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(9002);

    let static_dir = match env::var("WEATHER_STATIC_DIR") {
        Ok(p) => PathBuf::from(p),
        Err(_) => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src"),
    };

    // Surface the resolved data path at startup (mirrors the Tauri app) so a
    // missing Parquet file shows up in the logs instead of a silent empty UI.
    let data_path = plot_data_core::resolved_data_path();
    if data_path.exists() {
        println!("[weather-server] using data file: {}", data_path.display());
    } else {
        eprintln!(
            "[weather-server] WARNING: data file not found at {} -- dropdowns will be empty. \
             Set WEATHER_DATA_PATH to override.",
            data_path.display()
        );
    }

    let app = Router::new()
        .route("/api/stations", get(get_stations))
        .route("/api/datatypes", get(get_datatypes))
        .route("/api/series", get(get_series))
        .route("/api/mean-temp-trend", get(get_mean_temp_trend))
        .route("/api/hot-days", get(get_hot_days_per_year))
        .route("/api/growing-season", get(get_growing_season))
        .fallback_service(ServeDir::new(&static_dir));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("failed to bind listener");
    println!(
        "[weather-server] listening on http://127.0.0.1:{port} (static dir: {})",
        static_dir.display()
    );
    axum::serve(listener, app).await.expect("server stopped");
}
