import { getStations, getDatatypes, getSeries, getMeanTempTrend, getHotDaysPerYear, getGrowingSeason } from "./api.js";

// Fixed categorical order (dataviz skill's validated palette) -- assigned
// by station index, never cycled/reassigned, so a station keeps its color
// whether its raw line is shown alone or paired with a moving-average
// overlay in the same hue.
const PALETTE = ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#4a3aa7", "#e34948"];

function seriesToTraces(series, avgLabel, lpfLabel, rawMode = "lines", showRaw = true) {
  const traces = [];
  series.forEach((s, i) => {
    const color = PALETTE[i % PALETTE.length];
    if (showRaw) {
      traces.push({
        x: s.points.map((p) => p.date),
        y: s.points.map((p) => p.value),
        name: s.station_name,
        mode: rawMode,
        type: "scatter",
        line: { color, width: 2 },
        marker: { color },
      });
    }
    if (s.points.some((p) => p.avg != null)) {
      traces.push({
        x: s.points.map((p) => p.date),
        y: s.points.map((p) => p.avg),
        name: `${s.station_name} ${avgLabel}`,
        mode: "lines",
        type: "scatter",
        line: { color, width: 5, dash: "dash" },
      });
    }
    if (s.points.some((p) => p.lpf != null)) {
      traces.push({
        x: s.points.map((p) => p.date),
        y: s.points.map((p) => p.lpf),
        name: `${s.station_name} ${lpfLabel}`,
        mode: "lines",
        type: "scatter",
        line: { color, width: 4, dash: "dot" },
      });
    }
  });
  return traces;
}

function parseWindow(input) {
  const n = parseInt(input.value, 10);
  return Number.isFinite(n) && n > 1 ? n : null;
}

// ---- Tab switching ----

const tabButtons = document.querySelectorAll(".tab-btn");
const views = {
  daily: document.getElementById("daily-view"),
  trends: document.getElementById("trends-view"),
  hot: document.getElementById("hot-view"),
  growing: document.getElementById("growing-view"),
};

tabButtons.forEach((btn) => {
  btn.addEventListener("click", () => {
    tabButtons.forEach((b) => b.classList.remove("active"));
    btn.classList.add("active");
    Object.values(views).forEach((v) => v.classList.add("hidden"));
    views[btn.dataset.tab].classList.remove("hidden");
    // Plotly needs a nudge to redraw correctly once its container becomes visible.
    requestAnimationFrame(() => {
      if (btn.dataset.tab === "daily") Plotly.Plots.resize(document.getElementById("chart"));
      if (btn.dataset.tab === "trends") Plotly.Plots.resize(document.getElementById("trend-chart"));
      if (btn.dataset.tab === "hot") Plotly.Plots.resize(document.getElementById("hot-chart"));
      if (btn.dataset.tab === "growing") Plotly.Plots.resize(document.getElementById("growing-chart"));
    });
  });
});

// ---- Daily view ----

const datatypeSelect = document.getElementById("datatype");
const stationSelect = document.getElementById("station");
const dailyWindowInput = document.getElementById("daily-window");
const dailyLowPassInput = document.getElementById("daily-lowpass");
const dailyShowRaw = document.getElementById("daily-show-raw");

async function initDaily(datatypes, stations) {
  datatypeSelect.innerHTML = datatypes.map((d) => `<option value="${d}">${d}</option>`).join("");
  for (const s of stations) {
    const opt = document.createElement("option");
    opt.value = s.id;
    opt.textContent = s.name;
    stationSelect.appendChild(opt);
  }
  if (datatypes.includes("TMAX")) datatypeSelect.value = "TMAX";

  await renderDaily();
}

async function renderDaily() {
  const datatype = datatypeSelect.value;
  const station = stationSelect.value; // "" means All
  const window = parseWindow(dailyWindowInput);
  const lowPass = parseWindow(dailyLowPassInput);
  const series = await getSeries(datatype, station, window, lowPass);

  const traces = seriesToTraces(
      series,
      `(${window}-day avg)`,
      `(${lowPass}-day low-pass)`,
      "lines",
      dailyShowRaw.checked
  );

  Plotly.newPlot(
      "chart",
      traces,
      {
        title: `${datatype} by station`,
        xaxis: { title: "Date" },
        yaxis: { title: datatype },
        margin: { t: 50 },
        autosize: true,
      },
      { responsive: true }
  );
}

datatypeSelect.addEventListener("change", renderDaily);
stationSelect.addEventListener("change", renderDaily);
dailyWindowInput.addEventListener("input", renderDaily);
dailyLowPassInput.addEventListener("input", renderDaily);
dailyShowRaw.addEventListener("change", renderDaily);

// ---- Trends view ----

const trendPeriodSelect = document.getElementById("trend-period");
const trendStationSelect = document.getElementById("trend-station");
const trendWindowInput = document.getElementById("trend-window");
const trendLowPassInput = document.getElementById("trend-lowpass");
const trendShowRaw = document.getElementById("trend-show-raw");

async function initTrends(stations) {
  for (const s of stations) {
    const opt = document.createElement("option");
    opt.value = s.id;
    opt.textContent = s.name;
    trendStationSelect.appendChild(opt);
  }
  await renderTrends();
}

async function renderTrends() {
  const period = trendPeriodSelect.value;
  const station = trendStationSelect.value; // "" means All
  const window = parseWindow(trendWindowInput);
  const lowPass = parseWindow(trendLowPassInput);
  const series = await getMeanTempTrend(period, station, window, lowPass);

  const traces = seriesToTraces(
      series,
      `(${window}-period avg)`,
      `(${lowPass}-period low-pass)`,
      "lines+markers",
      trendShowRaw.checked
  );

  Plotly.newPlot(
      "trend-chart",
      traces,
      {
        title: `Mean temperature by ${period === "yearly" ? "year" : "month"}`,
        xaxis: { title: period === "yearly" ? "Year" : "Month" },
        yaxis: { title: "Mean temp (TMAX+TMIN)/2" },
        margin: { t: 50 },
        autosize: true,
      },
      { responsive: true }
  );
}

trendPeriodSelect.addEventListener("change", renderTrends);
trendStationSelect.addEventListener("change", renderTrends);
trendWindowInput.addEventListener("input", renderTrends);
trendLowPassInput.addEventListener("input", renderTrends);
trendShowRaw.addEventListener("change", renderTrends);

// ---- Hot days view ----

const hotThresholdInput = document.getElementById("hot-threshold");
const hotStationSelect = document.getElementById("hot-station");

async function initHot(stations) {
  for (const s of stations) {
    const opt = document.createElement("option");
    opt.value = s.id;
    opt.textContent = s.name;
    hotStationSelect.appendChild(opt);
  }
  await renderHot();
}

async function renderHot() {
  const threshold = parseFloat(hotThresholdInput.value);
  const station = hotStationSelect.value; // "" means All
  if (!Number.isFinite(threshold)) return;
  const { days, fit } = await getHotDaysPerYear(threshold, station);

  const traces = seriesToTraces(days, null, null, "lines+markers", true);
  // Quadratic fit of the count over the modern record (>= 1980), dashed line
  // in the same hue as its station's raw data.
  fit.forEach((s, i) => {
    const color = PALETTE[i % PALETTE.length];
    if (s.points.length > 0) {
      traces.push({
        x: s.points.map((p) => p.date),
        y: s.points.map((p) => p.value),
        name: `${s.station_name} quadratic fit`,
        mode: "lines",
        type: "scatter",
        line: { color, width: 3, dash: "dash" },
      });
    }
  });

  Plotly.newPlot(
      "hot-chart",
      traces,
      {
        title: `Days with high above ${threshold}°F by year`,
        xaxis: { title: "Year" },
        yaxis: { title: "Days" },
        margin: { t: 50 },
        autosize: true,
      },
      { responsive: true }
  );
}

hotThresholdInput.addEventListener("input", renderHot);
hotStationSelect.addEventListener("change", renderHot);

// ---- Growing season view ----

const growingStationSelect = document.getElementById("growing-station");

async function initGrowing(stations) {
  for (const s of stations) {
    const opt = document.createElement("option");
    opt.value = s.id;
    opt.textContent = s.name;
    growingStationSelect.appendChild(opt);
  }
  await renderGrowing();
}

async function renderGrowing() {
  const station = growingStationSelect.value; // "" means All
  const { days, fit } = await getGrowingSeason(station);

  const traces = seriesToTraces(days, null, null, "lines+markers", true);
  // Cubic fit of the season length over the full record, dashed line in the
  // same hue as its station's raw data.
  fit.forEach((s, i) => {
    const color = PALETTE[i % PALETTE.length];
    if (s.points.length > 0) {
      traces.push({
        x: s.points.map((p) => p.date),
        y: s.points.map((p) => p.value),
        name: `${s.station_name} cubic fit`,
        mode: "lines",
        type: "scatter",
        line: { color, width: 3, dash: "dash" },
      });
    }
  });

  Plotly.newPlot(
      "growing-chart",
      traces,
      {
        title: "Growing season length by year",
        xaxis: { title: "Year" },
        yaxis: { title: "Days between last spring and first fall frost" },
        margin: { t: 50 },
        autosize: true,
      },
      { responsive: true }
  );
}

growingStationSelect.addEventListener("change", renderGrowing);

// ---- Init ----

async function init() {
  try {
    const [datatypes, stations] = await Promise.all([getDatatypes(), getStations()]);
    await initDaily(datatypes, stations);
    await initTrends(stations);
    await initHot(stations);
    await initGrowing(stations);
  } catch (err) {
    console.error("Failed to load stations/datatypes:", err);
    document.getElementById("chart").innerHTML =
        `<p style="color: #b00; font-family: sans-serif;">Failed to load data: ${err}</p>`;
  }
}

window.addEventListener("resize", () => {
  Plotly.Plots.resize(document.getElementById("chart"));
  Plotly.Plots.resize(document.getElementById("trend-chart"));
  Plotly.Plots.resize(document.getElementById("hot-chart"));
  Plotly.Plots.resize(document.getElementById("growing-chart"));
});

init();