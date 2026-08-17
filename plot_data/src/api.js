// Thin data-access layer. Everything below talks to the Tauri backend via
// invoke(). To turn this into a plain web page backed by a REST API instead
// (e.g. an Axum server reusing src-tauri/src/data.rs's query functions),
// replace the bodies of the three functions below with fetch() calls --
// nothing in main.js or index.html needs to change.

const { invoke } = window.__TAURI__.core;

export async function getStations() {
  return invoke("get_stations");
}

export async function getDatatypes() {
  return invoke("get_datatypes");
}

export async function getSeries(datatype, station) {
  return invoke("get_series", { datatype, station: station || null });
}

export async function getMeanTempTrend(period, station) {
  return invoke("get_mean_temp_trend", { period, station: station || null });
}