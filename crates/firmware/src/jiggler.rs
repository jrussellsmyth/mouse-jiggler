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
                // Smooth 16-point circular orbit using fixed-point precomputed trigonometry
                // Each point P[i] = (round(R * cos θ), round(R * sin θ))
                // The delta between consecutive points dx = P[i+1].x - P[i].x, dy = P[i+1].y - P[i].y
                // Because P[16] == P[0], sum(dx) = 0 and sum(dy) = 0 exactly for any radius.
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

                let r = scale as i32;
                let p1_x = (TRIG_TABLE[cur_idx].0 * r + 512) >> 10;
                let p1_y = (TRIG_TABLE[cur_idx].1 * r + 512) >> 10;
                let p2_x = (TRIG_TABLE[next_idx].0 * r + 512) >> 10;
                let p2_y = (TRIG_TABLE[next_idx].1 * r + 512) >> 10;

                let dx = (p2_x - p1_x) as i8;
                let dy = (p2_y - p1_y) as i8;

                Some((dx, dy))
            }
        }
    }
}
