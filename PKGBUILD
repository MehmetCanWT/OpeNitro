# Maintainer: trwinner9 <github-trwinner9@gmail.com>
pkgname=openitro-git
_pkgname=OpeNitro
pkgver=r1.g$(git rev-parse --short HEAD 2>/dev/null || echo "initial")
pkgrel=1
pkgdesc="A lightweight Acer Nitro controller for Linux (Fan speed and battery charging limit)"
arch=('any')
url="https://github.com/trwinner9/OpeNitro"
license=('MIT')
depends=('python' 'python-pyqt6')
makedepends=('git')
provides=('openitro')
conflicts=('openitro')
source=("git+https://github.com/trwinner9/OpeNitro.git")
sha256sums=('SKIP')

pkgver() {
  cd "$_pkgname"
  # Generate version based on git commit count and hash
  printf "r%s.%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
}

package() {
  cd "$_pkgname"

  # Create destination directories
  install -d "$pkgdir/opt/openitro"
  install -d "$pkgdir/usr/bin"
  install -d "$pkgdir/usr/share/applications"
  install -d "$pkgdir/usr/share/pixmaps"
  install -d "$pkgdir/usr/lib/systemd/system"

  # Install Python files
  cp openitro_ec.py "$pkgdir/opt/openitro/"
  cp openitrod.py "$pkgdir/opt/openitro/"
  cp openitro-cli.py "$pkgdir/opt/openitro/"
  cp openitro-gui.py "$pkgdir/opt/openitro/"

  chmod 644 "$pkgdir/opt/openitro/openitro_ec.py"
  chmod 755 "$pkgdir/opt/openitro/openitrod.py"
  chmod 755 "$pkgdir/opt/openitro/openitro-cli.py"
  chmod 755 "$pkgdir/opt/openitro/openitro-gui.py"

  # Create executable wrapper scripts under /usr/bin/ (instead of /usr/local/bin)
  # openitrod
  cat > "$pkgdir/usr/bin/openitrod" << 'EOF'
#!/usr/bin/env python3
import sys
sys.path.insert(0, "/opt/openitro")
exec(open("/opt/openitro/openitrod.py").read())
EOF
  # openitro-cli
  cat > "$pkgdir/usr/bin/openitro-cli" << 'EOF'
#!/usr/bin/env python3
import sys
sys.path.insert(0, "/opt/openitro")
exec(open("/opt/openitro/openitro-cli.py").read())
EOF
  # openitro-gui
  cat > "$pkgdir/usr/bin/openitro-gui" << 'EOF'
#!/usr/bin/env python3
import sys
sys.path.insert(0, "/opt/openitro")
exec(open("/opt/openitro/openitro-gui.py").read())
EOF

  chmod 755 "$pkgdir/usr/bin/openitrod"
  chmod 755 "$pkgdir/usr/bin/openitro-cli"
  chmod 755 "$pkgdir/usr/bin/openitro-gui"

  # Copy icon
  if [ -f openitro.png ]; then
      cp openitro.png "$pkgdir/usr/share/pixmaps/openitro.png"
      chmod 644 "$pkgdir/usr/share/pixmaps/openitro.png"
  fi

  # Create desktop entry
  cat > "$pkgdir/usr/share/applications/openitro.desktop" << 'EOF'
[Desktop Entry]
Type=Application
Name=OpeNitro
Comment=Acer Nitro Fan & Battery Controller
Exec=/usr/bin/openitro-gui
Icon=openitro
Categories=System;Settings;Utility;
Terminal=false
StartupWMClass=openitro-gui
EOF
  chmod 644 "$pkgdir/usr/share/applications/openitro.desktop"

  # Create systemd service (AUR guidelines put services in /usr/lib/systemd/system/)
  cat > "$pkgdir/usr/lib/systemd/system/openitrod.service" << 'EOF'
[Unit]
Description=OpeNitro Controller Daemon
After=multi-user.target

[Service]
Type=simple
ExecStart=/usr/bin/openitrod
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF
  chmod 644 "$pkgdir/usr/lib/systemd/system/openitrod.service"

  # Install license file
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
