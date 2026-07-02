#!/usr/bin/env python3
"""
openitro-gui.py - Graphical frontend for OpeNitro
Communicates with openitrod via UNIX socket. Single-instance aware.
"""

import json
import socket
import sys

from PyQt6 import QtCore, QtGui, QtWidgets
from PyQt6.QtNetwork import QLocalServer, QLocalSocket

SOCKET_PATH = "/run/openitro.sock"
SINGLE_INSTANCE_NAME = "openitro_gui_single_instance"
SOCKET_TIMEOUT = 2  # seconds


# ─── Custom Widgets ───


class FanWidget(QtWidgets.QWidget):
    """Animated spinning fan blade widget. Speed matches live RPM."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.angle = 0.0
        self.rpm = 0
        self._timer = QtCore.QTimer(self)
        self._timer.timeout.connect(self._tick)
        self._timer.start(16)  # ~60 FPS

    def set_rpm(self, rpm: int):
        self.rpm = rpm

    def _tick(self):
        if self.rpm > 0:
            speed = max(0.5, (self.rpm / 6000.0) * 12.0)
            self.angle = (self.angle + speed) % 360.0
            self.update()

    def paintEvent(self, event):
        painter = QtGui.QPainter(self)
        painter.setRenderHint(QtGui.QPainter.RenderHint.Antialiasing)

        w, h = self.width(), self.height()
        side = min(w, h)
        cx, cy = w / 2.0, h / 2.0

        # Outer ring
        painter.setPen(QtGui.QPen(QtGui.QColor(40, 40, 45), 3))
        painter.setBrush(QtGui.QColor(22, 22, 26))
        painter.drawEllipse(QtCore.QPointF(cx, cy), side / 2.0 - 5, side / 2.0 - 5)

        # Inner ring
        painter.setPen(QtGui.QPen(QtGui.QColor(80, 80, 80, 50), 1))
        painter.drawEllipse(QtCore.QPointF(cx, cy), side / 2.0 - 15, side / 2.0 - 15)

        painter.translate(cx, cy)
        painter.save()
        painter.rotate(self.angle)

        # Fan blades
        blade_color = QtGui.QColor(240, 84, 84, 160)
        painter.setBrush(blade_color)
        painter.setPen(QtCore.Qt.PenStyle.NoPen)

        for _ in range(9):
            path = QtGui.QPainterPath()
            path.moveTo(0, 0)
            path.cubicTo(
                side / 6.0, -side / 10.0,
                side / 3.0, -side / 8.0,
                side / 2.0 - 10, -side / 14.0,
            )
            path.lineTo(side / 2.0 - 10, side / 14.0)
            path.cubicTo(
                side / 3.0, side / 10.0,
                side / 6.0, side / 8.0,
                0, 0,
            )
            painter.drawPath(path)
            painter.rotate(40.0)

        painter.restore()

        # Center hub
        hub_grad = QtGui.QRadialGradient(0, 0, side / 8.0)
        hub_grad.setColorAt(0, QtGui.QColor(60, 60, 65))
        hub_grad.setColorAt(0.8, QtGui.QColor(30, 30, 32))
        hub_grad.setColorAt(1, QtGui.QColor(15, 15, 17))
        painter.setBrush(hub_grad)
        painter.setPen(QtGui.QPen(QtGui.QColor(240, 84, 84), 1.5))
        painter.drawEllipse(QtCore.QPointF(0, 0), side / 8.0, side / 8.0)


class TemperatureGauge(QtWidgets.QWidget):
    """Arc gauge showing temperature in °C with colour-coded indicator."""

    def __init__(self, title: str, parent=None):
        super().__init__(parent)
        self.title = title
        self.temp = 0

    def set_temp(self, temp: int):
        if self.temp != temp:
            self.temp = temp
            self.update()

    def paintEvent(self, event):
        painter = QtGui.QPainter(self)
        painter.setRenderHint(QtGui.QPainter.RenderHint.Antialiasing)

        w, h = self.width(), self.height()
        side = min(w, h)
        rect = QtCore.QRectF(
            (w - side) / 2.0 + 8, (h - side) / 2.0 + 8, side - 16, side - 16
        )

        # Background track
        painter.setPen(
            QtGui.QPen(
                QtGui.QColor(35, 35, 40), 6,
                QtCore.Qt.PenStyle.SolidLine, QtCore.Qt.PenCapStyle.RoundCap,
            )
        )
        painter.drawArc(rect, -135 * 16, -270 * 16)

        # Active arc
        angle_span = int(min(1.0, self.temp / 100.0) * 270)
        if self.temp >= 80:
            arc_color = QtGui.QColor(231, 76, 60)
        elif self.temp >= 65:
            arc_color = QtGui.QColor(241, 196, 15)
        else:
            arc_color = QtGui.QColor(46, 204, 113)
        painter.setPen(
            QtGui.QPen(
                arc_color, 6,
                QtCore.Qt.PenStyle.SolidLine, QtCore.Qt.PenCapStyle.RoundCap,
            )
        )
        if angle_span > 0:
            painter.drawArc(rect, -135 * 16, -angle_span * 16)

        # Temperature text
        painter.setPen(QtGui.QColor(240, 240, 245))
        painter.setFont(QtGui.QFont("Outfit", 18, QtGui.QFont.Weight.Bold))
        text_rect = rect.toRect()
        text_rect.setTop(text_rect.top() + 5)
        painter.drawText(text_rect, QtCore.Qt.AlignmentFlag.AlignCenter, f"{self.temp}°C")

        # Title text
        painter.setFont(QtGui.QFont("Outfit", 9))
        painter.setPen(QtGui.QColor(160, 160, 170))
        title_rect = rect.toRect()
        title_rect.setTop(title_rect.bottom() - 25)
        painter.drawText(title_rect, QtCore.Qt.AlignmentFlag.AlignCenter, self.title)


# ─── Toggle Switch Widget (replaces broken QCheckBox hack) ───


class ToggleSwitch(QtWidgets.QWidget):
    """Custom toggle switch widget drawn entirely with QPainter."""

    toggled = QtCore.pyqtSignal(bool)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._checked = False
        self._anim_pos = 0.0
        self.setFixedSize(50, 26)
        self.setCursor(QtCore.Qt.CursorShape.PointingHandCursor)

        self._animation = QtCore.QPropertyAnimation(self, b"handle_pos")
        self._animation.setDuration(120)
        self._animation.setEasingCurve(QtCore.QEasingCurve.Type.InOutCubic)

    def isChecked(self) -> bool:
        return self._checked

    def setChecked(self, checked: bool):
        if self._checked == checked:
            return
        self._checked = checked
        end = 1.0 if checked else 0.0
        self._animation.setStartValue(self._anim_pos)
        self._animation.setEndValue(end)
        self._animation.start()

    @QtCore.pyqtProperty(float)
    def handle_pos(self):
        return self._anim_pos

    @handle_pos.setter
    def handle_pos(self, value):
        self._anim_pos = value
        self.update()

    def mousePressEvent(self, event):
        self._checked = not self._checked
        end = 1.0 if self._checked else 0.0
        self._animation.setStartValue(self._anim_pos)
        self._animation.setEndValue(end)
        self._animation.start()
        self.toggled.emit(self._checked)

    def paintEvent(self, event):
        painter = QtGui.QPainter(self)
        painter.setRenderHint(QtGui.QPainter.RenderHint.Antialiasing)

        w, h = self.width(), self.height()
        radius = h / 2.0

        # Track
        track_color = QtGui.QColor(240, 84, 84) if self._checked else QtGui.QColor(45, 45, 53)
        painter.setPen(QtCore.Qt.PenStyle.NoPen)
        painter.setBrush(track_color)
        painter.drawRoundedRect(QtCore.QRectF(0, 0, w, h), radius, radius)

        # Handle
        handle_margin = 3
        handle_diameter = h - 2 * handle_margin
        travel = w - handle_diameter - 2 * handle_margin
        x = handle_margin + self._anim_pos * travel
        painter.setBrush(QtGui.QColor(255, 255, 255))
        painter.drawEllipse(QtCore.QRectF(x, handle_margin, handle_diameter, handle_diameter))


# ─── Main Window ───


class OpeNitroWindow(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        self.status_data: dict = {}
        self._build_ui()

        self._poll_timer = QtCore.QTimer(self)
        self._poll_timer.timeout.connect(self._request_status)
        self._poll_timer.start(1000)
        self._request_status()

    # ─── Socket communication ───

    def _send_command(self, cmd: str) -> dict:
        try:
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            sock.settimeout(SOCKET_TIMEOUT)
            sock.connect(SOCKET_PATH)
            sock.sendall(cmd.encode("utf-8"))
            resp = sock.recv(8192).decode("utf-8")
            sock.close()
            self._status_lbl.setText("Connected to openitrod daemon")
            self._status_lbl.setStyleSheet("font-size: 8pt; color: #606067;")
            return json.loads(resp)
        except Exception:
            self._status_lbl.setText("Connection error — is openitrod running?")
            self._status_lbl.setStyleSheet("font-size: 8pt; color: #FF4D4D;")
            return {}

    def _request_status(self):
        resp = self._send_command("GET_STATUS")
        if resp.get("status") == "success":
            self._update_ui(resp["data"])

    # ─── UI Builder ───

    def _build_ui(self):
        self.setWindowTitle("OpeNitro Controller")
        self.setFixedSize(720, 740)
        self.setObjectName("MainWindow")

        self.setStyleSheet(_STYLESHEET)

        central = QtWidgets.QWidget()
        central.setObjectName("MainWindow")
        self.setCentralWidget(central)

        root = QtWidgets.QVBoxLayout(central)
        root.setContentsMargins(20, 20, 20, 20)
        root.setSpacing(15)

        # Header
        header = QtWidgets.QHBoxLayout()
        title_box = QtWidgets.QVBoxLayout()
        lbl = QtWidgets.QLabel("OpeNitro")
        lbl.setStyleSheet("font-size: 20pt; font-weight: 800; color: #FFF; letter-spacing: 2px;")
        title_box.addWidget(lbl)
        sub = QtWidgets.QLabel("SYSTEM CONTROLLER")
        sub.setStyleSheet("font-size: 8pt; font-weight: 600; color: #F05454; letter-spacing: 1px;")
        title_box.addWidget(sub)
        header.addLayout(title_box)
        header.addStretch()

        self._power_src_lbl = QtWidgets.QLabel("—")
        self._power_src_lbl.setStyleSheet(
            "font-size: 9pt; color: #A0A0A5; font-weight: bold; "
            "background: #1E1E24; padding: 6px 12px; border-radius: 6px; "
            "border: 1px solid #2C2C35;"
        )
        header.addWidget(self._power_src_lbl)
        root.addLayout(header)

        # ── Performance Modes ──
        mode_grp = QtWidgets.QGroupBox("PERFORMANCE MODE")
        mode_lay = QtWidgets.QHBoxLayout(mode_grp)
        mode_lay.setContentsMargins(15, 15, 15, 15)
        mode_lay.setSpacing(10)

        self._btn_quiet = self._mode_btn("QUIET", lambda: self._set_power_mode("quiet"))
        self._btn_default = self._mode_btn("DEFAULT", lambda: self._set_power_mode("default"))
        self._btn_extreme = self._mode_btn("EXTREME", lambda: self._set_power_mode("extreme"))

        self._mode_group = QtWidgets.QButtonGroup(self)
        for b in (self._btn_quiet, self._btn_default, self._btn_extreme):
            self._mode_group.addButton(b)
            mode_lay.addWidget(b)
        root.addWidget(mode_grp)

        # ── Fan Controls ──
        fans = QtWidgets.QHBoxLayout()
        fans.setSpacing(15)

        # CPU
        cpu_grp, self._cpu_temp, self._cpu_fan, self._cpu_rpm_lbl, \
            self._btn_cpu_auto, self._btn_cpu_max, self._btn_cpu_manual, \
            self._cpu_slider, self._cpu_slider_lbl, self._cpu_btn_grp = \
            self._build_fan_group("CPU", "cpu")
        fans.addWidget(cpu_grp)

        # GPU
        gpu_grp, self._gpu_temp, self._gpu_fan, self._gpu_rpm_lbl, \
            self._btn_gpu_auto, self._btn_gpu_max, self._btn_gpu_manual, \
            self._gpu_slider, self._gpu_slider_lbl, self._gpu_btn_grp = \
            self._build_fan_group("GPU", "gpu")
        fans.addWidget(gpu_grp)

        root.addLayout(fans)

        # ── Battery Protection ──
        bat_grp = QtWidgets.QGroupBox("BATTERY PROTECTION")
        bat_lay = QtWidgets.QVBoxLayout(bat_grp)
        bat_lay.setContentsMargins(20, 15, 20, 15)
        bat_lay.setSpacing(12)

        row = QtWidgets.QHBoxLayout()
        desc = QtWidgets.QVBoxLayout()
        t = QtWidgets.QLabel("80% Charge Limit")
        t.setStyleSheet("font-size: 11pt; font-weight: bold; color: #FFF;")
        desc.addWidget(t)
        s = QtWidgets.QLabel("Preserves battery health when plugged in")
        s.setStyleSheet("font-size: 8pt; color: #A0A0A5;")
        desc.addWidget(s)
        row.addLayout(desc)
        row.addStretch()

        self._bat_toggle = ToggleSwitch()
        self._bat_toggle.toggled.connect(self._toggle_battery_limit)
        row.addWidget(self._bat_toggle)
        bat_lay.addLayout(row)
        root.addWidget(bat_grp)

        # Status
        self._status_lbl = QtWidgets.QLabel("Starting…")
        self._status_lbl.setStyleSheet("font-size: 8pt; color: #606067; margin-top: 5px;")
        root.addWidget(self._status_lbl)

    # ─── UI helpers ───

    @staticmethod
    def _mode_btn(text: str, slot) -> QtWidgets.QPushButton:
        btn = QtWidgets.QPushButton(text)
        btn.setCheckable(True)
        btn.setMinimumWidth(80)
        btn.clicked.connect(slot)
        return btn

    def _build_fan_group(self, label: str, unit: str):
        grp = QtWidgets.QGroupBox(f"{label} FAN CONTROL")
        lay = QtWidgets.QVBoxLayout(grp)
        lay.setContentsMargins(15, 15, 15, 15)
        lay.setSpacing(10)

        metrics = QtWidgets.QHBoxLayout()
        temp_gauge = TemperatureGauge(label)
        temp_gauge.setFixedSize(90, 90)
        fan_anim = FanWidget()
        fan_anim.setFixedSize(90, 90)
        metrics.addWidget(temp_gauge)
        metrics.addWidget(fan_anim)
        lay.addLayout(metrics)

        rpm_lbl = QtWidgets.QLabel("0 RPM")
        rpm_lbl.setAlignment(QtCore.Qt.AlignmentFlag.AlignCenter)
        rpm_lbl.setStyleSheet("font-size: 13pt; font-weight: bold; color: #FFF;")
        lay.addWidget(rpm_lbl)

        modes = QtWidgets.QHBoxLayout()
        btn_auto = self._mode_btn("AUTO", lambda _=None, u=unit: self._set_fan_mode(u, "auto"))
        btn_max = self._mode_btn("MAX", lambda _=None, u=unit: self._set_fan_mode(u, "turbo"))
        btn_manual = self._mode_btn("MANUAL", lambda _=None, u=unit: self._set_fan_mode(u, "manual"))
        btn_grp = QtWidgets.QButtonGroup(self)
        for b in (btn_auto, btn_max, btn_manual):
            btn_grp.addButton(b)
            modes.addWidget(b)
        lay.addLayout(modes)

        slider_row = QtWidgets.QHBoxLayout()
        slider_row.addWidget(QtWidgets.QLabel("Speed:"))
        slider = QtWidgets.QSlider(QtCore.Qt.Orientation.Horizontal)
        slider.setRange(0, 200)
        slider.sliderReleased.connect(lambda u=unit: self._set_fan_speed_manual(u))
        slider_row.addWidget(slider)
        slider_lbl = QtWidgets.QLabel("0")
        slider_lbl.setFixedWidth(35)
        slider.valueChanged.connect(lambda v, lbl=slider_lbl: lbl.setText(str(v)))
        slider_row.addWidget(slider_lbl)
        lay.addLayout(slider_row)

        return grp, temp_gauge, fan_anim, rpm_lbl, btn_auto, btn_max, btn_manual, slider, slider_lbl, btn_grp

    # ─── UI Update ───

    def _update_ui(self, data: dict):
        self.status_data = data

        # Power source
        plugged = data.get("power_plugged", False)
        charging = data.get("battery_charging", False)
        if plugged:
            self._power_src_lbl.setText("⚡ AC (Charging)" if charging else "⚡ AC")
        else:
            self._power_src_lbl.setText("🔋 Battery")

        # Power mode
        pmode = data.get("nitro_mode", "default")
        {"quiet": self._btn_quiet, "extreme": self._btn_extreme}.get(
            pmode, self._btn_default
        ).setChecked(True)

        # CPU
        self._update_fan_section(
            data, "cpu",
            self._cpu_temp, self._cpu_fan, self._cpu_rpm_lbl,
            self._btn_cpu_auto, self._btn_cpu_max, self._btn_cpu_manual,
            self._cpu_slider,
        )
        # GPU
        self._update_fan_section(
            data, "gpu",
            self._gpu_temp, self._gpu_fan, self._gpu_rpm_lbl,
            self._btn_gpu_auto, self._btn_gpu_max, self._btn_gpu_manual,
            self._gpu_slider,
        )

        # Battery toggle
        self._bat_toggle.blockSignals(True)
        self._bat_toggle.setChecked(data.get("battery_limit_active", False))
        self._bat_toggle.blockSignals(False)

    @staticmethod
    def _update_fan_section(data, unit, temp_w, fan_w, rpm_lbl, btn_auto, btn_max, btn_manual, slider):
        rpm = data.get(f"{unit}_rpm", 0)
        temp = data.get(f"{unit}_temp", 0)
        mode = data.get(f"{unit}_fan_mode", "auto")
        speed = data.get(f"{unit}_manual_speed", 100)

        rpm_lbl.setText(f"{rpm} RPM")
        temp_w.set_temp(temp)
        fan_w.set_rpm(rpm)

        if not slider.isSliderDown():
            slider.setValue(speed)

        if mode == "turbo":
            btn_max.setChecked(True)
            slider.setEnabled(False)
        elif mode == "manual":
            btn_manual.setChecked(True)
            slider.setEnabled(True)
        else:
            btn_auto.setChecked(True)
            slider.setEnabled(False)

    # ─── Actions ───

    def _set_power_mode(self, mode: str):
        self._send_command(f"SET_POWER_MODE {mode}")

    def _set_fan_mode(self, unit: str, mode: str):
        slider = self._cpu_slider if unit == "cpu" else self._gpu_slider
        slider.setEnabled(mode == "manual")
        self._send_command(f"SET_FAN_MODE {unit} {mode} {slider.value()}")

    def _set_fan_speed_manual(self, unit: str):
        slider = self._cpu_slider if unit == "cpu" else self._gpu_slider
        self._send_command(f"SET_FAN_MODE {unit} manual {slider.value()}")

    def _toggle_battery_limit(self, checked: bool):
        self._send_command(f"SET_BATTERY_LIMIT {'on' if checked else 'off'}")

    def closeEvent(self, event):
        self._poll_timer.stop()
        super().closeEvent(event)


# ─── Stylesheet ───

_STYLESHEET = """
QWidget#MainWindow { background-color: #111113; }
QLabel {
    color: #ECECF1;
    font-family: "Outfit", "Inter", "Segoe UI", sans-serif;
}
QGroupBox {
    border: 1px solid #232328;
    border-radius: 12px;
    background-color: #18181C;
    margin-top: 15px;
    padding: 10px;
}
QGroupBox::title {
    subcontrol-origin: margin;
    subcontrol-position: top center;
    padding: 0 10px;
    color: #F05454;
    font-weight: bold;
    font-size: 11pt;
}
QPushButton {
    background-color: #24242B;
    border: 1px solid #32323A;
    border-radius: 8px;
    color: #ECECF1;
    padding: 8px 16px;
    font-size: 9pt;
    font-weight: bold;
}
QPushButton:hover {
    background-color: #2E2E37;
    border-color: #F05454;
}
QPushButton:checked {
    background-color: qlineargradient(x1:0, y1:0, x2:1, y2:0, stop:0 #F05454, stop:1 #C92C2C);
    border: none;
    color: #FFFFFF;
}
QSlider::groove:horizontal {
    height: 6px;
    background: #2D2D35;
    border-radius: 3px;
}
QSlider::sub-page:horizontal {
    background: #F05454;
    border-radius: 3px;
}
QSlider::handle:horizontal {
    background: #ECECF1;
    width: 14px;
    margin-top: -4px;
    margin-bottom: -4px;
    border-radius: 7px;
}
QSlider::handle:horizontal:hover { background: #F05454; }
"""


# ─── Entry Point ───


def main():
    app = QtWidgets.QApplication(sys.argv)

    # Single-instance check via QLocalSocket / QLocalServer
    probe = QLocalSocket()
    probe.connectToServer(SINGLE_INSTANCE_NAME)
    if probe.waitForConnected(200):
        # Another instance exists — ask it to come to the front
        probe.write(b"WAKE")
        probe.waitForBytesWritten(1000)
        probe.close()
        sys.exit(0)

    server = QLocalServer()
    server.removeServer(SINGLE_INSTANCE_NAME)
    server.listen(SINGLE_INSTANCE_NAME)

    win = OpeNitroWindow()

    def _on_new_connection():
        client = server.nextPendingConnection()
        if client and client.waitForReadyRead(500):
            msg = client.readAll().data()
            if msg == b"WAKE":
                win.showNormal()
                win.activateWindow()
                win.raise_()
        if client:
            client.close()

    server.newConnection.connect(_on_new_connection)

    win.show()
    ret = app.exec()
    server.close()
    sys.exit(ret)


if __name__ == "__main__":
    main()
