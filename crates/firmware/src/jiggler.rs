use common::{JiggleConfig, JiggleMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JiggleAction {
    pub dx: i8,
    pub dy: i8,
    pub buttons: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClickState {
    Idle,
    PressPending,
    ReleasePending,
}

pub struct JigglerEngine {
    config: JiggleConfig,
    prng_state: u32,
    angle_idx: u8,
    last_move_ms: u32,
    last_report_ms: u32,
    last_rth_ms: u32,
    total_jiggles: u32,
    cum_x: i32,
    cum_y: i32,
    pending_dx: i32,
    pending_dy: i32,
    nudge_return: Option<(i8, i8)>,
    click_after_move: bool,
    click_state: ClickState,
}

impl JigglerEngine {
    pub fn new(config: JiggleConfig) -> Self {
        Self {
            config,
            prng_state: 0x1234_5678,
            angle_idx: 0,
            last_move_ms: 0,
            last_report_ms: 0,
            last_rth_ms: 0,
            total_jiggles: 0,
            cum_x: 0,
            cum_y: 0,
            pending_dx: 0,
            pending_dy: 0,
            nudge_return: None,
            click_after_move: false,
            click_state: ClickState::Idle,
        }
    }

    pub fn config(&self) -> &JiggleConfig {
        &self.config
    }

    pub fn set_config(&mut self, mut config: JiggleConfig) {
        config.sanitize();
        if self.config.mode != config.mode
            || self.config.return_to_home != config.return_to_home
            || self.config.scale_px != config.scale_px
        {
            self.angle_idx = 0;
            self.cum_x = 0;
            self.cum_y = 0;
            self.pending_dx = 0;
            self.pending_dy = 0;
            self.nudge_return = None;
            self.click_after_move = false;
            self.click_state = ClickState::Idle;
        }
        self.config = config;
    }

    /// Resets the home reference point `(0, 0)` and timers when jiggling starts or resumes.
    pub fn on_jiggle_start(&mut self, current_time_ms: u32) {
        self.angle_idx = 0;
        self.cum_x = 0;
        self.cum_y = 0;
        self.pending_dx = 0;
        self.pending_dy = 0;
        self.nudge_return = None;
        self.click_after_move = false;
        self.click_state = ClickState::Idle;
        self.last_rth_ms = current_time_ms;
    }

    pub fn total_jiggles(&self) -> u32 {
        self.total_jiggles
    }

    fn next_prng(&mut self) -> u32 {
        let mut x = self.prng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.prng_state = x;
        x
    }

    pub fn update(&mut self, current_time_ms: u32, force_enabled: bool) -> Option<JiggleAction> {
        if !self.config.enabled && !force_enabled {
            self.nudge_return = None;
            self.pending_dx = 0;
            self.pending_dy = 0;
            self.click_after_move = false;
            self.click_state = ClickState::Idle;
            return None;
        }

        // 1. Drain any pending multi-packet movement (e.g. displacements > 120px or RTH return)
        if self.pending_dx != 0 || self.pending_dy != 0 {
            if current_time_ms.wrapping_sub(self.last_report_ms) >= 15 {
                let step_x = self.pending_dx.clamp(-120, 120) as i8;
                let step_y = self.pending_dy.clamp(-120, 120) as i8;
                self.pending_dx -= step_x as i32;
                self.pending_dy -= step_y as i32;
                self.last_report_ms = current_time_ms;

                if self.pending_dx == 0 && self.pending_dy == 0 && self.click_after_move {
                    self.click_after_move = false;
                    self.click_state = ClickState::PressPending;
                }

                return Some(JiggleAction {
                    dx: step_x,
                    dy: step_y,
                    buttons: 0,
                });
            }
            return None;
        }

        // 2. If a click at home was queued and no movement is pending, trigger it
        if self.click_after_move {
            self.click_after_move = false;
            self.click_state = ClickState::PressPending;
        }

        // 3. Process pending left mouse button press / release for Click Home
        match self.click_state {
            ClickState::PressPending => {
                if current_time_ms.wrapping_sub(self.last_report_ms) >= 15 {
                    self.click_state = ClickState::ReleasePending;
                    self.last_report_ms = current_time_ms;
                    return Some(JiggleAction {
                        dx: 0,
                        dy: 0,
                        buttons: 1,
                    });
                }
                return None;
            }
            ClickState::ReleasePending => {
                if current_time_ms.wrapping_sub(self.last_report_ms) >= 15 {
                    self.click_state = ClickState::Idle;
                    self.last_report_ms = current_time_ms;
                    return Some(JiggleAction {
                        dx: 0,
                        dy: 0,
                        buttons: 0,
                    });
                }
                return None;
            }
            ClickState::Idle => {}
        }

        // 4. For GentleNudge mode: wait at least 25ms before emitting the 1-2px return step
        if self.nudge_return.is_some() {
            if current_time_ms.wrapping_sub(self.last_move_ms) >= 25 {
                if let Some((rx, ry)) = self.nudge_return.take() {
                    self.last_report_ms = current_time_ms;
                    return Some(JiggleAction {
                        dx: -rx,
                        dy: -ry,
                        buttons: 0,
                    });
                }
            }
            return None;
        }

        // 5. Check Random mode Return-To-Home (RTH) period timer
        if self.config.mode == JiggleMode::Random && self.config.return_to_home {
            let rth_period_ms = self.config.rth_period_secs.max(1).saturating_mul(1000);
            if current_time_ms.wrapping_sub(self.last_rth_ms) >= rth_period_ms {
                self.last_rth_ms = current_time_ms;
                self.last_move_ms = current_time_ms;
                self.total_jiggles = self.total_jiggles.saturating_add(1);

                let ret_x = -self.cum_x;
                let ret_y = -self.cum_y;
                self.cum_x = 0;
                self.cum_y = 0;

                if self.config.click_home {
                    self.click_after_move = true;
                }

                if ret_x != 0 || ret_y != 0 {
                    let step_x = ret_x.clamp(-120, 120) as i8;
                    let step_y = ret_y.clamp(-120, 120) as i8;
                    self.pending_dx = ret_x - (step_x as i32);
                    self.pending_dy = ret_y - (step_y as i32);
                    self.last_report_ms = current_time_ms;

                    if self.pending_dx == 0 && self.pending_dy == 0 && self.click_after_move {
                        self.click_after_move = false;
                        self.click_state = ClickState::PressPending;
                    }

                    return Some(JiggleAction {
                        dx: step_x,
                        dy: step_y,
                        buttons: 0,
                    });
                } else if self.click_after_move {
                    self.click_after_move = false;
                    self.click_state = ClickState::ReleasePending;
                    self.last_report_ms = current_time_ms;
                    return Some(JiggleAction {
                        dx: 0,
                        dy: 0,
                        buttons: 1,
                    });
                }
            }
        }

        // 6. Regular movement interval check
        if current_time_ms.wrapping_sub(self.last_move_ms) < self.config.period_ms {
            return None;
        }

        self.last_move_ms = current_time_ms;
        self.total_jiggles = self.total_jiggles.saturating_add(1);

        let scale = self.config.scale_px.clamp(1, 400) as i32;

        match self.config.mode {
            JiggleMode::Random => {
                let rnd = self.next_prng();
                let span = (scale as u32 * 2) + 1;
                let rx = (rnd % span) as i32 - scale;
                let ry = ((rnd >> 16) % span) as i32 - scale;
                let dx = if rx == 0 { 1 } else { rx };
                let dy = if ry == 0 { -1 } else { ry };

                self.cum_x = self.cum_x.saturating_add(dx);
                self.cum_y = self.cum_y.saturating_add(dy);

                let step_x = dx.clamp(-120, 120) as i8;
                let step_y = dy.clamp(-120, 120) as i8;
                self.pending_dx = dx - (step_x as i32);
                self.pending_dy = dy - (step_y as i32);
                self.last_report_ms = current_time_ms;

                Some(JiggleAction {
                    dx: step_x,
                    dy: step_y,
                    buttons: 0,
                })
            }
            JiggleMode::GentleNudge => {
                let step = 2i8;
                let (dx, dy) = if self.total_jiggles.is_multiple_of(2) {
                    (step, 0)
                } else {
                    (0, step)
                };
                self.nudge_return = Some((dx, dy));
                self.last_report_ms = current_time_ms;
                Some(JiggleAction { dx, dy, buttons: 0 })
            }
            JiggleMode::Circle => {
                // Smooth 16-point circular orbit using fixed-point precomputed trigonometry.
                // `scale` is the circle's DIAMETER (so radius R = diameter / 2).
                // Since TRIG_TABLE values are cos/sin * 1024, P[i] = (TRIG * diameter + 1024) >> 11
                // divides by 2048 (= 2 * 1024), giving exact diameter span and zero cumulative drift.
                const CIRCLE_POINTS: usize = 16;
                const TRIG_TABLE: [(i32, i32); CIRCLE_POINTS] = [
                    (1024, 0),
                    (946, 392),
                    (724, 724),
                    (392, 946),
                    (0, 1024),
                    (-392, 946),
                    (-724, 724),
                    (-946, 392),
                    (-1024, 0),
                    (-946, -392),
                    (-724, -724),
                    (-392, -946),
                    (0, -1024),
                    (392, -946),
                    (724, -724),
                    (946, -392),
                ];

                let cur_idx = (self.angle_idx as usize) % CIRCLE_POINTS;
                let next_idx = (cur_idx + 1) % CIRCLE_POINTS;
                self.angle_idx = next_idx as u8;

                let diameter = scale;
                let p1_x = (TRIG_TABLE[cur_idx].0 * diameter + 1024) >> 11;
                let p1_y = (TRIG_TABLE[cur_idx].1 * diameter + 1024) >> 11;
                let p2_x = (TRIG_TABLE[next_idx].0 * diameter + 1024) >> 11;
                let p2_y = (TRIG_TABLE[next_idx].1 * diameter + 1024) >> 11;

                let dx = p2_x - p1_x;
                let dy = p2_y - p1_y;

                // When next_idx == 0, we have completed a full circle and returned to home P[0]
                if next_idx == 0 && self.config.click_home {
                    self.click_after_move = true;
                }

                let step_x = dx.clamp(-120, 120) as i8;
                let step_y = dy.clamp(-120, 120) as i8;
                self.pending_dx = dx - (step_x as i32);
                self.pending_dy = dy - (step_y as i32);
                self.last_report_ms = current_time_ms;

                if self.pending_dx == 0 && self.pending_dy == 0 && self.click_after_move {
                    self.click_after_move = false;
                    self.click_state = ClickState::PressPending;
                }

                Some(JiggleAction {
                    dx: step_x,
                    dy: step_y,
                    buttons: 0,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circle_diameter_and_click_home() {
        let cfg = JiggleConfig {
            enabled: true,
            mode: JiggleMode::Circle,
            period_ms: 100,
            scale_px: 200, // diameter = 200
            return_to_home: false,
            rth_period_secs: 30,
            click_home: true,
        };
        let mut engine = JigglerEngine::new(cfg);
        engine.on_jiggle_start(0);

        let mut cur_x: i32 = 0;
        let mut cur_y: i32 = 0;
        let mut min_x: i32 = 0;
        let mut max_x: i32 = 0;
        let mut click_pressed = false;
        let mut click_released = false;

        let mut t = 0u32;
        // Run through 16 circle steps (16 * 100ms = 1600ms) plus 60ms for click press/release
        while t < 1660 {
            t += 20;
            if let Some(act) = engine.update(t, false) {
                cur_x += act.dx as i32;
                cur_y += act.dy as i32;
                min_x = min_x.min(cur_x);
                max_x = max_x.max(cur_x);
                if act.buttons == 1 {
                    click_pressed = true;
                } else if click_pressed && act.buttons == 0 && act.dx == 0 && act.dy == 0 {
                    click_released = true;
                }
            }
        }

        // After 16 steps, cumulative position returns to (0, 0)
        assert_eq!(cur_x, 0);
        assert_eq!(cur_y, 0);
        // Peak-to-peak horizontal span equals the 200px diameter (from 0 down to -200)
        assert_eq!(max_x - min_x, 200);
        assert!(click_pressed);
        assert!(click_released);
    }

    #[test]
    fn test_random_rth_and_400px_scale_splitting() {
        let cfg = JiggleConfig {
            enabled: true,
            mode: JiggleMode::Random,
            period_ms: 500,
            scale_px: 400,
            return_to_home: true,
            rth_period_secs: 2, // RTH at 2000ms
            click_home: true,
        };
        let mut engine = JigglerEngine::new(cfg);
        engine.on_jiggle_start(0);

        let mut cur_x: i32 = 0;
        let mut cur_y: i32 = 0;
        let mut click_pressed = false;

        // Step up to 1900ms (random movements at 500, 1000, 1500ms)
        let mut t = 0u32;
        while t < 1980 {
            t += 20;
            if let Some(act) = engine.update(t, false) {
                cur_x += act.dx as i32;
                cur_y += act.dy as i32;
            }
        }
        // Should have moved away from (0, 0)
        assert!(cur_x != 0 || cur_y != 0);

        // Now cross the 2000ms RTH boundary and drain all multi-packet return steps + click
        while t <= 2300 {
            t += 20;
            if let Some(act) = engine.update(t, false) {
                cur_x += act.dx as i32;
                cur_y += act.dy as i32;
                if act.buttons == 1 {
                    click_pressed = true;
                }
            }
        }

        // RTH must have returned cursor to exact home (0, 0) and clicked home
        assert_eq!(cur_x, 0);
        assert_eq!(cur_y, 0);
        assert!(click_pressed);
    }

    #[test]
    fn test_gentle_nudge_returns_to_zero() {
        let cfg = JiggleConfig {
            enabled: true,
            mode: JiggleMode::GentleNudge,
            period_ms: 200,
            scale_px: 20,
            return_to_home: false,
            rth_period_secs: 30,
            click_home: false,
        };
        let mut engine = JigglerEngine::new(cfg);
        engine.on_jiggle_start(0);

        let first = engine.update(200, false).unwrap();
        assert!(first.dx != 0 || first.dy != 0);

        let second = engine.update(230, false).unwrap();
        assert_eq!(first.dx + second.dx, 0);
        assert_eq!(first.dy + second.dy, 0);
    }
}
