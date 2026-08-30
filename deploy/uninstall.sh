#!/usr/bin/env bash
# Stop and remove the weather plots web app. Run with sudo:
#   sudo ./deploy/uninstall.sh
set -euo pipefail

systemctl disable --now weather-plot 2>/dev/null || true
rm -f /etc/systemd/system/weather-plot.service
systemctl daemon-reload
rm -f /home/bob/.local/bin/weather-plot-server
echo "Removed weather-plot service and binary."
echo "The Caddyfile block for bicknellfamily.duckdns.org:9926 was left in place; remove it manually if you want."