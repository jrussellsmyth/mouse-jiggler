#![no_std]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum JiggleMode {
    Off,
    #[default]
    Random,
    GentleNudge,
    Circle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JiggleConfig {
    pub enabled: bool,
    pub mode: JiggleMode,
    pub period_ms: u32,
    pub scale_px: i8,
}

impl Default for JiggleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: JiggleMode::Random,
            period_ms: 1000,
            scale_px: 5,
        }
    }
}

impl JiggleConfig {
    pub fn sanitize(&mut self) {
        self.period_ms = self.period_ms.clamp(50, 300_000);
        self.scale_px = self.scale_px.clamp(1, 100);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    GetStatus,
    SetConfig(JiggleConfig),
    SaveConfig,
    ResetDefaults,
    StartTest { duration_secs: u32 },
    Pause,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Status {
        config: JiggleConfig,
        total_jiggles: u32,
        uptime_secs: u32,
        paused: bool,
        testing: bool,
    },
    Ok,
    Error(heapless::String<64>),
}

pub fn make_string<const N: usize>(s: &str) -> heapless::String<N> {
    let mut res = heapless::String::new();
    let _ = res.push_str(s);
    res
}

pub fn parse_command(input: &str) -> Result<Command, heapless::String<64>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(make_string("empty command"));
    }

    if trimmed.eq_ignore_ascii_case("GET") || trimmed.eq_ignore_ascii_case("STATUS") {
        return Ok(Command::GetStatus);
    }
    if trimmed.eq_ignore_ascii_case("SAVE") {
        return Ok(Command::SaveConfig);
    }
    if trimmed.eq_ignore_ascii_case("RESET") {
        return Ok(Command::ResetDefaults);
    }
    if trimmed.eq_ignore_ascii_case("PAUSE") {
        return Ok(Command::Pause);
    }
    if trimmed.eq_ignore_ascii_case("RESUME") {
        return Ok(Command::Resume);
    }
    if trimmed.starts_with("TEST") || trimmed.starts_with("test") {
        let secs = trimmed
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(10);
        return Ok(Command::StartTest {
            duration_secs: secs,
        });
    }

    if let Ok((cmd, _)) = serde_json_core::from_str::<Command>(trimmed) {
        return Ok(cmd);
    }

    if trimmed.starts_with("SET ") || trimmed.starts_with("set ") {
        let parts = trimmed[4..].split_whitespace();
        let mut config = JiggleConfig::default();
        for part in parts {
            let mut kv = part.split('=');
            let key = kv.next().unwrap_or("");
            let val = kv.next().unwrap_or("");
            match key {
                "enabled" => config.enabled = val == "1" || val.eq_ignore_ascii_case("true"),
                "mode" => {
                    if val.eq_ignore_ascii_case("off") {
                        config.mode = JiggleMode::Off;
                    } else if val.eq_ignore_ascii_case("random") {
                        config.mode = JiggleMode::Random;
                    } else if val.eq_ignore_ascii_case("nudge")
                        || val.eq_ignore_ascii_case("gentlenudge")
                    {
                        config.mode = JiggleMode::GentleNudge;
                    } else if val.eq_ignore_ascii_case("circle") {
                        config.mode = JiggleMode::Circle;
                    }
                }
                "period" | "period_ms" => {
                    if let Ok(parsed) = val.parse::<u32>() {
                        config.period_ms = parsed;
                    }
                }
                "scale" | "scale_px" => {
                    if let Ok(parsed) = val.parse::<i8>() {
                        config.scale_px = parsed;
                    }
                }
                _ => {}
            }
        }
        config.sanitize();
        return Ok(Command::SetConfig(config));
    }

    Err(make_string("unknown or invalid command"))
}

pub fn serialize_response(resp: &Response) -> heapless::String<256> {
    let mut out = heapless::String::new();
    match resp {
        Response::Ok => {
            let _ = out.push_str("OK\r\n");
        }
        Response::Error(msg) => {
            let _ = out.push_str("ERR: ");
            let _ = out.push_str(msg.as_str());
            let _ = out.push_str("\r\n");
        }
        Response::Status {
            config,
            total_jiggles,
            uptime_secs,
            paused,
            testing,
        } => {
            let mode_str = match config.mode {
                JiggleMode::Off => "off",
                JiggleMode::Random => "random",
                JiggleMode::GentleNudge => "nudge",
                JiggleMode::Circle => "circle",
            };
            use core::fmt::Write;
            let _ = write!(
                out,
                "STATUS enabled={} mode={} period_ms={} scale_px={} jiggles={} uptime={} paused={} testing={}\r\n",
                if config.enabled { 1 } else { 0 },
                mode_str,
                config.period_ms,
                config.scale_px,
                total_jiggles,
                uptime_secs,
                if *paused { 1 } else { 0 },
                if *testing { 1 } else { 0 },
            );
        }
    }
    out
}
