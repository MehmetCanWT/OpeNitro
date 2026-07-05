#!/usr/bin/env bash
set -euo pipefail

# ─── OpeNitro — Installer (No Systemd) ───

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}════════════════════════════════════════════════${NC}"
echo -e "${CYAN}      OpeNitro — Installer (No Systemd)${NC}"
echo -e "${CYAN}════════════════════════════════════════════════${NC}"

if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Error: You must run this script with root privileges!${NC}"
    echo "  → sudo ./install_no_systemd.sh"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="/opt/openitro"
BIN_DIR="/usr/local/bin"

# ─── 1. Copy Files ───
echo "[1/4] Copying files..."
mkdir -p "$INSTALL_DIR"
cp "$SCRIPT_DIR/openitro_ec.py"    "$INSTALL_DIR/"
cp "$SCRIPT_DIR/openitrod.py"      "$INSTALL_DIR/"
cp "$SCRIPT_DIR/openitro-cli.py"   "$INSTALL_DIR/"
cp "$SCRIPT_DIR/openitro-gui.py"   "$INSTALL_DIR/"
chmod 644 "$INSTALL_DIR/openitro_ec.py"
chmod 755 "$INSTALL_DIR/openitrod.py"
chmod 755 "$INSTALL_DIR/openitro-cli.py"
chmod 755 "$INSTALL_DIR/openitro-gui.py"

# ─── 2. Create Wrapper Scripts (Resolves sys.path issues) ───
echo "[2/4] Creating executable wrappers..."

# Daemon wrapper
cat > "$BIN_DIR/openitrod" << 'WRAPPER'
#!/usr/bin/env python3
import sys, os
sys.path.insert(0, "/opt/openitro")
exec(open("/opt/openitro/openitrod.py").read())
WRAPPER
chmod 755 "$BIN_DIR/openitrod"

# CLI
cat > "$BIN_DIR/openitro-cli" << 'WRAPPER'
#!/usr/bin/env python3
import sys, os
sys.path.insert(0, "/opt/openitro")
exec(open("/opt/openitro/openitro-cli.py").read())
WRAPPER
chmod 755 "$BIN_DIR/openitro-cli"

# GUI
cat > "$BIN_DIR/openitro-gui" << 'WRAPPER'
#!/usr/bin/env python3
import sys, os
sys.path.insert(0, "/opt/openitro")
exec(open("/opt/openitro/openitro-gui.py").read())
WRAPPER
chmod 755 "$BIN_DIR/openitro-gui"

# ─── 3. Icon ───
echo "[3/4] Copying application icon..."
if [ -f "$SCRIPT_DIR/openitro.png" ]; then
    cp "$SCRIPT_DIR/openitro.png" /usr/share/pixmaps/openitro.png
    chmod 644 /usr/share/pixmaps/openitro.png
else
    echo "  ⚠ openitro.png not found, skipping icon installation."
fi

# ─── 4. Desktop Entry ───
echo "[4/4] Creating desktop shortcut..."
cat > /usr/share/applications/openitro.desktop << 'EOF'
[Desktop Entry]
Type=Application
Name=OpeNitro
Comment=Acer Nitro Fan & Battery Controller
Exec=openitro-gui
Icon=openitro
Categories=System;Settings;Utility;
Terminal=false
StartupWMClass=openitro-gui
EOF
chmod 644 /usr/share/applications/openitro.desktop

echo ""
echo -e "${GREEN}════════════════════════════════════════════════${NC}"
echo -e "${GREEN}         Installation Completed Successfully!${NC}"
echo -e "${GREEN}════════════════════════════════════════════════${NC}"
echo ""
echo "  Note: Since your system does not run systemd, you must start the daemon manually"
echo "        or configure your own init script (OpenRC, runit, dinit, etc.)."
echo ""
echo "  To run the daemon manually:"
echo "    • sudo openitrod &"
echo ""
echo "  To use the applications:"
echo "    • Press the OpeNitro hotkey or search for 'OpeNitro' in your applications menu."
echo "    • Run 'openitro-cli --status' in your terminal."
echo ""
