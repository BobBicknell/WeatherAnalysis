import { getStations, getDatatypes, getSeries, getMeanTempTrend } from "./api.js";

// ---- Tab switching ----

const tabButtons = document.querySelectorAll(".tab-btn");
const views = {
  daily: document.getElementById("daily-view"),
  trends: document.getElementById("trends-view"),
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
    });
  });
});

// ---- Daily view ----

const datatypeSelect = document.getElementById("datatype");
const stationSelect = document.getElementById("station");

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
  const series = await getSeries(datatype, station);

  const traces = series.map((s) => ({
    x: s.points.map((p) => p.date),
    y: s.points.map((p) => p.value),
    name: s.station_name,
    mode: "lines",
    type: "scatter",
  }));

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

// ---- Trends view ----

const trendPeriodSelect = document.getElementById("trend-period");
const trendStationSelect = document.getElementById("trend-station");

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
  const series = await getMeanTempTrend(period, station);

  const traces = series.map((s) => ({
    x: s.points.map((p) => p.date),
    y: s.points.map((p) => p.value),
    name: s.station_name,
    mode: "lines+markers",
    type: "scatter",
  }));

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

// ---- Init ----

async function init() {
  try {
    const [datatypes, stations] = await Promise.all([getDatatypes(), getStations()]);
    await initDaily(datatypes, stations);
    await initTrends(stations);
  } catch (err) {
    console.error("Failed to load stations/datatypes:", err);
    document.getElementById("chart").innerHTML =
        `<p style="color: #b00; font-family: sans-serif;">Failed to load data: ${err}</p>`;
  }
}

window.addEventListener("resize", () => {
  Plotly.Plots.resize(document.getElementById("chart"));
  Plotly.Plots.resize(document.getElementById("trend-chart"));
});

init();