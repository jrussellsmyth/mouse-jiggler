use anyhow::{bail, Result};
use common::{Command, JiggleConfig, JiggleMode};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum ClientEvent {
    Connected(String),
    Disconnected,
    StatusReceived {
        config: JiggleConfig,
        jiggles: u32,
        uptime: u32,
        paused: bool,
        testing: bool,
    },
    CommandAck,
    Error(String),
}

pub struct SerialClient {
    cmd_tx: Option<Sender<Command>>,
    stop_signal: Arc<AtomicBool>,
    event_rx: Receiver<ClientEvent>,
}

impl SerialClient {
    pub fn new() -> (Self, Receiver<ClientEvent>) {
        let (_event_tx, event_rx) = bounded(32);
        (
            Self {
                cmd_tx: None,
                stop_signal: Arc::new(AtomicBool::new(false)),
                event_rx: event_rx.clone(),
            },
            event_rx,
        )
    }

    pub fn list_ports() -> Vec<String> {
        let mut ports: Vec<String> = serialport::available_ports()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.port_name)
            // On macOS, exclude /dev/tty.* dial-in nodes in favor of /dev/cu.* call-out nodes
            .filter(|name| !name.starts_with("/dev/tty."))
            .collect();

        // Sort RP2040 / USB modem ports to the top
        ports.sort_by(|a, b| {
            let a_score = if a.contains("usbmodem") || a.contains("ttyACM") {
                0
            } else {
                1
            };
            let b_score = if b.contains("usbmodem") || b.contains("ttyACM") {
                0
            } else {
                1
            };
            a_score.cmp(&b_score).then_with(|| a.cmp(b))
        });

        ports
    }

    pub fn connect(&mut self, port_name: &str) -> Result<Receiver<ClientEvent>> {
        self.disconnect();

        let (cmd_tx, cmd_rx) = bounded::<Command>(32);
        let (event_tx, event_rx) = bounded(64);
        let stop = Arc::new(AtomicBool::new(false));

        self.cmd_tx = Some(cmd_tx);
        self.stop_signal = Arc::clone(&stop);
        self.event_rx = event_rx.clone();

        let port_path = port_name.to_string();

        thread::spawn(move || {
            let port_res = serialport::new(&port_path, 115_200)
                .timeout(Duration::from_millis(30))
                .open();

            let mut port = match port_res {
                Ok(mut p) => {
                    let _ = p.write_data_terminal_ready(true);
                    let _ = p.write_request_to_send(true);
                    let _ = event_tx.send(ClientEvent::Connected(port_path.clone()));
                    p
                }
                Err(e) => {
                    let _ =
                        event_tx.send(ClientEvent::Error(format!("Failed to open port: {}", e)));
                    let _ = event_tx.send(ClientEvent::Disconnected);
                    return;
                }
            };

            // Query initial status
            if port.write_all(b"GET\r\n").is_err() {
                let _ = event_tx.send(ClientEvent::Disconnected);
                return;
            }
            let _ = port.flush();

            let mut rx_buf = [0u8; 256];
            let mut line_acc = String::new();

            'outer: while !stop.load(Ordering::Relaxed) {
                // Drain and send outgoing commands
                while let Ok(cmd) = cmd_rx.try_recv() {
                    let payload = match cmd {
                        Command::GetStatus => "GET\r\n".to_string(),
                        Command::SaveConfig => "SAVE\r\n".to_string(),
                        Command::ResetDefaults => "RESET\r\n".to_string(),
                        Command::Pause => "PAUSE\r\n".to_string(),
                        Command::Resume => "RESUME\r\n".to_string(),
                        Command::StartTest { duration_secs } => {
                            format!("TEST {}\r\n", duration_secs)
                        }
                        Command::SetConfig(cfg) => {
                            let mode_str = match cfg.mode {
                                JiggleMode::Random => "random",
                                JiggleMode::GentleNudge => "nudge",
                                JiggleMode::Circle => "circle",
                            };
                            format!(
                                "SET enabled={} mode={} period={} scale={} rth={} rth_period={} click_home={}\r\n",
                                if cfg.enabled { 1 } else { 0 },
                                mode_str,
                                cfg.period_ms,
                                cfg.scale_px,
                                if cfg.return_to_home { 1 } else { 0 },
                                cfg.rth_period_secs,
                                if cfg.click_home { 1 } else { 0 },
                            )
                        }
                    };

                    if let Err(e) = port.write_all(payload.as_bytes()) {
                        let _ = event_tx.send(ClientEvent::Error(format!("Write failed: {}", e)));
                        break 'outer;
                    }
                    let _ = port.flush();
                }

                // Read available incoming bytes into persistent line accumulator
                match port.read(&mut rx_buf) {
                    Ok(0) => {
                        // EOF / POLLHUP: USB serial device was reset or unplugged
                        break 'outer;
                    }
                    Ok(n) => {
                        if let Ok(chunk) = std::str::from_utf8(&rx_buf[..n]) {
                            line_acc.push_str(chunk);
                        } else {
                            for &b in &rx_buf[..n] {
                                if b.is_ascii() {
                                    line_acc.push(b as char);
                                }
                            }
                        }

                        while let Some(newline_idx) = line_acc.find('\n') {
                            let line: String = line_acc.drain(..=newline_idx).collect();
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            if trimmed == "OK" {
                                let _ = event_tx.send(ClientEvent::CommandAck);
                            } else if trimmed.starts_with("ERR:") {
                                let _ = event_tx.send(ClientEvent::Error(trimmed.to_string()));
                            } else if let Some(rest) = trimmed.strip_prefix("STATUS ") {
                                let mut cfg = JiggleConfig::default();
                                let mut jiggles = 0u32;
                                let mut uptime = 0u32;
                                let mut paused = false;
                                let mut testing = false;

                                for token in rest.split_whitespace() {
                                    let mut kv = token.split('=');
                                    let k = kv.next().unwrap_or("");
                                    let v = kv.next().unwrap_or("");
                                    match k {
                                        "enabled" => cfg.enabled = v == "1" || v == "true",
                                        "mode" => match v {
                                            "random" => cfg.mode = JiggleMode::Random,
                                            "nudge" => cfg.mode = JiggleMode::GentleNudge,
                                            "circle" => cfg.mode = JiggleMode::Circle,
                                            _ => {}
                                        },
                                        "period_ms" => cfg.period_ms = v.parse().unwrap_or(1000),
                                        "scale_px" => cfg.scale_px = v.parse().unwrap_or(20),
                                        "rth" => cfg.return_to_home = v == "1" || v == "true",
                                        "rth_period" => {
                                            cfg.rth_period_secs = v.parse().unwrap_or(30)
                                        }
                                        "click_home" => cfg.click_home = v == "1" || v == "true",
                                        "jiggles" => jiggles = v.parse().unwrap_or(0),
                                        "uptime" => uptime = v.parse().unwrap_or(0),
                                        "paused" => paused = v == "1" || v == "true",
                                        "testing" => testing = v == "1" || v == "true",
                                        _ => {}
                                    }
                                }
                                let _ = event_tx.send(ClientEvent::StatusReceived {
                                    config: cfg,
                                    jiggles,
                                    uptime,
                                    paused,
                                    testing,
                                });
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => {
                        let _ = event_tx.send(ClientEvent::Error(format!("Read error: {}", e)));
                        break 'outer;
                    }
                }
            }

            let _ = event_tx.send(ClientEvent::Disconnected);
        });

        Ok(event_rx)
    }

    pub fn disconnect(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        self.cmd_tx = None;
    }

    #[allow(dead_code)]
    pub fn is_connected(&self) -> bool {
        self.cmd_tx.is_some() && !self.stop_signal.load(Ordering::Relaxed)
    }

    pub fn send_command(&self, cmd: Command) -> Result<()> {
        if let Some(ref tx) = self.cmd_tx {
            tx.try_send(cmd)
                .map_err(|e| anyhow::anyhow!("Failed to queue command: {}", e))
        } else {
            bail!("Not connected to serial device")
        }
    }
}
