# Deployment

Serves the weather plots app at
`https://bicknellfamily.duckdns.org/CorvallisWeather/`, using a path prefix
instead of a separate port. Caddy handles automatic HTTPS (Let's Encrypt via
the DuckDNS domain, HTTP-01 challenge over port 80) and strips the
`/CorvallisWeather` prefix before proxying to the Rust server on
`127.0.0.1:9002`; paths outside it get a 404. Plain HTTP requests are
redirected to HTTPS automatically.

## Layout

- Binary: `/home/bob/.local/bin/weather-plot-server` — release build of
  `plot_data/server`, served by systemd as user `bob`.
- Data: `~/.config/RobertBicknell/Weatheranalysis/weather_all.parquet`
  (`WEATHER_DATA_PATH` unset, so `plot-data-core` resolves its default).
- Static files: served from the repo's `plot_data/src` (same files as the
  Tauri desktop app; `api.js` picks Tauri vs REST automatically and resolves
  `/api` relative to the page's base path).

## Install

```
cd plot_data/server && cargo build --release
sudo ./deploy/install.sh
```

The script installs the binary, installs `weather-plot.service`, appends the
Caddy site (idempotent), validates the Caddyfile, starts the service and
reloads Caddy.

## Updating

```
cd plot_data/server && cargo build --release
sudo cp plot_data/server/target/release/weather-server /home/bob/.local/bin/weather-plot-server
sudo systemctl restart weather-plot
```

(Frontend-only changes need no rebuild — just `sudo systemctl restart` to pick
up the edited `plot_data/src` files, or leave it running; the files are read
per request.)

## Manual steps you still owe

- **Router**: forward TCP ports **80** and **443** to this machine if they
  aren't already (the existing Mealie/8083 ports forward the *other* public
  ports). Port 80 is required even though the site serves HTTPS — Caddy uses
  it for the Let's Encrypt HTTP-01 challenge and to redirect plain-HTTP
  requests.
- **DNS**: `bicknellfamily.duckdns.org` already resolves to this box.

## Uninstall

```
sudo ./deploy/uninstall.sh
```