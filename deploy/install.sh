#!/usr/bin/env bash
# Enable the weather plots web app at https://bicknellfamily.duckdns.org/CorvallisWeather/
# Requires the release build to exist first:
#   cd plot_data/server && cargo build --release
# Then run from the repo root with sudo:
#   sudo ./deploy/install.sh
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN=/home/bob/.local/bin/weather-plot-server
UNIT=/etc/systemd/system/weather-plot.service

echo ">> Installing binary to $BIN"
cp "$REPO/plot_data/server/target/release/weather-server" "$BIN"

echo ">> Installing systemd unit"
cp "$REPO/deploy/weather-plot.service" "$UNIT"

echo ">> Registering Caddy site https://bicknellfamily.duckdns.org/CorvallisWeather/"
if grep -q 'CorvallisWeather' /etc/caddy/Caddyfile; then
    echo "   CorvallisWeather block already present; leaving Caddyfile untouched"
else
    cat "$REPO/deploy/Caddyfile.weather-plot" >> /etc/caddy/Caddyfile
fi

if grep -q 'bicknellfamily.duckdns.org:9926' /etc/caddy/Caddyfile; then
    echo ">> NOTE: old :9926 site block still present in /etc/caddy/Caddyfile -- remove it manually"
fi

echo ">> Validating Caddy configuration"
caddy validate --adapter caddyfile --config /etc/caddy/Caddyfile

echo ">> Starting weather-plot and reloading Caddy"
systemctl daemon-reload
systemctl enable --now weather-plot
systemctl reload caddy

echo ">> Status:"
systemctl status weather-plot --no-pager | head -12