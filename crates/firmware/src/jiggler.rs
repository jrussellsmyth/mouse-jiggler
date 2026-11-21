use common::{JiggleConfig, JiggleMode};

pub struct JigglerEngine {
    config: JiggleConfig,
    prng_state: u32,
    angle_idx: u8,
    last_move_ms: u32,
    total_jiggles: u32,
    return_step: Option<(i8, i8)>,
}

impl JigglerEngine {
    pub fn new(config: JiggleConfig) -> Self {
        Self {
            config,
            prng_state: 0x12345678,
            angle_idx: 0,
            last_move_ms: 0,
            total_jiggles: 0,
            return_step: None,
        }
    }

    pub fn config(&self) -> &JiggleConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: JiggleConfig) {
        if self.config.mode != config.mode {
            self.angle_idx = 0;
            self.return_step = None;
        }
        self.config = config;
        self.config.sanitize();
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

    pub fn update(&mut self, current_time_ms: u32) -> Option<(i8, i8)> {
        if !self.config.enabled || self.config.mode == JiggleMode::Off {
            self.return_step = None;
            return None;
        }

        if let Some((rx, ry)) = self.return_step.take() {
            return Some((-rx, -ry));
        }

        if current_time_ms.wrapping_sub(self.last_move_ms) < self.config.period_ms {
            return None;
        }

        self.last_move_ms = current_time_ms;
        self.total_jiggles = self.total_jiggles.saturating_add(1);

        let scale = self.config.scale_px.max(1);

        match self.config.mode {
            JiggleMode::Off => None,
            JiggleMode::Random => {
                let rnd = self.next_prng();
                let rx = ((rnd % ((scale as u32 * 2) + 1)) as i32 - scale as i32) as i8;
                let ry = (((rnd >> 16) % ((scale as u32 * 2) + 1)) as i32 - scale as i32) as i8;
                let dx = if rx == 0 { 1 } else { rx };
                let dy = if ry == 0 { -1 } else { ry };
                self.return_step = Some((dx, dy));
                Some((dx, dy))
            }
            JiggleMode::GentleNudge => {
                let step = scale.min(2);
                let (dx, dy) = if self.total_jiggles % 2 == 0 {
                    (step, 0)
                } else {
                    (0, step)
                };
                self.return_step = Some((dx, dy));
                Some((dx, dy))
            }
            JiggleMode::Circle => {
                self.angle_idx = (self.angle_idx + 1) % 8;
                // Incremental displacements along an 8-sided regular polygon:
                // Sum of dx = (scale) + 0 + (-scale) + (-scale) + 0 + (scale) + (scale) + (-scale) = 0
                // Sum of dy = 0 + (scale) + (scale) + 0 + (-scale) + (-scale) + 0 + 0 = 0
                let (dx, dy) = match self.angle_idx {
                    0 => (scale, 0),
                    1 => (0, scale),
                    2 => (-scale, 0),
                    3 => (-scale, 0),
                    4 => (0, -scale),
                    5 => (0, -scale),
                    6 => (scale, 0),
                    _ => (0, scale),
                };
                Some((dx, dy))
            }
        }
    }
}
