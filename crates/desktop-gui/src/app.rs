use common::{Command, JiggleConfig, JiggleMode};
use crossbeam_channel::Receiver;
use eframe::egui;

use crate::serial_client::{ClientEvent, SerialClient};

pub struct JigglerApp {
    client: SerialClient,
    event_rx: Option<Receiver<ClientEvent>>,
    available_ports: Vec<String>,
    selected_port: String,
    connected_port: Option<String>,
    config: JiggleConfig,
    jiggles: u32,
    uptime_secs: u32,
    device_paused: bool,
    device_testing: bool,
    status_message: String,
    last_ping: std::time::Instant,
}

impl Default for JigglerApp {
    fn default() -> Self {
        let ports = SerialClient::list_ports();
        let selected = ports.first().cloned().unwrap_or_default();
        let (client, _) = SerialClient::new();
        Self {
            client,
            event_rx: None,
            available_ports: ports,
            selected_port: selected,
            connected_port: None,
            config: JiggleConfig::default(),
            jiggles: 0,
            uptime_secs: 0,
            device_paused: false,
            device_testing: false,
            status_message: "Disconnected".into(),
            last_ping: std::time::Instant::now(),
        }
    }
}

impl eframe::App for JigglerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(ref rx) = self.event_rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    ClientEvent::Connected(port) => {
                        self.connected_port = Some(port.clone());
                        self.status_message = format!("Connected to {}", port);
                    }
                    ClientEvent::Disconnected => {
                        self.connected_port = None;
                        self.status_message = "Disconnected".into();
                    }
                    ClientEvent::StatusReceived {
                        config,
                        jiggles,
                        uptime,
                        paused,
                        testing,
                    } => {
                        self.config = config;
                        self.jiggles = jiggles;
                        self.uptime_secs = uptime;
                        self.device_paused = paused;
                        self.device_testing = testing;
                        if testing {
                            self.status_message = "Test jiggle running...".into();
                        } else if paused {
                            self.status_message =
                                "Connected (jiggler paused for configuration)".into();
                        } else {
                            self.status_message = "Status synchronized".into();
                        }
                    }
                    ClientEvent::CommandAck => {
                        self.status_message = "Command accepted".into();
                    }
                    ClientEvent::Error(err) => {
                        self.status_message = format!("Error: {}", err);
                    }
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RP2040 Mouse Jiggler Controller");
            ui.add_space(8.0);

            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Serial Port:");
                    egui::ComboBox::from_label("")
                        .selected_text(&self.selected_port)
                        .show_ui(ui, |ui| {
                            for port in &self.available_ports {
                                ui.selectable_value(&mut self.selected_port, port.clone(), port);
                            }
                        });

                    if ui.button("Refresh").clicked() {
                        self.available_ports = SerialClient::list_ports();
                        if !self.available_ports.contains(&self.selected_port) {
                            self.selected_port =
                                self.available_ports.first().cloned().unwrap_or_default();
                        }
                    }

                    if self.connected_port.is_none() {
                        if ui.button("Connect").clicked() && !self.selected_port.is_empty() {
                            match self.client.connect(&self.selected_port) {
                                Ok(rx) => {
                                    self.event_rx = Some(rx);
                                    self.status_message =
                                        format!("Connecting to {}...", self.selected_port);
                                }
                                Err(e) => {
                                    self.status_message = format!("Connect failed: {}", e);
                                }
                            }
                        }
                    } else if ui.button("Disconnect").clicked() {
                        self.client.disconnect();
                        self.connected_port = None;
                        self.event_rx = None;
                        self.status_message = "Disconnected".into();
                    }
                });
            });

            ui.add_space(8.0);

            ui.group(|ui| {
                ui.label("Configuration");
                ui.separator();

                ui.checkbox(&mut self.config.enabled, "Enable Mouse Jiggling");

                ui.horizontal(|ui| {
                    ui.label("Movement Mode:");
                    ui.selectable_value(&mut self.config.mode, JiggleMode::Random, "Random");
                    ui.selectable_value(
                        &mut self.config.mode,
                        JiggleMode::GentleNudge,
                        "Gentle Nudge",
                    );
                    ui.selectable_value(&mut self.config.mode, JiggleMode::Circle, "Circle");
                    ui.selectable_value(&mut self.config.mode, JiggleMode::Off, "Off");
                });

                ui.add_space(4.0);
                ui.label(format!(
                    "Period: {} ms (Frequency: {:.1} Hz)",
                    self.config.period_ms,
                    1000.0 / self.config.period_ms.max(1) as f32
                ));
                ui.add(
                    egui::Slider::new(&mut self.config.period_ms, 100..=10_000)
                        .text("Interval (ms)"),
                );

                ui.add_space(4.0);
                ui.label(format!("Movement Scale: {} px", self.config.scale_px));
                ui.add(
                    egui::Slider::new(&mut self.config.scale_px, 1..=50).text("Displacement (px)"),
                );

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let is_connected = self.connected_port.is_some();

                    if ui
                        .add_enabled(is_connected, egui::Button::new("Apply Settings"))
                        .clicked()
                    {
                        let _ = self.client.send_command(Command::SetConfig(self.config));
                    }

                    if ui
                        .add_enabled(is_connected, egui::Button::new("Save To Flash"))
                        .clicked()
                    {
                        let _ = self.client.send_command(Command::SaveConfig);
                    }

                    if ui
                        .add_enabled(is_connected, egui::Button::new("Read Device Status"))
                        .clicked()
                    {
                        let _ = self.client.send_command(Command::GetStatus);
                    }
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let is_connected = self.connected_port.is_some();

                    let test_btn_text = if self.device_testing {
                        "Testing Sequence Running..."
                    } else {
                        "Test Jiggle (10s Demo)"
                    };

                    if ui
                        .add_enabled(
                            is_connected && !self.device_testing,
                            egui::Button::new(test_btn_text),
                        )
                        .clicked()
                    {
                        let _ = self
                            .client
                            .send_command(Command::StartTest { duration_secs: 10 });
                    }
                });
            });

            ui.add_space(8.0);

            ui.group(|ui| {
                ui.label("Device Statistics & State");
                ui.separator();
                let state_str = if self.connected_port.is_none() {
                    "Disconnected (Auto-jiggling when plugged into USB)"
                } else if self.device_testing {
                    "TESTING SEQUENCE ACTIVE (10s)"
                } else if self.device_paused {
                    "PAUSED (Configuration session active)"
                } else {
                    "Active"
                };
                ui.label(format!("Device State: {}", state_str));
                ui.label(format!("Total Jiggles Sent: {}", self.jiggles));
                ui.label(format!(
                    "Device Uptime: {}s ({:.1} min)",
                    self.uptime_secs,
                    self.uptime_secs as f32 / 60.0
                ));
            });

            ui.add_space(8.0);
            ui.separator();
            ui.label(format!("Status: {}", self.status_message));
        });

        // Periodic heartbeat when connected to keep the device paused during config
        if self.connected_port.is_some()
            && self.last_ping.elapsed() >= std::time::Duration::from_millis(1500)
        {
            let _ = self.client.send_command(Command::GetStatus);
            self.last_ping = std::time::Instant::now();
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}
