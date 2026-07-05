#!/usr/bin/env python3
"""
openitro-gui.py - Graphical frontend for OpeNitro
Communicates with openitrod via UNIX socket. Single-instance aware.
"""

import json
import os
import socket
import sys
import time

from PyQt6 import QtCore, QtGui, QtWidgets
from PyQt6.QtNetwork import QLocalServer, QLocalSocket

SOCKET_PATH = "/run/openitro.sock"
SINGLE_INSTANCE_NAME = "openitro_gui_single_instance"
SOCKET_TIMEOUT = 2  # seconds


class ClickSlider(QtWidgets.QSlider):
    """QSlider that jumps directly to the clicked position and supports dragging."""

    def mousePressEvent(self, event: QtGui.QMouseEvent):
        if event.button() == QtCore.Qt.MouseButton.LeftButton:
            val = QtWidgets.QStyle.sliderValueFromPosition(
                self.minimum(),
                self.maximum(),
                int(event.position().x()),
                self.width()
            )
            self.setValue(val)
            self.sliderMoved.emit(val)
            
            span = self.maximum() - self.minimum()
            handle_x = int((val - self.minimum()) / span * self.width()) if span > 0 else 0
            event = QtGui.QMouseEvent(
                event.type(),
                QtCore.QPointF(handle_x, event.position().y()),
                event.button(),
                event.buttons(),
                event.modifiers()
            )
        super().mousePressEvent(event)


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
        blade_color = QtGui.QColor(255, 62, 108, 160)
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
        painter.setPen(QtGui.QPen(QtGui.QColor(255, 62, 108), 1.5))
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
            arc_color = QtGui.QColor(255, 62, 108)
        elif self.temp >= 65:
            arc_color = QtGui.QColor(241, 196, 15)
        else:
            arc_color = QtGui.QColor(0, 226, 154)
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
        track_color = QtGui.QColor(255, 62, 108) if self._checked else QtGui.QColor(28, 28, 36)
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


class RGBDialog(QtWidgets.QDialog):
    """Standalone floating dialog for WMI RGB Keyboard settings."""

    def __init__(self, send_cmd_func, parent=None):
        super().__init__(parent)
        self.send_cmd = send_cmd_func
        self.setWindowTitle("OpeNitro RGB Customizer")
        self.setFixedSize(580, 310)
        self.setStyleSheet(_STYLESHEET)
        self.setModal(False)

        lay = QtWidgets.QVBoxLayout(self)
        lay.setContentsMargins(15, 15, 15, 15)

        self._rgb_grp = QtWidgets.QGroupBox("RGB KEYBOARD CONTROL")
        rgb_lay = QtWidgets.QHBoxLayout(self._rgb_grp)
        rgb_lay.setContentsMargins(15, 12, 15, 12)
        rgb_lay.setSpacing(15)

        # Left column: controls
        left_col = QtWidgets.QVBoxLayout()
        left_col.setSpacing(6)

        mode_zone_lay = QtWidgets.QHBoxLayout()
        self._rgb_mode_combo = QtWidgets.QComboBox()
        self._rgb_mode_combo.addItems(["Static", "Breath", "Neon", "Wave", "Shifting", "Zoom"])
        self._rgb_mode_combo.currentIndexChanged.connect(self._on_rgb_config_changed)
        mode_zone_lay.addWidget(QtWidgets.QLabel("Mode:"))
        mode_zone_lay.addWidget(self._rgb_mode_combo)

        self._rgb_zone_lbl = QtWidgets.QLabel("Zone:")
        self._rgb_zone_combo = QtWidgets.QComboBox()
        self._rgb_zone_combo.addItems(["Zone 1", "Zone 2", "Zone 3", "Zone 4"])
        self._rgb_zone_combo.currentIndexChanged.connect(self._on_rgb_config_changed)
        mode_zone_lay.addWidget(self._rgb_zone_lbl)
        mode_zone_lay.addWidget(self._rgb_zone_combo)
        left_col.addLayout(mode_zone_lay)

        bright_lay = QtWidgets.QHBoxLayout()
        bright_lay.addWidget(QtWidgets.QLabel("Bright:"))
        self._rgb_bright_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self._rgb_bright_slider.setRange(0, 100)
        self._rgb_bright_slider.sliderReleased.connect(self._on_rgb_config_changed)
        self._rgb_bright_lbl = QtWidgets.QLabel("100")
        self._rgb_bright_lbl.setFixedWidth(25)
        self._rgb_bright_slider.valueChanged.connect(lambda v: self._rgb_bright_lbl.setText(str(v)))
        bright_lay.addWidget(self._rgb_bright_slider)
        bright_lay.addWidget(self._rgb_bright_lbl)
        left_col.addLayout(bright_lay)

        speed_dir_lay = QtWidgets.QHBoxLayout()
        self._rgb_speed_title = QtWidgets.QLabel("Speed:")
        self._rgb_speed_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self._rgb_speed_slider.setRange(1, 9)
        self._rgb_speed_slider.sliderReleased.connect(self._on_rgb_config_changed)
        self._rgb_speed_lbl = QtWidgets.QLabel("4")
        self._rgb_speed_lbl.setFixedWidth(15)
        self._rgb_speed_slider.valueChanged.connect(lambda v: self._rgb_speed_lbl.setText(str(v)))
        speed_dir_lay.addWidget(self._rgb_speed_title)
        speed_dir_lay.addWidget(self._rgb_speed_slider)
        speed_dir_lay.addWidget(self._rgb_speed_lbl)

        self._rgb_dir_lbl = QtWidgets.QLabel("Dir:")
        self._rgb_dir_combo = QtWidgets.QComboBox()
        self._rgb_dir_combo.addItems(["R ➔ L", "L ➔ R"])
        self._rgb_dir_combo.currentIndexChanged.connect(self._on_rgb_config_changed)
        speed_dir_lay.addWidget(self._rgb_dir_lbl)
        speed_dir_lay.addWidget(self._rgb_dir_combo)
        left_col.addLayout(speed_dir_lay)
        rgb_lay.addLayout(left_col, 3)

        # Right column: Color selection & preview
        right_col = QtWidgets.QVBoxLayout()
        right_col.setSpacing(4)
        
        self._rgb_preview = QtWidgets.QFrame()
        self._rgb_preview.setFixedSize(65, 45)
        self._rgb_preview.setStyleSheet("border-radius: 6px; border: 1.5px solid #2C2C39; background-color: #FF3E6C;")

        color_sliders_lay = QtWidgets.QVBoxLayout()
        color_sliders_lay.setSpacing(4)

        r_lay = QtWidgets.QHBoxLayout()
        r_lay.addWidget(QtWidgets.QLabel("R:"))
        self._rgb_r_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self._rgb_r_slider.setRange(0, 255)
        self._rgb_r_slider.sliderReleased.connect(self._on_rgb_config_changed)
        self._rgb_r_slider.valueChanged.connect(lambda v: self._rgb_r_lbl.setText(str(v)))
        self._rgb_r_slider.valueChanged.connect(lambda v: self._update_color_preview())
        self._rgb_r_lbl = QtWidgets.QLabel("255")
        self._rgb_r_lbl.setFixedWidth(25)
        r_lay.addWidget(self._rgb_r_slider)
        r_lay.addWidget(self._rgb_r_lbl)
        color_sliders_lay.addLayout(r_lay)

        g_lay = QtWidgets.QHBoxLayout()
        g_lay.addWidget(QtWidgets.QLabel("G:"))
        self._rgb_g_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self._rgb_g_slider.setRange(0, 255)
        self._rgb_g_slider.sliderReleased.connect(self._on_rgb_config_changed)
        self._rgb_g_slider.valueChanged.connect(lambda v: self._rgb_g_lbl.setText(str(v)))
        self._rgb_g_slider.valueChanged.connect(lambda v: self._update_color_preview())
        self._rgb_g_lbl = QtWidgets.QLabel("62")
        self._rgb_g_lbl.setFixedWidth(25)
        g_lay.addWidget(self._rgb_g_slider)
        g_lay.addWidget(self._rgb_g_lbl)
        color_sliders_lay.addLayout(g_lay)

        b_lay = QtWidgets.QHBoxLayout()
        b_lay.addWidget(QtWidgets.QLabel("B:"))
        self._rgb_b_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self._rgb_b_slider.setRange(0, 255)
        self._rgb_b_slider.sliderReleased.connect(self._on_rgb_config_changed)
        self._rgb_b_slider.valueChanged.connect(lambda v: self._rgb_b_lbl.setText(str(v)))
        self._rgb_b_slider.valueChanged.connect(lambda v: self._update_color_preview())
        self._rgb_b_lbl = QtWidgets.QLabel("108")
        self._rgb_b_lbl.setFixedWidth(25)
        b_lay.addWidget(self._rgb_b_slider)
        b_lay.addWidget(self._rgb_b_lbl)
        color_sliders_lay.addLayout(b_lay)

        color_picker_lay = QtWidgets.QHBoxLayout()
        color_picker_lay.addLayout(color_sliders_lay, 3)
        color_picker_lay.addWidget(self._rgb_preview, 1, alignment=QtCore.Qt.AlignmentFlag.AlignCenter)
        right_col.addLayout(color_picker_lay)
        rgb_lay.addLayout(right_col, 2)

        lay.addWidget(self._rgb_grp)

    def _update_rgb_ui_elements(self):
        mode = self._rgb_mode_combo.currentIndex()
        is_static = (mode == 0)
        self._rgb_zone_combo.setEnabled(is_static)
        self._rgb_zone_combo.setVisible(is_static)
        self._rgb_zone_lbl.setVisible(is_static)

        has_speed = (mode > 0)
        self._rgb_speed_slider.setEnabled(has_speed)
        self._rgb_speed_slider.setVisible(has_speed)
        self._rgb_speed_lbl.setVisible(has_speed)
        self._rgb_speed_title.setVisible(has_speed)

        has_dir = (mode == 3)
        self._rgb_dir_combo.setEnabled(has_dir)
        self._rgb_dir_combo.setVisible(has_dir)
        self._rgb_dir_lbl.setVisible(has_dir)

        has_color = (mode in (0, 1, 4, 5))
        self._rgb_r_slider.setEnabled(has_color)
        self._rgb_g_slider.setEnabled(has_color)
        self._rgb_b_slider.setEnabled(has_color)
        self._rgb_r_slider.setVisible(has_color)
        self._rgb_g_slider.setVisible(has_color)
        self._rgb_b_slider.setVisible(has_color)
        self._rgb_preview.setVisible(has_color)

    def _update_color_preview(self):
        r = self._rgb_r_slider.value()
        g = self._rgb_g_slider.value()
        b = self._rgb_b_slider.value()
        self._rgb_preview.setStyleSheet(
            f"border-radius: 6px; border: 1.5px solid #2C2C39; "
            f"background-color: rgb({r}, {g}, {b});"
        )

    def _on_rgb_config_changed(self):
        mode = self._rgb_mode_combo.currentIndex()
        r = self._rgb_r_slider.value()
        g = self._rgb_g_slider.value()
        b = self._rgb_b_slider.value()
        bright = self._rgb_bright_slider.value()
        speed = self._rgb_speed_slider.value()
        direction = self._rgb_dir_combo.currentIndex() + 1
        zone = self._rgb_zone_combo.currentIndex() + 1

        self._update_rgb_ui_elements()
        self._update_color_preview()

        self.send_cmd(f"SET_RGB {mode} {r} {g} {b} {bright} {speed} {direction} {zone}")

    def update_data(self, data: dict):
        for w in (self._rgb_mode_combo, self._rgb_zone_combo, self._rgb_bright_slider,
                  self._rgb_speed_slider, self._rgb_dir_combo, self._rgb_r_slider,
                  self._rgb_g_slider, self._rgb_b_slider):
            w.blockSignals(True)

        self._rgb_mode_combo.setCurrentIndex(data.get("rgb_mode", 0))
        self._rgb_zone_combo.setCurrentIndex(data.get("rgb_zone", 1) - 1)
        self._rgb_bright_slider.setValue(data.get("rgb_brightness", 100))
        self._rgb_bright_lbl.setText(str(data.get("rgb_brightness", 100)))
        self._rgb_speed_slider.setValue(data.get("rgb_speed", 4))
        self._rgb_speed_lbl.setText(str(data.get("rgb_speed", 4)))
        self._rgb_dir_combo.setCurrentIndex(data.get("rgb_direction", 1) - 1)
        self._rgb_r_slider.setValue(data.get("rgb_red", 255))
        self._rgb_r_lbl.setText(str(data.get("rgb_red", 255)))
        self._rgb_g_slider.setValue(data.get("rgb_green", 62))
        self._rgb_g_lbl.setText(str(data.get("rgb_green", 62)))
        self._rgb_b_slider.setValue(data.get("rgb_blue", 108))
        self._rgb_b_lbl.setText(str(data.get("rgb_blue", 108)))

        for w in (self._rgb_mode_combo, self._rgb_zone_combo, self._rgb_bright_slider,
                  self._rgb_speed_slider, self._rgb_dir_combo, self._rgb_r_slider,
                  self._rgb_g_slider, self._rgb_b_slider):
            w.blockSignals(False)

        self._update_rgb_ui_elements()
        self._update_color_preview()


# ─── Main Window ───


class OpeNitroWindow(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        self.status_data: dict = {}
        self.rgb_dialog = None
        self._last_slider_change = {"cpu": 0.0, "gpu": 0.0}
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
        self.setFixedWidth(840)
        self.setObjectName("MainWindow")

        # Window Icon
        icon_path = "/usr/share/pixmaps/openitro.png"
        if not os.path.exists(icon_path):
            local_icon = os.path.join(os.path.dirname(os.path.abspath(__file__)), "openitro.png")
            if os.path.exists(local_icon):
                icon_path = local_icon
            else:
                icon_path = "/opt/openitro/openitro.png"
        if os.path.exists(icon_path):
            self.setWindowIcon(QtGui.QIcon(icon_path))

        self.setStyleSheet(_STYLESHEET)

        central = QtWidgets.QWidget()
        central.setObjectName("MainWindow")
        self.setCentralWidget(central)

        root = QtWidgets.QVBoxLayout(central)
        root.setContentsMargins(20, 20, 20, 20)
        root.setSpacing(25)

        # Header
        header = QtWidgets.QHBoxLayout()
        title_box = QtWidgets.QVBoxLayout()
        lbl = QtWidgets.QLabel("OpeNitro")
        lbl.setStyleSheet("font-size: 20pt; font-weight: 800; color: #FFF; letter-spacing: 2px;")
        title_box.addWidget(lbl)
        sub = QtWidgets.QLabel("SYSTEM CONTROLLER")
        sub.setStyleSheet("font-size: 8pt; font-weight: 600; color: #F05454; letter-spacing: 1px;")
        title_box.addWidget(sub)
        
        self._model_lbl = QtWidgets.QLabel("Model: —")
        self._model_lbl.setStyleSheet("font-size: 8pt; color: #808085; font-weight: bold; margin-top: 4px;")
        self._serial_lbl = QtWidgets.QLabel("S/N: —")
        self._serial_lbl.setStyleSheet("font-size: 8pt; color: #808085; font-weight: bold;")
        title_box.addWidget(self._model_lbl)
        title_box.addWidget(self._serial_lbl)
        header.addLayout(title_box)
        header.addStretch()

        self._rgb_btn = QtWidgets.QPushButton("⌨ RGB Keyboard")
        self._rgb_btn.setObjectName("RGBButton")
        self._rgb_btn.clicked.connect(self._open_rgb_dialog)
        self._rgb_btn.hide()
        header.addWidget(self._rgb_btn, alignment=QtCore.Qt.AlignmentFlag.AlignVCenter)

        self._power_src_lbl = QtWidgets.QLabel("—")
        self._power_src_lbl.setStyleSheet(
            "font-size: 8pt; color: #A0A0A5; font-weight: bold; "
            "background: #1E1E24; padding: 4px 10px; border-radius: 6px; "
            "border: 1px solid #2C2C35;"
        )
        header.addWidget(self._power_src_lbl, alignment=QtCore.Qt.AlignmentFlag.AlignVCenter)
        root.addLayout(header)

        # ── Performance Modes ──
        mode_grp = QtWidgets.QGroupBox("PERFORMANCE MODE")
        mode_lay = QtWidgets.QHBoxLayout(mode_grp)
        mode_lay.setContentsMargins(15, 25, 15, 15)
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

        # ── System Settings & Health ──
        sys_grp = QtWidgets.QGroupBox("SYSTEM SETTINGS && HEALTH")
        sys_lay = QtWidgets.QVBoxLayout(sys_grp)
        sys_lay.setContentsMargins(20, 25, 20, 15)
        sys_lay.setSpacing(10)

        # Row 1: Battery limit
        row1 = QtWidgets.QHBoxLayout()
        desc1 = QtWidgets.QVBoxLayout()
        t1 = QtWidgets.QLabel("80% Charge Limit")
        t1.setStyleSheet("font-size: 11pt; font-weight: bold; color: #FFF;")
        desc1.addWidget(t1)
        s1 = QtWidgets.QLabel("Preserves battery health by stopping charge at 80%")
        s1.setStyleSheet("font-size: 8pt; color: #A0A0A5;")
        desc1.addWidget(s1)
        row1.addLayout(desc1)
        row1.addStretch()

        self._bat_toggle = ToggleSwitch()
        self._bat_toggle.toggled.connect(self._toggle_battery_limit)
        row1.addWidget(self._bat_toggle)
        sys_lay.addLayout(row1)

        # Divider 1
        div1 = QtWidgets.QFrame()
        div1.setFrameShape(QtWidgets.QFrame.Shape.HLine)
        div1.setFrameShadow(QtWidgets.QFrame.Shadow.Sunken)
        div1.setStyleSheet("background-color: #232328; max-height: 1px; border: none;")
        sys_lay.addWidget(div1)

        # Row 2: CoolBoost
        row2 = QtWidgets.QHBoxLayout()
        desc2 = QtWidgets.QVBoxLayout()
        t2 = QtWidgets.QLabel("Acer CoolBoost")
        t2.setStyleSheet("font-size: 11pt; font-weight: bold; color: #FFF;")
        desc2.addWidget(t2)
        s2 = QtWidgets.QLabel("Delivers higher maximum fan speed and cooling under load")
        s2.setStyleSheet("font-size: 8pt; color: #A0A0A5;")
        desc2.addWidget(s2)
        row2.addLayout(desc2)
        row2.addStretch()

        self._cb_toggle = ToggleSwitch()
        self._cb_toggle.toggled.connect(self._toggle_coolboost)
        row2.addWidget(self._cb_toggle)
        sys_lay.addLayout(row2)

        # Divider 2
        div2 = QtWidgets.QFrame()
        div2.setFrameShape(QtWidgets.QFrame.Shape.HLine)
        div2.setFrameShadow(QtWidgets.QFrame.Shadow.Sunken)
        div2.setStyleSheet("background-color: #232328; max-height: 1px; border: none;")
        sys_lay.addWidget(div2)

        # Row 3: Keyboard timeout
        row3 = QtWidgets.QHBoxLayout()
        desc3 = QtWidgets.QVBoxLayout()
        t3 = QtWidgets.QLabel("Keyboard Backlight Timeout")
        t3.setStyleSheet("font-size: 11pt; font-weight: bold; color: #FFF;")
        desc3.addWidget(t3)
        s3 = QtWidgets.QLabel("Automatically turns off the keyboard backlight after 30 seconds of inactivity")
        s3.setStyleSheet("font-size: 8pt; color: #A0A0A5;")
        desc3.addWidget(s3)
        row3.addLayout(desc3)
        row3.addStretch()

        self._kb_toggle = ToggleSwitch()
        self._kb_toggle.toggled.connect(self._toggle_kb_timeout)
        row3.addWidget(self._kb_toggle)
        sys_lay.addLayout(row3)

        # Divider 3
        div3 = QtWidgets.QFrame()
        div3.setFrameShape(QtWidgets.QFrame.Shape.HLine)
        div3.setFrameShadow(QtWidgets.QFrame.Shadow.Sunken)
        div3.setStyleSheet("background-color: #232328; max-height: 1px; border: none;")
        sys_lay.addWidget(div3)

        # Row 4: USB Power-off Charging
        row4 = QtWidgets.QHBoxLayout()
        desc4 = QtWidgets.QVBoxLayout()
        t4 = QtWidgets.QLabel("USB Power-off Charging")
        t4.setStyleSheet("font-size: 11pt; font-weight: bold; color: #FFF;")
        desc4.addWidget(t4)
        s4 = QtWidgets.QLabel("Allows charging external devices from USB ports when laptop is shut down")
        s4.setStyleSheet("font-size: 8pt; color: #A0A0A5;")
        desc4.addWidget(s4)
        row4.addLayout(desc4)
        row4.addStretch()

        self._usb_toggle = ToggleSwitch()
        self._usb_toggle.toggled.connect(self._toggle_usb_charge)
        row4.addWidget(self._usb_toggle)
        sys_lay.addLayout(row4)

        root.addWidget(sys_grp)

        # Status
        self._status_lbl = QtWidgets.QLabel("Starting…")
        self._status_lbl.setStyleSheet("font-size: 8pt; color: #606067; margin-top: 5px;")
        root.addWidget(self._status_lbl)

        self.adjustSize()

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
        lay.setContentsMargins(15, 25, 15, 15)
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
        slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        slider.setRange(0, 200)
        slider.sliderPressed.connect(lambda u=unit: self._register_slider_interaction(u))
        slider.sliderMoved.connect(lambda v, u=unit: self._register_slider_interaction(u))
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
            self._power_src_lbl.setStyleSheet(
                "font-size: 8pt; color: #39C481; font-weight: bold; "
                "background: #12281D; padding: 4px 10px; border-radius: 6px; "
                "border: 1px solid #1B452D;"
            )
        else:
            self._power_src_lbl.setText("🔋 Battery")
            self._power_src_lbl.setStyleSheet(
                "font-size: 8pt; color: #FFB340; font-weight: bold; "
                "background: #2A1F12; padding: 4px 10px; border-radius: 6px; "
                "border: 1px solid #4D331A;"
            )

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

        # CoolBoost toggle
        self._cb_toggle.blockSignals(True)
        self._cb_toggle.setChecked(data.get("coolboost_active", False))
        self._cb_toggle.blockSignals(False)

        # Keyboard timeout toggle
        self._kb_toggle.blockSignals(True)
        self._kb_toggle.setChecked(data.get("kb_backlight_timeout", True))
        self._kb_toggle.blockSignals(False)

        # USB charge toggle
        self._usb_toggle.blockSignals(True)
        self._usb_toggle.setChecked(data.get("usb_charge_poweroff", False))
        self._usb_toggle.blockSignals(False)

        # Model & Serial Info display
        self._model_lbl.setText(f"Model: {data.get('model', 'Unknown')}")
        self._serial_lbl.setText(f"S/N: {data.get('product_serial', 'Unknown')}")

        # RGB keyboard dynamic section button & update
        rgb_sup = data.get("rgb_supported", False)
        self._rgb_btn.setVisible(rgb_sup)
        if rgb_sup and self.rgb_dialog and self.rgb_dialog.isVisible():
            self.rgb_dialog.update_data(data)

    def _register_slider_interaction(self, unit: str):
        self._last_slider_change[unit] = time.time()

    def _update_fan_section(self, data, unit, temp_w, fan_w, rpm_lbl, btn_auto, btn_max, btn_manual, slider):
        rpm = data.get(f"{unit}_rpm", 0)
        temp = data.get(f"{unit}_temp", 0)
        mode = data.get(f"{unit}_fan_mode", "auto")
        speed = data.get(f"{unit}_manual_speed", 100)

        rpm_lbl.setText(f"{rpm} RPM")
        temp_w.set_temp(temp)
        fan_w.set_rpm(rpm)

        if time.time() - self._last_slider_change.get(unit, 0.0) >= 2.5:
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
        if mode == "manual":
            self._last_slider_change[unit] = time.time()
        self._send_command(f"SET_FAN_MODE {unit} {mode} {slider.value()}")

    def _set_fan_speed_manual(self, unit: str):
        slider = self._cpu_slider if unit == "cpu" else self._gpu_slider
        self._last_slider_change[unit] = time.time()
        self._send_command(f"SET_FAN_MODE {unit} manual {slider.value()}")

    def _toggle_battery_limit(self, checked: bool):
        self._send_command(f"SET_BATTERY_LIMIT {'on' if checked else 'off'}")

    def _toggle_coolboost(self, checked: bool):
        self._send_command(f"SET_COOLBOOST {'on' if checked else 'off'}")

    def _toggle_kb_timeout(self, checked: bool):
        self._send_command(f"SET_KB_TIMEOUT {'on' if checked else 'off'}")

    def _toggle_usb_charge(self, checked: bool):
        self._send_command(f"SET_USB_CHARGE {'on' if checked else 'off'}")

    def _open_rgb_dialog(self):
        if not self.rgb_dialog:
            self.rgb_dialog = RGBDialog(self._send_command, self)
        self.rgb_dialog.update_data(self.status_data)
        self.rgb_dialog.show()
        self.rgb_dialog.raise_()
        self.rgb_dialog.activateWindow()

    def closeEvent(self, event):
        self._poll_timer.stop()
        super().closeEvent(event)


# ─── Stylesheet ───

_STYLESHEET = """
QWidget#MainWindow, QDialog { background-color: #0C0C0E; }
QLabel {
    color: #E2E2E9;
    font-family: "Outfit", "Inter", "Segoe UI", sans-serif;
    padding: 2px 0px;
}
QGroupBox {
    border: 1px solid #22222B;
    border-radius: 12px;
    background-color: #131318;
    padding: 12px;
}
QGroupBox::title {
    subcontrol-origin: border;
    subcontrol-position: top center;
    padding: 0 10px;
    color: #FF3E6C;
    font-weight: 800;
    font-size: 11pt;
    letter-spacing: 1px;
    top: -10px;
    background-color: #0C0C0E;
}
QPushButton {
    background-color: #1C1C24;
    border: 1px solid #2C2C39;
    border-radius: 8px;
    color: #E2E2E9;
    padding: 8px 16px;
    font-size: 9pt;
    font-weight: bold;
}
QPushButton#RGBButton {
    background-color: #1C1C24;
    border: 1px solid #2C2C39;
    border-radius: 6px;
    color: #E2E2E9;
    padding: 5px 12px;
    font-size: 8.5pt;
    font-weight: bold;
}
QPushButton#RGBButton:hover {
    background-color: #252530;
    border-color: #FF3E6C;
    color: #FFFFFF;
}
QPushButton:hover {
    background-color: #252530;
    border-color: #FF3E6C;
    color: #FFFFFF;
}
QPushButton:checked {
    background-color: qlineargradient(x1:0, y1:0, x2:1, y2:0, stop:0 #FF3E6C, stop:1 #E60045);
    border: none;
    color: #FFFFFF;
}
QSlider::groove:horizontal {
    height: 6px;
    background: #1C1C24;
    border-radius: 3px;
}
QSlider::sub-page:horizontal {
    background: qlineargradient(x1:0, y1:0, x2:1, y2:0, stop:0 #FF3E6C, stop:1 #FF6B8B);
    border-radius: 3px;
}
QSlider::handle:horizontal {
    background: #ECECF1;
    width: 14px;
    margin-top: -4px;
    margin-bottom: -4px;
    border-radius: 7px;
    border: 1px solid #2C2C39;
}
QSlider::handle:horizontal:hover {
    background: #FF3E6C;
    border-color: #FF6B8B;
}
QComboBox {
    background-color: #1C1C24;
    border: 1px solid #2C2C39;
    border-radius: 6px;
    color: #E2E2E9;
    padding: 3px 6px;
    font-size: 8.5pt;
    font-weight: bold;
    min-width: 70px;
}
QComboBox::drop-down {
    border: none;
}
QComboBox QAbstractItemView {
    background-color: #131318;
    border: 1px solid #22222B;
    color: #E2E2E9;
    selection-background-color: #FF3E6C;
    selection-color: #FFFFFF;
}

"""


# ─── Entry Point ───


def main():
    app = QtWidgets.QApplication(sys.argv)
    app.setDesktopFileName("openitro.desktop")

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
