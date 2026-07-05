#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
openitro-gui — Modern Card-Based Linux System Controller for Acer Nitro & Predator Laptops
Communicates with openitrod daemon via UNIX socket.
"""

import os
import sys
import json
import math
import time
import socket
from typing import Optional, Dict, Any

from PyQt6 import QtCore, QtGui, QtWidgets

SOCKET_PATH = "/var/run/openitro.sock"


# ─── STYLESHEET ───
_STYLESHEET = """
QMainWindow, QDialog {
    background-color: #0C0C10;
    color: #FFFFFF;
}
QWidget {
    font-family: "Inter", "Segoe UI", "Roboto", sans-serif;
}
QLabel {
    color: #E0E0E8;
}
QPushButton {
    background-color: #1F1F28;
    border: 1px solid #333342;
    border-radius: 8px;
    padding: 8px 16px;
    color: #FFFFFF;
    font-weight: 600;
    font-size: 10pt;
}
QPushButton:hover {
    background-color: #282834;
    border-color: #FF3E6C;
}
QPushButton:checked {
    background-color: #FF3E6C;
    border-color: #FF3E6C;
    color: #FFFFFF;
}
QPushButton:pressed {
    background-color: #D62852;
}
QPushButton:disabled {
    background-color: #15151A;
    border-color: #222228;
    color: #555560;
}
QSlider::groove:horizontal {
    border: none;
    height: 6px;
    background: #242430;
    border-radius: 3px;
}
QSlider::sub-page:horizontal {
    background: #FF3E6C;
    border-radius: 3px;
}
QSlider::handle:horizontal {
    background: #FFFFFF;
    border: 2px solid #FF3E6C;
    width: 16px;
    margin: -6px 0;
    border-radius: 8px;
}
QSlider::handle:horizontal:hover {
    background: #FF3E6C;
}
QSlider::handle:horizontal:disabled {
    background: #444450;
    border-color: #444450;
}
QComboBox {
    background-color: #1F1F28;
    border: 1px solid #333342;
    border-radius: 8px;
    padding: 6px 12px;
    color: #FFFFFF;
    font-weight: 600;
}
QComboBox:hover {
    border-color: #FF3E6C;
}
QComboBox::drop-down {
    subcontrol-origin: padding;
    subcontrol-position: top right;
    width: 24px;
    border-left: none;
}
QComboBox QAbstractItemView {
    background-color: #181820;
    border: 1px solid #333342;
    selection-background-color: #FF3E6C;
    color: #FFFFFF;
}
"""


# ─── ASYNC BACKGROUND POLLER ───

class StatusThread(QtCore.QThread):
    """Background thread to poll daemon socket asynchronously without blocking UI."""
    statusReceived = QtCore.pyqtSignal(dict)

    def __init__(self, sock_path: str, parent=None):
        super().__init__(parent)
        self.sock_path = sock_path
        self.running = True

    def run(self):
        while self.running:
            try:
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                    client.settimeout(1.5)
                    client.connect(self.sock_path)
                    client.sendall(b"GET_STATUS")
                    resp = client.recv(4096).decode("utf-8")
                    res = json.loads(resp)
                    if res and res.get("status") == "success":
                        self.statusReceived.emit(res["data"])
            except Exception:
                pass
            self.msleep(1000)


# ─── CUSTOM WIDGETS ───

class ClickSlider(QtWidgets.QSlider):
    """QSlider that jumps directly to the clicked position and supports smooth dragging."""

    def mousePressEvent(self, event: QtGui.QMouseEvent):
        if event.button() == QtCore.Qt.MouseButton.LeftButton:
            val = QtWidgets.QStyle.sliderValueFromPosition(
                self.minimum(),
                self.maximum(),
                int(event.position().x()),
                self.width(),
            )
            self.setValue(val)
            self.sliderMoved.emit(val)
            self.sliderReleased.emit()
            event.accept()
        else:
            super().mousePressEvent(event)


class ToggleSwitch(QtWidgets.QAbstractButton):
    """Modern animated pill toggle switch."""

    def __init__(self, parent: Optional[QtWidgets.QWidget] = None):
        super().__init__(parent)
        self.setCheckable(True)
        self.setFixedSize(50, 26)
        self._thumb_pos = 3.0
        self._anim = QtCore.QPropertyAnimation(self, b"thumb_pos", self)
        self._anim.setDuration(180)
        self._anim.setEasingCurve(QtCore.QEasingCurve.Type.InOutQuad)
        self.toggled.connect(self._on_toggled)

    def _get_thumb_pos(self) -> float:
        return self._thumb_pos

    def _set_thumb_pos(self, pos: float):
        self._thumb_pos = pos
        self.update()

    thumb_pos = QtCore.pyqtProperty(float, _get_thumb_pos, _set_thumb_pos)

    def _on_toggled(self, checked: bool):
        self._anim.stop()
        self._anim.setEndValue(25.0 if checked else 3.0)
        self._anim.start()

    def paintEvent(self, event: QtGui.QPaintEvent):
        p = QtGui.QPainter(self)
        p.setRenderHint(QtGui.QPainter.RenderHint.Antialiasing)

        # Track background
        bg_color = QtGui.QColor("#FF3E6C") if self.isChecked() else QtGui.QColor("#2A2A35")
        if not self.isEnabled():
            bg_color = QtGui.QColor("#1A1A22")

        p.setBrush(QtGui.QBrush(bg_color))
        p.setPen(QtCore.Qt.PenStyle.NoPen)
        p.drawRoundedRect(0, 0, self.width(), self.height(), 13, 13)

        # Thumb
        thumb_color = QtGui.QColor("#FFFFFF") if self.isEnabled() else QtGui.QColor("#555560")
        p.setBrush(QtGui.QBrush(thumb_color))
        p.drawEllipse(QtCore.QRectF(self._thumb_pos, 3.0, 20.0, 20.0))
        p.end()


class FanWidget(QtWidgets.QWidget):
    """Animated spinning fan blade widget matching live RPM with precomputed path."""

    def __init__(self, parent: Optional[QtWidgets.QWidget] = None):
        super().__init__(parent)
        self.setFixedSize(80, 80)
        self.angle = 0.0
        self.rpm = 0

        # Precompute blade path once in init to eliminate CPU lag and garbage collection
        radius = 34.0  # min(80, 80) / 2 - 6
        self.blade_path = QtGui.QPainterPath()
        self.blade_path.moveTo(0, 0)
        self.blade_path.cubicTo(radius * 0.3, -radius * 0.3, radius * 0.8, -radius * 0.5, radius * 0.85, 0)
        self.blade_path.cubicTo(radius * 0.6, radius * 0.3, radius * 0.2, radius * 0.2, 0, 0)

        self._timer = QtCore.QTimer(self)
        self._timer.timeout.connect(self._tick)
        self._timer.start(33)  # ~30 FPS for buttery smooth rendering

    def set_rpm(self, rpm: int):
        self.rpm = rpm

    def _tick(self):
        if self.rpm > 0:
            speed = max(1.0, (self.rpm / 5000.0) * 14.0)
            self.angle = (self.angle + speed) % 360.0
            self.update()

    def paintEvent(self, event: QtGui.QPaintEvent):
        p = QtGui.QPainter(self)
        p.setRenderHint(QtGui.QPainter.RenderHint.Antialiasing)
        rect = self.rect()
        center = QtCore.QPointF(rect.center())
        radius = min(rect.width(), rect.height()) / 2.0 - 6.0

        # Outer casing ring
        p.setPen(QtGui.QPen(QtGui.QColor("#242430"), 3))
        p.setBrush(QtGui.QBrush(QtGui.QColor("#14141C")))
        p.drawEllipse(center, radius, radius)

        # Inner hub
        p.setPen(QtCore.Qt.PenStyle.NoPen)
        p.setBrush(QtGui.QBrush(QtGui.QColor("#FF3E6C") if self.rpm > 0 else QtGui.QColor("#333342")))
        p.drawEllipse(center, radius * 0.28, radius * 0.28)

        # Fan blades
        p.save()
        p.translate(center)
        p.rotate(self.angle)

        blade_color = QtGui.QColor("#FF527B") if self.rpm > 0 else QtGui.QColor("#444450")
        p.setBrush(QtGui.QBrush(blade_color))

        num_blades = 7
        for _ in range(num_blades):
            p.drawPath(self.blade_path)
            p.rotate(360.0 / num_blades)

        p.restore()
        p.end()


class CardFrame(QtWidgets.QFrame):
    """
    Modern Card Container widget replacing QGroupBox.
    Prevents title clipping and layout overlap by placing the header cleanly inside a vertical layout.
    """

    def __init__(self, title: str = "", parent: Optional[QtWidgets.QWidget] = None):
        super().__init__(parent)
        self.setObjectName("CardFrame")
        self.setStyleSheet("""
            QFrame#CardFrame {
                background-color: #14141A;
                border: 1px solid #242430;
                border-radius: 14px;
            }
        """)

        self.main_layout = QtWidgets.QVBoxLayout(self)
        self.main_layout.setContentsMargins(20, 20, 20, 20)
        self.main_layout.setSpacing(14)

        if title:
            self.title_lbl = QtWidgets.QLabel(title.upper())
            self.title_lbl.setStyleSheet("""
                color: #FF3E6C;
                font-weight: 800;
                font-size: 11pt;
                letter-spacing: 1.5px;
                border: none;
            """)
            self.main_layout.addWidget(self.title_lbl)

            # Subtle separator line
            line = QtWidgets.QFrame()
            line.setFrameShape(QtWidgets.QFrame.Shape.HLine)
            line.setFrameShadow(QtWidgets.QFrame.Shadow.Plain)
            line.setStyleSheet("background-color: #242430; border: none; max-height: 1px;")
            self.main_layout.addWidget(line)

        self.content_layout = QtWidgets.QVBoxLayout()
        self.content_layout.setSpacing(12)
        self.main_layout.addLayout(self.content_layout)


# ─── STANDALONE RGB DIALOG ───

class RGBDialog(QtWidgets.QDialog):
    """Dedicated floating dialog window for RGB Keyboard control."""

    def __init__(self, send_cmd_callback, parent: Optional[QtWidgets.QWidget] = None):
        super().__init__(parent)
        self.send_command = send_cmd_callback
        self.setWindowTitle("⌨ Acer RGB Keyboard Controller")
        self.resize(520, 640)
        self.setMinimumSize(480, 580)

        layout = QtWidgets.QVBoxLayout(self)
        layout.setContentsMargins(20, 20, 20, 20)
        layout.setSpacing(16)

        # ─── 1. Mode & Zone Card ───
        mode_card = CardFrame("Lighting Mode & Zones")
        form_layout = QtWidgets.QGridLayout()
        form_layout.setSpacing(12)

        form_layout.addWidget(QtWidgets.QLabel("Mode:"), 0, 0)
        self.mode_combo = QtWidgets.QComboBox()
        self.mode_combo.addItems(["Static", "Breath", "Neon", "Wave", "Shifting", "Zoom"])
        self.mode_combo.currentIndexChanged.connect(self._on_mode_changed)
        form_layout.addWidget(self.mode_combo, 0, 1)

        form_layout.addWidget(QtWidgets.QLabel("Target Zone:"), 1, 0)
        self.zone_combo = QtWidgets.QComboBox()
        self.zone_combo.addItems(["All Zones", "Zone 1 (Left)", "Zone 2 (Mid-Left)", "Zone 3 (Mid-Right)", "Zone 4 (Right)"])
        self.zone_combo.currentIndexChanged.connect(self._on_zone_changed)
        form_layout.addWidget(self.zone_combo, 1, 1)

        mode_card.content_layout.addLayout(form_layout)
        layout.addWidget(mode_card)

        # ─── 2. Speed & Direction Card ───
        self.speed_card = CardFrame("Animation Speed & Direction")
        speed_layout = QtWidgets.QGridLayout()
        speed_layout.setSpacing(12)

        speed_layout.addWidget(QtWidgets.QLabel("Speed:"), 0, 0)
        self.speed_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self.speed_slider.setRange(0, 9)
        self.speed_slider.setValue(4)
        self.speed_lbl = QtWidgets.QLabel("4")
        self.speed_slider.valueChanged.connect(lambda v: self.speed_lbl.setText(str(v)))
        self.speed_slider.sliderReleased.connect(self._on_speed_changed)
        speed_layout.addWidget(self.speed_slider, 0, 1)
        speed_layout.addWidget(self.speed_lbl, 0, 2)

        self.dir_lbl = QtWidgets.QLabel("Direction:")
        speed_layout.addWidget(self.dir_lbl, 1, 0)
        self.dir_combo = QtWidgets.QComboBox()
        self.dir_combo.addItems(["Right to Left", "Left to Right"])
        self.dir_combo.currentIndexChanged.connect(self._on_dir_changed)
        speed_layout.addWidget(self.dir_combo, 1, 1, 1, 2)

        self.speed_card.content_layout.addLayout(speed_layout)
        layout.addWidget(self.speed_card)

        # ─── 3. Brightness & Color Card ───
        color_card = CardFrame("Brightness & Color Customizer")
        color_layout = QtWidgets.QGridLayout()
        color_layout.setSpacing(12)

        # Brightness
        color_layout.addWidget(QtWidgets.QLabel("Brightness:"), 0, 0)
        self.bright_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self.bright_slider.setRange(0, 100)
        self.bright_slider.setValue(100)
        self.bright_lbl = QtWidgets.QLabel("100%")
        self.bright_slider.valueChanged.connect(lambda v: self.bright_lbl.setText(f"{v}%"))
        self.bright_slider.sliderReleased.connect(self._on_bright_changed)
        color_layout.addWidget(self.bright_slider, 0, 1)
        color_layout.addWidget(self.bright_lbl, 0, 2)

        # Color preview box
        self.preview_box = QtWidgets.QFrame()
        self.preview_box.setFixedSize(50, 30)
        self.preview_box.setStyleSheet("background-color: rgb(255, 62, 108); border-radius: 6px; border: 1px solid #FFFFFF;")
        color_layout.addWidget(QtWidgets.QLabel("Preview:"), 1, 0)
        color_layout.addWidget(self.preview_box, 1, 1)

        # Red
        color_layout.addWidget(QtWidgets.QLabel("Red:"), 2, 0)
        self.red_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self.red_slider.setRange(0, 255)
        self.red_slider.setValue(255)
        self.red_lbl = QtWidgets.QLabel("255")
        self.red_slider.valueChanged.connect(self._on_color_slider_moved)
        self.red_slider.sliderReleased.connect(self._on_color_changed)
        color_layout.addWidget(self.red_slider, 2, 1)
        color_layout.addWidget(self.red_lbl, 2, 2)

        # Green
        color_layout.addWidget(QtWidgets.QLabel("Green:"), 3, 0)
        self.green_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self.green_slider.setRange(0, 255)
        self.green_slider.setValue(62)
        self.green_lbl = QtWidgets.QLabel("62")
        self.green_slider.valueChanged.connect(self._on_color_slider_moved)
        self.green_slider.sliderReleased.connect(self._on_color_changed)
        color_layout.addWidget(self.green_slider, 3, 1)
        color_layout.addWidget(self.green_lbl, 3, 2)

        # Blue
        color_layout.addWidget(QtWidgets.QLabel("Blue:"), 4, 0)
        self.blue_slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        self.blue_slider.setRange(0, 255)
        self.blue_slider.setValue(108)
        self.blue_lbl = QtWidgets.QLabel("108")
        self.blue_slider.valueChanged.connect(self._on_color_slider_moved)
        self.blue_slider.sliderReleased.connect(self._on_color_changed)
        color_layout.addWidget(self.blue_slider, 4, 1)
        color_layout.addWidget(self.blue_lbl, 4, 2)

        color_card.content_layout.addLayout(color_layout)
        layout.addWidget(color_card)

        layout.addStretch()

        # Close button
        btn_layout = QtWidgets.QHBoxLayout()
        btn_layout.addStretch()
        close_btn = QtWidgets.QPushButton("Close")
        close_btn.setMinimumWidth(100)
        close_btn.clicked.connect(self.accept)
        btn_layout.addWidget(close_btn)
        layout.addLayout(btn_layout)

        self._update_visibility()

    def _update_visibility(self):
        mode_idx = self.mode_combo.currentIndex()
        is_static = (mode_idx == 0)
        is_wave = (mode_idx == 3)
        is_dynamic = (mode_idx > 0)

        self.zone_combo.setEnabled(is_static)
        self.red_slider.setEnabled(is_static or mode_idx in (1, 5))
        self.green_slider.setEnabled(is_static or mode_idx in (1, 5))
        self.blue_slider.setEnabled(is_static or mode_idx in (1, 5))

        self.speed_slider.setEnabled(is_dynamic)
        self.dir_combo.setEnabled(is_wave)
        self.dir_combo.setVisible(is_wave)
        self.dir_lbl.setVisible(is_wave)

    def _on_mode_changed(self, idx: int):
        self._update_visibility()
        self.send_command(f"SET_RGB_MODE {idx}")

    def _on_zone_changed(self, idx: int):
        zone_val = 0 if idx == 0 else (1 << (idx - 1))
        self.send_command(f"SET_RGB_ZONE {zone_val}")

    def _on_speed_changed(self):
        self.send_command(f"SET_RGB_SPEED {self.speed_slider.value()}")

    def _on_dir_changed(self, idx: int):
        dir_val = 1 if idx == 0 else 2
        self.send_command(f"SET_RGB_DIRECTION {dir_val}")

    def _on_bright_changed(self):
        self.send_command(f"SET_RGB_BRIGHTNESS {self.bright_slider.value()}")

    def _on_color_slider_moved(self):
        r, g, b = self.red_slider.value(), self.green_slider.value(), self.blue_slider.value()
        self.red_lbl.setText(str(r))
        self.green_lbl.setText(str(g))
        self.blue_lbl.setText(str(b))
        self.preview_box.setStyleSheet(f"background-color: rgb({r}, {g}, {b}); border-radius: 6px; border: 1px solid #FFFFFF;")

    def _on_color_changed(self):
        r, g, b = self.red_slider.value(), self.green_slider.value(), self.blue_slider.value()
        self.send_command(f"SET_RGB_COLOR {r} {g} {b}")

    def update_from_status(self, data: Dict[str, Any]):
        """Update controls if dialog is open without triggering signals."""
        self.blockSignals(True)
        try:
            mode = data.get("rgb_mode", 0)
            if 0 <= mode < self.mode_combo.count():
                self.mode_combo.setCurrentIndex(mode)

            r = data.get("rgb_red", 255)
            g = data.get("rgb_green", 62)
            b = data.get("rgb_blue", 108)
            self.red_slider.setValue(r)
            self.green_slider.setValue(g)
            self.blue_slider.setValue(b)
            self._on_color_slider_moved()

            bright = data.get("rgb_brightness", 100)
            self.bright_slider.setValue(bright)
            self.bright_lbl.setText(f"{bright}%")

            speed = data.get("rgb_speed", 4)
            self.speed_slider.setValue(speed)
            self.speed_lbl.setText(str(speed))

            direction = data.get("rgb_direction", 1)
            self.dir_combo.setCurrentIndex(0 if direction == 1 else 1)

            zone = data.get("rgb_zone", 0)
            if zone == 0:
                self.zone_combo.setCurrentIndex(0)
            elif zone == 1:
                self.zone_combo.setCurrentIndex(1)
            elif zone == 2:
                self.zone_combo.setCurrentIndex(2)
            elif zone == 4:
                self.zone_combo.setCurrentIndex(3)
            elif zone == 8:
                self.zone_combo.setCurrentIndex(4)

            self._update_visibility()
        finally:
            self.blockSignals(False)


# ─── MAIN WINDOW ───

class OpeNitroWindow(QtWidgets.QMainWindow):
    """Main application window using responsive Card-based layout."""

    def __init__(self):
        super().__init__()
        self.setWindowTitle("OpeNitro — Linux System Controller")
        self.resize(980, 680)
        self.setMinimumSize(880, 620)
        self.setStyleSheet(_STYLESHEET)

        self._sock_path = SOCKET_PATH
        self._last_slider_change: Dict[str, float] = {}
        self._last_mode_change: Dict[str, float] = {}

        # Min/Max temperature tracking
        self._cpu_min = 999
        self._cpu_max = 0
        self._gpu_min = 999
        self._gpu_max = 0
        self._sys_min = 999
        self._sys_max = 0

        self._rgb_dialog: Optional[RGBDialog] = None

        self._build_ui()

        # Start asynchronous background polling thread
        self._status_thread = StatusThread(self._sock_path, self)
        self._status_thread.statusReceived.connect(self._update_ui)
        self._status_thread.start()

    def _build_ui(self):
        central = QtWidgets.QWidget()
        self.setCentralWidget(central)
        main_layout = QtWidgets.QVBoxLayout(central)
        main_layout.setContentsMargins(24, 20, 24, 24)
        main_layout.setSpacing(20)

        # ─── Top Header Bar ───
        header_layout = QtWidgets.QHBoxLayout()
        header_layout.setSpacing(16)

        # Title & Subtitle
        title_box = QtWidgets.QVBoxLayout()
        title_box.setSpacing(2)
        logo_lbl = QtWidgets.QLabel("OPENITRO")
        logo_lbl.setStyleSheet("color: #FF3E6C; font-size: 20pt; font-weight: 900; letter-spacing: 2px;")
        sub_lbl = QtWidgets.QLabel("LINUX SYSTEM CONTROLLER")
        sub_lbl.setStyleSheet("color: #888898; font-size: 8pt; font-weight: 700; letter-spacing: 1px;")
        title_box.addWidget(logo_lbl)
        title_box.addWidget(sub_lbl)
        header_layout.addLayout(title_box)

        header_layout.addStretch()

        # Model & Serial Number Badge
        self.info_badge = QtWidgets.QLabel("Model: Detecting...  |  S/N: Detecting...")
        self.info_badge.setStyleSheet("""
            background-color: #14141A;
            border: 1px solid #242430;
            border-radius: 8px;
            padding: 8px 16px;
            color: #A0A0B0;
            font-weight: 600;
            font-size: 9.5pt;
        """)
        header_layout.addWidget(self.info_badge)

        # Power Source Badge
        self.power_badge = QtWidgets.QLabel("⚡ AC POWER")
        self.power_badge.setStyleSheet("""
            background-color: #1A1820;
            border: 1px solid #FF3E6C;
            border-radius: 8px;
            padding: 8px 14px;
            color: #FF3E6C;
            font-weight: 800;
            font-size: 9.5pt;
        """)
        header_layout.addWidget(self.power_badge)

        # RGB Keyboard Button
        self.btn_rgb = QtWidgets.QPushButton("⌨ RGB Keyboard")
        self.btn_rgb.setStyleSheet("""
            QPushButton {
                background-color: #181420;
                border: 1.5px solid #FF3E6C;
                border-radius: 8px;
                padding: 8px 18px;
                color: #FFFFFF;
                font-weight: 800;
            }
            QPushButton:hover {
                background-color: #FF3E6C;
            }
        """)
        self.btn_rgb.clicked.connect(self._open_rgb_dialog)
        self.btn_rgb.setVisible(False)  # Shown dynamically when supported
        header_layout.addWidget(self.btn_rgb)

        main_layout.addLayout(header_layout)

        # ─── Content Grid Layout ───
        grid = QtWidgets.QGridLayout()
        grid.setSpacing(20)
        grid.setColumnStretch(0, 1)
        grid.setColumnStretch(1, 1)
        grid.setRowStretch(0, 5)
        grid.setRowStretch(1, 4)

        # Row 0, Col 0: CPU Thermal & Fan
        self.cpu_card, self.cpu_temp_lbl, self.cpu_minmax_lbl, self.cpu_fan_w, self.cpu_rpm_lbl, \
            self.btn_cpu_auto, self.btn_cpu_max, self.btn_cpu_man, self.cpu_slider, self.cpu_slider_lbl = \
            self._build_fan_card("CPU Thermal & Fan Control", "cpu")
        grid.addWidget(self.cpu_card, 0, 0)

        # Row 0, Col 1: GPU Thermal & Fan
        self.gpu_card, self.gpu_temp_lbl, self.gpu_minmax_lbl, self.gpu_fan_w, self.gpu_rpm_lbl, \
            self.btn_gpu_auto, self.btn_gpu_max, self.btn_gpu_man, self.gpu_slider, self.gpu_slider_lbl = \
            self._build_fan_card("GPU Thermal & Fan Control", "gpu")
        grid.addWidget(self.gpu_card, 0, 1)

        # Row 1, Col 0: Performance Modes
        grid.addWidget(self._build_profiles_card(), 1, 0)

        # Row 1, Col 1: System Health & Settings
        grid.addWidget(self._build_settings_card(), 1, 1)

        main_layout.addLayout(grid)

    def _build_fan_card(self, title: str, unit: str):
        card = CardFrame(title)

        # Top row: Temp Gauge & Min/Max Badge + Spinning Fan Widget
        top_box = QtWidgets.QHBoxLayout()
        top_box.setSpacing(16)

        temp_col = QtWidgets.QVBoxLayout()
        temp_col.setSpacing(4)
        temp_lbl = QtWidgets.QLabel("--°C")
        temp_lbl.setStyleSheet("font-size: 32pt; font-weight: 900; color: #FFFFFF;")
        
        minmax_lbl = QtWidgets.QLabel("MIN: --°C  |  MAX: --°C")
        minmax_lbl.setStyleSheet("font-size: 9.5pt; font-weight: 700; color: #00E5FF; background-color: #1A1E26; padding: 4px 10px; border-radius: 6px;")
        
        temp_col.addWidget(temp_lbl)
        temp_col.addWidget(minmax_lbl)
        temp_col.addStretch()
        top_box.addLayout(temp_col)

        top_box.addStretch()

        fan_col = QtWidgets.QVBoxLayout()
        fan_col.setSpacing(6)
        fan_col.setAlignment(QtCore.Qt.AlignmentFlag.AlignCenter)
        fan_w = FanWidget()
        rpm_lbl = QtWidgets.QLabel("-- RPM")
        rpm_lbl.setStyleSheet("font-size: 11pt; font-weight: 800; color: #FF3E6C;")
        rpm_lbl.setAlignment(QtCore.Qt.AlignmentFlag.AlignCenter)
        fan_col.addWidget(fan_w)
        fan_col.addWidget(rpm_lbl)
        top_box.addLayout(fan_col)

        card.content_layout.addLayout(top_box)

        # Mode Buttons (Mutually Exclusive Group)
        btn_box = QtWidgets.QHBoxLayout()
        btn_box.setSpacing(10)
        btn_auto = QtWidgets.QPushButton("AUTO")
        btn_max = QtWidgets.QPushButton("MAX")
        btn_man = QtWidgets.QPushButton("MANUAL")

        btn_group = QtWidgets.QButtonGroup(self)
        btn_group.setExclusive(True)

        for btn in (btn_auto, btn_max, btn_man):
            btn.setCheckable(True)
            btn.setMinimumHeight(38)
            btn_group.addButton(btn)
            btn_box.addWidget(btn)

        btn_auto.clicked.connect(lambda: self._set_fan_mode(unit, "auto"))
        btn_max.clicked.connect(lambda: self._set_fan_mode(unit, "turbo"))
        btn_man.clicked.connect(lambda: self._set_fan_mode(unit, "manual"))

        card.content_layout.addLayout(btn_box)

        # Manual Slider Box
        slider_box = QtWidgets.QHBoxLayout()
        slider_box.setSpacing(12)
        slider = ClickSlider(QtCore.Qt.Orientation.Horizontal)
        slider.setRange(0, 100)
        slider.setValue(100)
        slider.setEnabled(False)

        slider_lbl = QtWidgets.QLabel("100%")
        slider_lbl.setMinimumWidth(45)
        slider_lbl.setStyleSheet("font-weight: 800; color: #FF3E6C;")

        slider.sliderPressed.connect(lambda u=unit: self._register_slider_interaction(u))
        slider.sliderMoved.connect(lambda v, u=unit: self._register_slider_interaction(u))
        slider.sliderReleased.connect(lambda u=unit: self._set_fan_speed_manual(u))
        slider.valueChanged.connect(lambda v, lbl=slider_lbl: lbl.setText(f"{v}%"))

        slider_box.addWidget(QtWidgets.QLabel("Speed:"))
        slider_box.addWidget(slider)
        slider_box.addWidget(slider_lbl)

        card.content_layout.addLayout(slider_box)
        return card, temp_lbl, minmax_lbl, fan_w, rpm_lbl, btn_auto, btn_max, btn_man, slider, slider_lbl

    def _build_profiles_card(self):
        card = CardFrame("Performance Mode Profiles")
        layout = QtWidgets.QVBoxLayout()
        layout.setSpacing(12)

        self.btn_quiet = QtWidgets.QPushButton("🍃  QUIET MODE")
        self.btn_default = QtWidgets.QPushButton("⚖️  DEFAULT (BALANCED)")
        self.btn_extreme = QtWidgets.QPushButton("🚀  EXTREME PERFORMANCE")

        self.profile_group = QtWidgets.QButtonGroup(self)
        self.profile_group.setExclusive(True)

        for btn in (self.btn_quiet, self.btn_default, self.btn_extreme):
            btn.setCheckable(True)
            btn.setMinimumHeight(44)
            btn.setStyleSheet("""
                QPushButton {
                    font-size: 11pt;
                    font-weight: 800;
                    text-align: left;
                    padding-left: 20px;
                }
                QPushButton:checked {
                    background-color: #FF3E6C;
                    color: #FFFFFF;
                }
            """)
            self.profile_group.addButton(btn)
            layout.addWidget(btn)

        self.btn_quiet.clicked.connect(lambda: self._set_power_mode("quiet"))
        self.btn_default.clicked.connect(lambda: self._set_power_mode("default"))
        self.btn_extreme.clicked.connect(lambda: self._set_power_mode("extreme"))

        card.content_layout.addLayout(layout)
        card.content_layout.addStretch()
        return card

    def _build_settings_card(self):
        card = CardFrame("System Settings & Health")
        layout = QtWidgets.QGridLayout()
        layout.setSpacing(14)

        # 1. Battery Charge Limit (80%)
        layout.addWidget(QtWidgets.QLabel("🔋 80% Battery Charge Limit (Protection):"), 0, 0)
        self.sw_battery = ToggleSwitch()
        self.sw_battery.toggled.connect(lambda checked: self._send_command(f"SET_BATTERY_LIMIT {'on' if checked else 'off'}"))
        layout.addWidget(self.sw_battery, 0, 1, QtCore.Qt.AlignmentFlag.AlignRight)

        # 2. Acer CoolBoost
        layout.addWidget(QtWidgets.QLabel("❄️ Acer CoolBoost (High Performance Fan):"), 1, 0)
        self.sw_coolboost = ToggleSwitch()
        self.sw_coolboost.toggled.connect(lambda checked: self._send_command(f"SET_COOLBOOST {'on' if checked else 'off'}"))
        layout.addWidget(self.sw_coolboost, 1, 1, QtCore.Qt.AlignmentFlag.AlignRight)

        # 3. KB Backlight Timeout
        layout.addWidget(QtWidgets.QLabel("⌨️ Keyboard Backlight 30s Timeout:"), 2, 0)
        self.sw_kb_timeout = ToggleSwitch()
        self.sw_kb_timeout.toggled.connect(lambda checked: self._send_command(f"SET_KB_TIMEOUT {'on' if checked else 'off'}"))
        layout.addWidget(self.sw_kb_timeout, 2, 1, QtCore.Qt.AlignmentFlag.AlignRight)

        # 4. USB Suspend Charging
        layout.addWidget(QtWidgets.QLabel("🔌 USB Power-Off / Suspend Charging:"), 3, 0)
        self.sw_usb_charge = ToggleSwitch()
        self.sw_usb_charge.toggled.connect(lambda checked: self._send_command(f"SET_USB_CHARGE {'on' if checked else 'off'}"))
        layout.addWidget(self.sw_usb_charge, 3, 1, QtCore.Qt.AlignmentFlag.AlignRight)

        # Divider
        line = QtWidgets.QFrame()
        line.setFrameShape(QtWidgets.QFrame.Shape.HLine)
        line.setStyleSheet("background-color: #242430; border: none; max-height: 1px;")
        layout.addWidget(line, 4, 0, 1, 2)

        # System Temperature
        sys_box = QtWidgets.QHBoxLayout()
        sys_box.addWidget(QtWidgets.QLabel("System Motherboard Temp:"))
        sys_box.addStretch()
        self.sys_temp_lbl = QtWidgets.QLabel("--°C")
        self.sys_temp_lbl.setStyleSheet("font-size: 13pt; font-weight: 800; color: #FF3E6C;")
        sys_box.addWidget(self.sys_temp_lbl)
        layout.addLayout(sys_box, 5, 0, 1, 2)

        card.content_layout.addLayout(layout)
        return card

    # ─── SOCKET & COMMANDS ───

    def _send_command(self, cmd: str) -> Optional[Dict[str, Any]]:
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                client.settimeout(2.0)
                client.connect(self._sock_path)
                client.sendall(cmd.encode("utf-8"))
                resp = client.recv(4096).decode("utf-8")
                return json.loads(resp)
        except (OSError, socket.timeout, json.JSONDecodeError):
            return None

    def _register_slider_interaction(self, unit: str):
        self._last_slider_change[unit] = time.time()

    def _set_fan_mode(self, unit: str, mode: str):
        self._last_mode_change[unit] = time.time()
        slider = self.cpu_slider if unit == "cpu" else self.gpu_slider
        slider.setEnabled(mode == "manual")
        if mode == "manual":
            self._last_slider_change[unit] = time.time()
        self._send_command(f"SET_FAN_MODE {unit} {mode} {slider.value()}")

    def _set_fan_speed_manual(self, unit: str):
        slider = self.cpu_slider if unit == "cpu" else self.gpu_slider
        self._last_slider_change[unit] = time.time()
        self._send_command(f"SET_FAN_SPEED {unit} {slider.value()}")

    def _set_power_mode(self, mode: str):
        self._last_mode_change["power"] = time.time()
        self._send_command(f"SET_POWER_MODE {mode}")

    def _open_rgb_dialog(self):
        if not self._rgb_dialog:
            self._rgb_dialog = RGBDialog(self._send_command, self)
        self._rgb_dialog.show()
        self._rgb_dialog.raise_()
        self._rgb_dialog.activateWindow()

    def closeEvent(self, event: QtGui.QCloseEvent):
        if self._status_thread:
            self._status_thread.running = False
            self._status_thread.wait(1500)
        super().closeEvent(event)

    # ─── UI UPDATER ───

    def _update_ui(self, data: Dict[str, Any]):
        # 1. Header Info
        model = data.get("model", "Unknown")
        serial = data.get("product_serial", "Unknown")
        self.info_badge.setText(f"Model: {model}  |  S/N: {serial}")

        plugged = data.get("power_plugged", True)
        if plugged:
            self.power_badge.setText("⚡ AC POWER")
            self.power_badge.setStyleSheet("background-color: #1A1820; border: 1px solid #FF3E6C; border-radius: 8px; padding: 8px 14px; color: #FF3E6C; font-weight: 800; font-size: 9.5pt;")
        else:
            self.power_badge.setText("🔋 BATTERY")
            self.power_badge.setStyleSheet("background-color: #181A20; border: 1px solid #00E5FF; border-radius: 8px; padding: 8px 14px; color: #00E5FF; font-weight: 800; font-size: 9.5pt;")

        rgb_supported = data.get("rgb_supported", False)
        self.btn_rgb.setVisible(rgb_supported)
        if rgb_supported and self._rgb_dialog and self._rgb_dialog.isVisible():
            self._rgb_dialog.update_from_status(data)

        # 2. CPU Thermal & Fan
        cpu_temp = data.get("cpu_temp", 0)
        if cpu_temp > 0:
            self._cpu_min = min(self._cpu_min, cpu_temp)
            self._cpu_max = max(self._cpu_max, cpu_temp)
            self.cpu_temp_lbl.setText(f"{cpu_temp}°C")
            self.cpu_minmax_lbl.setText(f"MIN: {self._cpu_min}°C  |  MAX: {self._cpu_max}°C")

        cpu_rpm = data.get("cpu_rpm", 0)
        self.cpu_rpm_lbl.setText(f"{cpu_rpm} RPM")
        self.cpu_fan_w.set_rpm(cpu_rpm)

        cpu_mode = data.get("cpu_fan_mode", "auto")
        cpu_speed = data.get("cpu_manual_speed", 100)
        self._sync_fan_controls("cpu", cpu_mode, cpu_speed, self.btn_cpu_auto, self.btn_cpu_max, self.btn_cpu_man, self.cpu_slider)

        # 3. GPU Thermal & Fan
        gpu_temp = data.get("gpu_temp", 0)
        if gpu_temp > 0:
            self._gpu_min = min(self._gpu_min, gpu_temp)
            self._gpu_max = max(self._gpu_max, gpu_temp)
            self.gpu_temp_lbl.setText(f"{gpu_temp}°C")
            self.gpu_minmax_lbl.setText(f"MIN: {self._gpu_min}°C  |  MAX: {self._gpu_max}°C")

        gpu_rpm = data.get("gpu_rpm", 0)
        self.gpu_rpm_lbl.setText(f"{gpu_rpm} RPM")
        self.gpu_fan_w.set_rpm(gpu_rpm)

        gpu_mode = data.get("gpu_fan_mode", "auto")
        gpu_speed = data.get("gpu_manual_speed", 100)
        self._sync_fan_controls("gpu", gpu_mode, gpu_speed, self.btn_gpu_auto, self.btn_gpu_max, self.btn_gpu_man, self.gpu_slider)

        # 4. Performance Mode
        if time.time() - self._last_mode_change.get("power", 0.0) >= 1.5:
            nitro_mode = data.get("nitro_mode", "default")
            self.btn_quiet.setChecked(nitro_mode == "quiet")
            self.btn_default.setChecked(nitro_mode == "default")
            self.btn_extreme.setChecked(nitro_mode == "extreme")

        # 5. System Settings & Health
        sys_temp = data.get("sys_temp", 0)
        if sys_temp > 0:
            self._sys_min = min(self._sys_min, sys_temp)
            self._sys_max = max(self._sys_max, sys_temp)
            self.sys_temp_lbl.setText(f"{sys_temp}°C (Min: {self._sys_min}°C / Max: {self._sys_max}°C)")

        self._sync_switch(self.sw_battery, data.get("battery_limit_active", False))
        self._sync_switch(self.sw_coolboost, data.get("coolboost_active", False))
        self._sync_switch(self.sw_kb_timeout, data.get("kb_backlight_timeout", True))
        self._sync_switch(self.sw_usb_charge, data.get("usb_charge_poweroff", False))

    def _sync_fan_controls(self, unit: str, mode: str, speed: int, btn_auto, btn_max, btn_man, slider):
        if time.time() - self._last_mode_change.get(unit, 0.0) >= 1.5:
            btn_auto.setChecked(mode == "auto")
            btn_max.setChecked(mode == "turbo")
            btn_man.setChecked(mode == "manual")
            slider.setEnabled(mode == "manual")

        # Prevent slider knob jumping while user is actively dragging or just clicked
        if time.time() - self._last_slider_change.get(unit, 0.0) >= 2.5:
            if not slider.isSliderDown():
                slider.setValue(speed)

    def _sync_switch(self, switch: ToggleSwitch, target: bool):
        if switch.isChecked() != target:
            switch.blockSignals(True)
            switch.setChecked(target)
            switch._on_toggled(target)
            switch.blockSignals(False)


def main():
    app = QtWidgets.QApplication(sys.argv)
    app.setApplicationName("OpeNitro")
    app.setStyle("Fusion")

    win = OpeNitroWindow()
    win.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
