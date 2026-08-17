import { getStations, getDatatypes, getSeries } from "./api.js";

const datatypeSelect = document.getElementById("datatype");
const stationSelect = document.getElementById("station");

async function init() {
  const [datatypes, stations] = await Promise.all([getDatatypes(), getStations()]);

  datatypeSelect.innerHTML = datatypes
    .map((d) => `<option value="${d}">${d}</option>`)
    .join("");

  for (const s of stations) {
    const opt = document.createElement("option");
    opt.value = s.id;
    opt.textContent = s.name;
    stationSelect.appendChild(opt);
  }

  // Default to TMAX if present, otherwise whatever's first.
  if (datatypes.includes("TMAX")) datatypeSelect.value = "TMAX";

  await render();
}

async function render() {
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

  Plotly.newPlot("chart", traces, {
    title: `${datatype} by station`,
    xaxis: { title: "Date" },
    yaxis: { title: datatype },
    margin: { t: 50 },
  });
}

datatypeSelect.addEventListener("change", render);
stationSelect.addEventListener("change", render);

init();
