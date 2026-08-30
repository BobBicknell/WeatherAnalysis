//! Pull the full available daily weather record -- every field this station
//! reports (TMAX, TMIN, PRCP, and whatever else is present: SNOW, SNWD,
//! TOBS, WT## weather-type flags, etc.) -- for Corvallis, OR from NOAA's
//! Climate Data Online (CDO) API v2.
//!
//! Station: GHCND:USC00351862 (Corvallis State University, OR)
//! Period of record: 1893-01-01 to present (per NOAA CDO station detail page).
//!
//! Setup:
//! 1. Get a free CDO API token: https://www.ncdc.noaa.gov/cdo-web/token
//! 2. Set it as an env var:  export NOAA_TOKEN="your_token_here"
//! 3. cargo run --release
//!
//! The CDO API caps requests at 1 year of data and 1000 records per call, and
//! rate-limits to 5 req/sec / 10,000 req/day, so this pages within each year
//! and sleeps briefly between calls.

use chrono::Datelike;
use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fs::File;
use std::thread::sleep;
use std::time::Duration;

//const STATION_ID: &str = "GHCND:USC00351862"; // Corvallis State University, OR
//const STATION_START_YEAR: i32 = 1893;
//const OUTFILE: &str = "corvallis_weather_full.csv";

//const STATION_ID: &str = "GHCND:USW00024221"; // Eugene Airport OR
//const STATION_START_YEAR: i32 = 1938;
//const OUTFILE: &str = "eugene_weather_full.csv";

const STATION_ID: &str = "GHCND:USW00024232"; // Salem Airport OR
const STATION_START_YEAR: i32 = 1892;
const OUTFILE: &str = "salem_weather_full.csv";
const DATASET_ID: &str = "GHCND";
const BASE_URL: &str = "https://www.ncei.noaa.gov/cdo-web/api/v2/data";
// No datatypeid filter is sent in the request below -- pulling every field
// this station reports (TMAX/TMIN/PRCP plus whatever else is available:
// SNOW, SNWD, TOBS, WT## weather-type flags, etc.)

// Station's period of record starts 1893-01-01 (per NOAA CDO station detail
// page). Update this if you switch stations.

#[derive(Deserialize, Debug)]
struct CdoResponse {
    #[serde(default)]
    results: Vec<CdoRecord>,
}

#[derive(Deserialize, Debug)]
struct CdoRecord {
    date: String,
    datatype: String,
    station: String,
    value: f64,
}

fn fetch_year(
    client: &reqwest::blocking::Client,
    token: &str,
    year: i32,
) -> Result<Vec<CdoRecord>, Box<dyn Error>> {
    let mut records = Vec::new();
    let mut offset: u32 = 1;
    let limit: u32 = 1000;

    loop {
        let resp = client
            .get(BASE_URL)
            .header("token", token)
            .query(&[
                ("datasetid", DATASET_ID),
                ("stationid", STATION_ID),
                ("startdate", &format!("{year}-01-01")),
                ("enddate", &format!("{year}-12-31")),
                ("units", "standard"),
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
            ])
            .send()?;

        if resp.status() == 429 {
            eprintln!("  rate limited, sleeping 5s...");
            sleep(Duration::from_secs(5));
            continue;
        }

        let resp = resp.error_for_status()?;
        let parsed: CdoResponse = resp.json()?;
        let n = parsed.results.len();
        records.extend(parsed.results);

        if n < limit as usize {
            break;
        }
        offset += limit;
        sleep(Duration::from_millis(250)); // stay under 5 req/sec
    }

    Ok(records)
}

fn main() -> Result<(), Box<dyn Error>> {
    let token = env::var("NOAA_TOKEN").map_err(|_| {
        "Set NOAA_TOKEN env var first. Get a free token at https://www.ncdc.noaa.gov/cdo-web/token"
    })?;

    let end_year = chrono::Local::now().year();
    let start_year = STATION_START_YEAR;

    let client = reqwest::blocking::Client::new();
    let mut all_rows: Vec<CdoRecord> = Vec::new();

    for year in start_year..=end_year {
        println!("Fetching {year}...");
        match fetch_year(&client, &token, year) {
            Ok(mut recs) => all_rows.append(&mut recs),
            Err(e) => eprintln!("  failed for {year}: {e}"),
        }
        sleep(Duration::from_millis(250));
    }

    if all_rows.is_empty() {
        println!("No data retrieved.");
        return Ok(());
    }

    let file = File::create(OUTFILE)?;
    let mut wtr = csv::Writer::from_writer(file);
    wtr.write_record(["date", "datatype", "station", "value"])?;
    for r in &all_rows {
        wtr.write_record(&[
            r.date.chars().take(10).collect::<String>(),
            r.datatype.clone(),
            r.station.clone(),
            r.value.to_string(),
        ])?;
    }
    wtr.flush()?;

    println!("Wrote {} records to {}", all_rows.len(), OUTFILE);
    Ok(())
}
