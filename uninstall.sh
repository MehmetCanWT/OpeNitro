#!/usr/bin/env bash
set -euo pipefail

# ─── OpeNitro — Uninstaller ───

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}════════════════════════════════════════════════${NC}"
echo -e "${CYAN}             OpeNitro — Uninstaller${NC}"
echo -e "${CYAN}════════════════════════════════════════════════${NC}"

if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Error: You must run this script with root privileges (sudo)!${NC}"
    echo "  → sudo ./uninstall.sh"
    exit 1
fi

echo "[1/5] Stopping and disabling background services..."
# Disable OpeNitro
systemctl stop openitrod.service 2>/dev/null || true
systemctl disable openitrod.service 2>/dev/null || true
# Disable legacy nitrosense if present
systemctl stop nitrosensed.service 2>/dev/null || true
systemctl disable nitrosensed.service 2>/dev/null || true

echo "[2/5] Removing systemd service files..."
rm -f /etc/systemd/system/openitrod.service
rm -f /etc/systemd/system/nitrosensed.service
systemctl daemon-reload

echo "[3/5] Removing executables and wrapper scripts..."
# OpeNitro
rm -f /usr/local/bin/openitrod
rm -f /usr/local/bin/openitro-cli
rm -f /usr/local/bin/openitro-gui
# Legacy
rm -f /usr/local/bin/nitrosensed
rm -f /usr/local/bin/nitrosense-cli
rm -f /usr/local/bin/nitrosense-gui

echo "[4/5] Removing desktop shortcuts and icons..."
# OpeNitro
rm -f /usr/share/applications/openitro.desktop
rm -f /usr/share/pixmaps/openitro.png
# Legacy
rm -f /usr/share/applications/nitrosense.desktop
rm -f /usr/share/pixmaps/nitrosense.png

echo "[5/5] Cleaning up installation folders, configurations, and sockets..."
# OpeNitro
rm -rf /opt/openitro
rm -f /etc/openitro.json
rm -f /run/openitro.sock
# Legacy
rm -rf /opt/nitrosense
rm -f /etc/nitrosense.json
rm -f /run/nitrosense.sock

echo -e "${GREEN}════════════════════════════════════════════════${NC}"
echo -e "${GREEN}       Uninstall Completed Successfully!${NC}"
echo -e "${GREEN}════════════════════════════════════════════════${NC}"
