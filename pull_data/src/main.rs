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

/// Everything about `fetch_year`'s behavior that needs to change between
/// production and tests: which server it talks to, and how long it sleeps.
/// Tests use a mock server and near-zero sleeps so the pagination and
/// rate-limit-retry logic can be exercised in milliseconds instead of
/// tens of seconds.
struct PullConfig {
    base_url: String,
    /// Sleep after a 429 before retrying the same page.
    rate_limit_sleep: Duration,
    /// Sleep between successful page/year requests, to stay under the
    /// 5 req/sec CDO limit.
    page_sleep: Duration,
}

impl PullConfig {
    fn production() -> Self {
        PullConfig {
            base_url: BASE_URL.to_string(),
            rate_limit_sleep: Duration::from_secs(5),
            page_sleep: Duration::from_millis(250),
        }
    }
}

/// NOAA CDO dates come back as RFC-3339-ish timestamps
/// (`"1998-04-12T00:00:00"`); the CSV only wants the date part.
fn truncate_date(date: &str) -> String {
    date.chars().take(10).collect()
}

fn fetch_year(
    client: &reqwest::blocking::Client,
    config: &PullConfig,
    token: &str,
    year: i32,
) -> Result<Vec<CdoRecord>, Box<dyn Error>> {
    let mut records = Vec::new();
    let mut offset: u32 = 1;
    let limit: u32 = 1000;

    loop {
        let resp = client
            .get(&config.base_url)
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
            eprintln!("  rate limited, sleeping {:?}...", config.rate_limit_sleep);
            sleep(config.rate_limit_sleep);
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
        sleep(config.page_sleep); // stay under 5 req/sec
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
    let config = PullConfig::production();
    let mut all_rows: Vec<CdoRecord> = Vec::new();

    for year in start_year..=end_year {
        println!("Fetching {year}...");
        match fetch_year(&client, &config, &token, year) {
            Ok(mut recs) => all_rows.append(&mut recs),
            Err(e) => eprintln!("  failed for {year}: {e}"),
        }
        sleep(config.page_sleep);
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
            truncate_date(&r.date),
            r.datatype.clone(),
            r.station.clone(),
            r.value.to_string(),
        ])?;
    }
    wtr.flush()?;

    println!("Wrote {} records to {}", all_rows.len(), OUTFILE);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PullConfig pointed at a mockito server, with sleeps small enough
    /// that a retry test finishes in milliseconds instead of seconds.
    fn test_config(base_url: String) -> PullConfig {
        PullConfig {
            base_url,
            rate_limit_sleep: Duration::from_millis(5),
            page_sleep: Duration::from_millis(1),
        }
    }

    fn record_json(date: &str, datatype: &str, value: f64) -> String {
        format!(
            r#"{{"date":"{date}","datatype":"{datatype}","station":"{STATION_ID}","value":{value}}}"#
        )
    }

    #[test]
    fn truncates_timestamp_to_date() {
        assert_eq!(truncate_date("1998-04-12T00:00:00"), "1998-04-12");
        // Already-short input is left alone rather than panicking.
        assert_eq!(truncate_date("2020-01-01"), "2020-01-01");
    }

    #[test]
    fn single_page_under_limit_stops_after_one_request() {
        let mut server = mockito::Server::new();
        let body = format!(
            r#"{{"results":[{}]}}"#,
            record_json("1998-04-12T00:00:00", "TMAX", 61.0)
        );
        let mock = server
            .mock("GET", "/data")
            .match_query(mockito::Matcher::UrlEncoded("offset".into(), "1".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .expect(1) // must be called exactly once -- no second page requested
            .create();

        let client = reqwest::blocking::Client::new();
        let config = test_config(format!("{}/data", server.url()));
        let records = fetch_year(&client, &config, "test-token", 1998).unwrap();

        mock.assert();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].date, "1998-04-12T00:00:00");
        assert_eq!(records[0].datatype, "TMAX");
    }

    #[test]
    fn full_page_triggers_second_request_at_next_offset() {
        let mut server = mockito::Server::new();

        // First page: exactly `limit` (1000) records -> caller must ask
        // again at offset 1001 rather than assuming that's the end.
        let first_page_records: Vec<String> = (0..1000)
            .map(|i| {
                record_json(
                    &format!("1998-01-{:02}T00:00:00", (i % 28) + 1),
                    "TMAX",
                    50.0,
                )
            })
            .collect();
        let first_body = format!(r#"{{"results":[{}]}}"#, first_page_records.join(","));

        let second_body = format!(
            r#"{{"results":[{}]}}"#,
            record_json("1998-12-31T00:00:00", "TMAX", 45.0)
        );

        let mock_page1 = server
            .mock("GET", "/data")
            .match_query(mockito::Matcher::UrlEncoded("offset".into(), "1".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(first_body)
            .expect(1)
            .create();

        let mock_page2 = server
            .mock("GET", "/data")
            .match_query(mockito::Matcher::UrlEncoded("offset".into(), "1001".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(second_body)
            .expect(1)
            .create();

        let client = reqwest::blocking::Client::new();
        let config = test_config(format!("{}/data", server.url()));
        let records = fetch_year(&client, &config, "test-token", 1998).unwrap();

        mock_page1.assert();
        mock_page2.assert();
        assert_eq!(records.len(), 1001, "should combine both pages");
    }

    #[test]
    fn rate_limit_response_is_retried_not_propagated() {
        let mut server = mockito::Server::new();
        let body = format!(
            r#"{{"results":[{}]}}"#,
            record_json("2000-06-15T00:00:00", "PRCP", 0.1)
        );

        // First call: 429. Second call (the retry): success. mockito serves
        // mocks in creation order per matching request when both match the
        // same query, so the 429 mock is consumed first.
        let mock_429 = server
            .mock("GET", "/data")
            .match_query(mockito::Matcher::UrlEncoded("offset".into(), "1".into()))
            .with_status(429)
            .expect(1)
            .create();
        let mock_ok = server
            .mock("GET", "/data")
            .match_query(mockito::Matcher::UrlEncoded("offset".into(), "1".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .expect(1)
            .create();

        let client = reqwest::blocking::Client::new();
        let config = test_config(format!("{}/data", server.url()));
        let records = fetch_year(&client, &config, "test-token", 2000).unwrap();

        mock_429.assert();
        mock_ok.assert();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn server_error_is_propagated_as_err() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/data").with_status(500).create();

        let client = reqwest::blocking::Client::new();
        let config = test_config(format!("{}/data", server.url()));
        let result = fetch_year(&client, &config, "test-token", 1998);

        assert!(
            result.is_err(),
            "a non-429 error status should surface as Err, not be swallowed"
        );
    }

    #[test]
    fn empty_results_produce_empty_vec_not_error() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("GET", "/data")
            .match_query(mockito::Matcher::UrlEncoded("offset".into(), "1".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"results":[]}"#)
            .create();

        let client = reqwest::blocking::Client::new();
        let config = test_config(format!("{}/data", server.url()));
        let records = fetch_year(&client, &config, "test-token", 1998).unwrap();

        assert!(records.is_empty());
    }
}
