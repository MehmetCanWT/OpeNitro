//! Custom fan curve evaluation with piecewise-linear interpolation and hysteresis.

use serde::{Deserialize, Serialize};

/// A single `(temperature °C, fan speed %)` control point on a fan curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temp_c: u8,
    pub speed_pct: u8,
}

impl CurvePoint {
    pub fn new(temp_c: u8, speed_pct: u8) -> Self {
        Self {
            temp_c: temp_c.clamp(30, 100),
            speed_pct: speed_pct.min(100),
        }
    }
}

/// Built-in fan curve presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurvePreset {
    Silent,
    Balanced,
    Gaming,
    Aggressive,
}

impl CurvePreset {
    pub fn label(self) -> &'static str {
        match self {
            CurvePreset::Silent => "Silent",
            CurvePreset::Balanced => "Balanced",
            CurvePreset::Gaming => "Gaming",
            CurvePreset::Aggressive => "Aggressive",
        }
    }

    pub fn points(self) -> Vec<CurvePoint> {
        match self {
            CurvePreset::Silent => vec![
                CurvePoint::new(40, 0),
                CurvePoint::new(55, 22),
                CurvePoint::new(70, 42),
                CurvePoint::new(82, 68),
                CurvePoint::new(92, 95),
            ],
            CurvePreset::Balanced => vec![
                CurvePoint::new(40, 20),
                CurvePoint::new(55, 35),
                CurvePoint::new(70, 58),
                CurvePoint::new(82, 82),
                CurvePoint::new(92, 100),
            ],
            CurvePreset::Gaming => vec![
                CurvePoint::new(38, 30),
                CurvePoint::new(52, 50),
                CurvePoint::new(66, 72),
                CurvePoint::new(78, 90),
                CurvePoint::new(88, 100),
            ],
            CurvePreset::Aggressive => vec![
                CurvePoint::new(35, 45),
                CurvePoint::new(48, 65),
                CurvePoint::new(62, 85),
                CurvePoint::new(74, 100),
                CurvePoint::new(85, 100),
            ],
        }
    }
}

/// Configurable fan curve with sorted control points and hysteresis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanCurve {
    pub points: Vec<CurvePoint>,
    pub hysteresis_c: u8,
}

impl Default for FanCurve {
    fn default() -> Self {
        Self {
            points: CurvePreset::Balanced.points(),
            hysteresis_c: 2,
        }
    }
}

impl FanCurve {
    /// Normalize points: clamp bounds, sort ascending by temperature, deduplicate temps.
    pub fn normalize(&mut self) {
        for p in &mut self.points {
            p.temp_c = p.temp_c.clamp(30, 100);
            p.speed_pct = p.speed_pct.min(100);
        }
        self.points.sort_by_key(|p| p.temp_c);
        self.points.dedup_by_key(|p| p.temp_c);
        if self.points.len() < 2 {
            self.points = CurvePreset::Balanced.points();
        }
        self.hysteresis_c = self.hysteresis_c.min(10);
    }

    /// Evaluate target fan speed `%` (0..=100) at a given temperature using linear interpolation.
    pub fn evaluate(&self, temp_c: u8) -> u8 {
        if self.points.is_empty() {
            return 50;
        }
        if self.points.len() == 1 {
            return self.points[0].speed_pct.min(100);
        }

        let mut sorted = self.points.clone();
        sorted.sort_by_key(|p| p.temp_c);

        if temp_c <= sorted[0].temp_c {
            return sorted[0].speed_pct.min(100);
        }
        let last = sorted[sorted.len() - 1];
        if temp_c >= last.temp_c {
            return last.speed_pct.min(100);
        }

        for pair in sorted.windows(2) {
            let p0 = pair[0];
            let p1 = pair[1];
            if temp_c >= p0.temp_c && temp_c <= p1.temp_c {
                let dt = (p1.temp_c - p0.temp_c) as f32;
                if dt <= f32::EPSILON {
                    return p1.speed_pct.min(100);
                }
                let t = (temp_c - p0.temp_c) as f32 / dt;
                let speed = p0.speed_pct as f32 + t * (p1.speed_pct as f32 - p0.speed_pct as f32);
                return speed.round().clamp(0.0, 100.0) as u8;
            }
        }

        last.speed_pct.min(100)
    }

    /// Evaluate target fan speed while respecting hysteresis when temperature is dropping.
    pub fn evaluate_with_hysteresis(&self, temp_c: u8, tracker: &mut HysteresisTracker) -> u8 {
        let effective_temp = tracker.update(temp_c, self.hysteresis_c);
        self.evaluate(effective_temp)
    }
}

/// Tracks peak-held temperature to prevent fan oscillation on small thermal dips.
#[derive(Debug, Clone, Default)]
pub struct HysteresisTracker {
    effective_temp: Option<u8>,
}

impl HysteresisTracker {
    pub fn update(&mut self, current_temp: u8, hysteresis_c: u8) -> u8 {
        let next = match self.effective_temp {
            None => current_temp,
            Some(prev) => {
                if current_temp >= prev || prev.saturating_sub(current_temp) >= hysteresis_c {
                    current_temp
                } else {
                    prev
                }
            }
        };
        self.effective_temp = Some(next);
        next
    }

    pub fn reset(&mut self) {
        self.effective_temp = None;
    }
}
