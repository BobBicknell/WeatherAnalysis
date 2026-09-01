// Thin data-access layer. Talks to either the Tauri backend (invoke) or the
// web server (REST /api endpoints), whichever environment it is running in --
// the same files serve both the desktop app and the public web app. Nothing
// in main.js or index.html needs to change.

const tauriInvoke = window.__TAURI__?.core?.invoke;

async function webJson(path, params) {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== null && v !== undefined && v !== "") q.set(k, v);
  }
  const qs = q.toString();
  // Resolve relative to the document so /api works at the site root and when
  // the app is served under a path prefix (e.g. /CorvallisWeather/).
  const url = new URL(`api/${path}${qs ? `?${qs}` : ""}`, document.baseURI);
  const resp = await fetch(url, { headers: { Accept: "application/json" } });
  if (!resp.ok) {
    throw new Error(`GET /api/${path} failed: ${resp.status} ${await resp.text()}`);
  }
  return resp.json();
}

export async function getStations() {
  if (tauriInvoke) return tauriInvoke("get_stations");
  return webJson("stations", {});
}

export async function getDatatypes() {
  if (tauriInvoke) return tauriInvoke("get_datatypes");
  return webJson("datatypes", {});
}

export async function getSeries(datatype, station, window, lowPass) {
  if (tauriInvoke) {
    return tauriInvoke("get_series", {
      datatype,
      station: station || null,
      window: window || null,
      lowPass: lowPass || null,
    });
  }
  return webJson("series", {
    datatype,
    station: station || null,
    window: window || null,
    low_pass: lowPass || null,
  });
}

export async function getDailyAnomaly(datatype, station, window, lowPass) {
  if (tauriInvoke) {
    return tauriInvoke("get_daily_anomaly", {
      datatype,
      station: station || null,
      window: window || null,
      lowPass: lowPass || null,
    });
  }
  return webJson("daily-anomaly", {
    datatype,
    station: station || null,
    window: window || null,
    low_pass: lowPass || null,
  });
}

export async function getMeanTempTrend(period, station, window, lowPass) {
  if (tauriInvoke) {
    return tauriInvoke("get_mean_temp_trend", {
      period,
      station: station || null,
      window: window || null,
      lowPass: lowPass || null,
    });
  }
  return webJson("mean-temp-trend", {
    period,
    station: station || null,
    window: window || null,
    low_pass: lowPass || null,
  });
}

export async function getHotDaysPerYear(threshold, station) {
  if (tauriInvoke) {
    return tauriInvoke("get_hot_days_per_year", {
      threshold,
      station: station || null,
    });
  }
  return webJson("hot-days", { threshold, station: station || null });
}

export async function getGrowingSeason(station) {
  if (tauriInvoke) {
    return tauriInvoke("get_growing_season", {
      station: station || null,
    });
  }
  return webJson("growing-season", { station: station || null });
}