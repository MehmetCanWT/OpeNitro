#!/usr/bin/env bash
set -euo pipefail

# ─── OpeNitro — Clean Up Legacy NitroSense Installation ───

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}════════════════════════════════════════════════${NC}"
echo -e "${CYAN}      Cleaning Legacy NitroSense Installation${NC}"
echo -e "${CYAN}════════════════════════════════════════════════${NC}"

if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Error: You must run this script with root privileges (sudo)!${NC}"
    echo "  → sudo ./cleanup_old.sh"
    exit 1
fi

echo "[1/5] Stopping and disabling old nitrosensed service..."
systemctl stop nitrosensed.service 2>/dev/null || true
systemctl disable nitrosensed.service 2>/dev/null || true

echo "[2/5] Removing old systemd service file..."
rm -f /etc/systemd/system/nitrosensed.service
systemctl daemon-reload

echo "[3/5] Removing old binaries and wrapper scripts..."
rm -f /usr/local/bin/nitrosensed
rm -f /usr/local/bin/nitrosense-cli
rm -f /usr/local/bin/nitrosense-gui

echo "[4/5] Removing old desktop shortcut and icon..."
rm -f /usr/share/applications/nitrosense.desktop
rm -f /usr/share/pixmaps/nitrosense.png

echo "[5/5] Removing old installation directory & config files..."
rm -rf /opt/nitrosense
rm -f /etc/nitrosense.json
rm -f /run/nitrosense.sock

echo -e "${GREEN}════════════════════════════════════════════════${NC}"
echo -e "${GREEN}      Legacy NitroSense cleaned up successfully!${NC}"
echo -e "${GREEN}════════════════════════════════════════════════${NC}"
