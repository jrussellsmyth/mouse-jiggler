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
    manual_disconnect: bool,
    config: JiggleConfig,
    has_synced_initial_config: bool,
    jiggles: u32,
    uptime_secs: u32,
    device_paused: bool,
    device_testing: bool,
    status_message: String,
    action_feedback: Option<(String, std::time::Instant)>,
    last_ping: std::time::Instant,
}

impl Default for JigglerApp {
    fn default() -> Self {
        let ports = SerialClient::list_ports();
        let selected = ports.first().cloned().unwrap_or_default();
        let (mut client, _) = SerialClient::new();
        let mut event_rx = None;
        let mut connected_port = None;
        let mut status_message = "Disconnected".into();

        // Auto-connect on launch if an RP2040 USB serial device is detected
        if !selected.is_empty() && (selected.contains("usbmodem") || selected.contains("ttyACM")) {
            if let Ok(rx) = client.connect(&selected) {
                event_rx = Some(rx);
                connected_port = Some(selected.clone());
                status_message = format!("Auto-connected to {}", selected);
            }
        }

        Self {
            client,
            event_rx,
            available_ports: ports,
            selected_port: selected,
            connected_port,
            manual_disconnect: false,
            config: JiggleConfig::default(),
            has_synced_initial_config: false,
            jiggles: 0,
            uptime_secs: 0,
            device_paused: false,
            device_testing: false,
            status_message,
            action_feedback: None,
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
                        self.device_paused = false;
                        self.device_testing = false;
                        self.status_message = "Disconnected (waiting for device...)".into();
                    }
                    ClientEvent::StatusReceived {
                        config,
                        jiggles,
                        uptime,
                        paused,
                        testing,
                    } => {
                        // Only sync config into GUI sliders on the very first read
                        // so user edits aren't overwritten by periodic background heartbeats
                        if !self.has_synced_initial_config {
                            self.config = config;
                            self.has_synced_initial_config = true;
                        }
                        self.jiggles = jiggles;
                        self.uptime_secs = uptime;
                        self.device_paused = paused;
                        self.device_testing = testing;
                        if testing {
                            self.status_message = "Test jiggle running (10s demo)...".into();
                        } else if paused {
                            self.status_message =
                                "Connected (jiggler paused for configuration)".into();
                        } else {
                            self.status_message = "Status synchronized".into();
                        }
                    }
                    ClientEvent::CommandAck => {
                        self.status_message = "Command accepted by device".into();
                    }
                    ClientEvent::Error(err) => {
                        self.action_feedback =
                            Some((format!("Error: {}", err), std::time::Instant::now()));
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
                            self.manual_disconnect = false;
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
                        self.manual_disconnect = true;
                        self.client.disconnect();
                        self.connected_port = None;
                        self.event_rx = None;
                        self.has_synced_initial_config = false;
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
                });

                let full_slider_width = (ui.available_width() - 65.0).max(160.0);
                ui.spacing_mut().slider_width = full_slider_width;

                ui.add_space(4.0);
                ui.label(format!(
                    "Interval: {} ms (Frequency: {:.1} Hz)",
                    self.config.period_ms,
                    1000.0 / self.config.period_ms.max(1) as f32
                ));
                ui.add(egui::Slider::new(&mut self.config.period_ms, 100..=10_000));

                match self.config.mode {
                    JiggleMode::Random => {
                        ui.add_space(4.0);
                        ui.label(format!("Displacement: {} px", self.config.scale_px));
                        ui.add(egui::Slider::new(&mut self.config.scale_px, 1..=400));

                        ui.add_space(6.0);
                        ui.checkbox(&mut self.config.return_to_home, "Return to Home");

                        if self.config.return_to_home {
                            ui.add_space(4.0);
                            ui.label(format!(
                                "Return to Home Period: {} s",
                                self.config.rth_period_secs
                            ));
                            ui.add(egui::Slider::new(&mut self.config.rth_period_secs, 1..=300));

                            ui.add_space(4.0);
                            ui.checkbox(&mut self.config.click_home, "Click Home");
                        }
                    }
                    JiggleMode::Circle => {
                        ui.add_space(4.0);
                        ui.label(format!("Diameter: {} px", self.config.scale_px));
                        ui.add(egui::Slider::new(&mut self.config.scale_px, 1..=400));

                        ui.add_space(6.0);
                        ui.checkbox(&mut self.config.click_home, "Click Home");
                    }
                    JiggleMode::GentleNudge => {
                        // Scale slider and label hidden in Gentle Nudge mode
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let is_connected = self.connected_port.is_some();

                    if ui
                        .add_enabled(is_connected, egui::Button::new("Apply Settings"))
                        .clicked()
                    {
                        let _ = self.client.send_command(Command::SetConfig(self.config));
                        self.action_feedback = Some((
                            "Settings applied to device RAM".into(),
                            std::time::Instant::now(),
                        ));
                    }

                    if ui
                        .add_enabled(is_connected, egui::Button::new("Save To Flash"))
                        .clicked()
                    {
                        // Ensure current GUI settings are applied before committing to flash
                        let _ = self.client.send_command(Command::SetConfig(self.config));
                        let _ = self.client.send_command(Command::SaveConfig);
                        self.action_feedback = Some((
                            "Settings applied and saved to Flash!".into(),
                            std::time::Instant::now(),
                        ));
                    }

                    if ui
                        .add_enabled(is_connected, egui::Button::new("Read Device Status"))
                        .clicked()
                    {
                        self.has_synced_initial_config = false;
                        let _ = self.client.send_command(Command::GetStatus);
                        self.action_feedback = Some((
                            "Reloaded active settings from device".into(),
                            std::time::Instant::now(),
                        ));
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
                        let _ = self.client.send_command(Command::SetConfig(self.config));
                        let _ = self
                            .client
                            .send_command(Command::StartTest { duration_secs: 10 });
                        self.device_testing = true;
                        self.action_feedback = Some((
                            "Running 10-second test jiggle sequence...".into(),
                            std::time::Instant::now(),
                        ));
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
            let displayed_status = if let Some((ref msg, ts)) = self.action_feedback {
                if ts.elapsed() < std::time::Duration::from_secs(4) {
                    format!("{} | {}", msg, self.status_message)
                } else {
                    self.status_message.clone()
                }
            } else {
                self.status_message.clone()
            };
            ui.label(format!("Status: {}", displayed_status));
        });

        // Periodic heartbeat when connected to keep the device paused during config,
        // or auto-reconnect when disconnected (e.g. after tapping the hardware RESET button).
        if self.last_ping.elapsed() >= std::time::Duration::from_millis(1500) {
            if self.connected_port.is_some() {
                let _ = self.client.send_command(Command::GetStatus);
            } else if !self.manual_disconnect {
                self.available_ports = SerialClient::list_ports();
                if let Some(first_port) = self.available_ports.first().cloned() {
                    if first_port.contains("usbmodem") || first_port.contains("ttyACM") {
                        self.selected_port = first_port.clone();
                        if let Ok(rx) = self.client.connect(&first_port) {
                            self.event_rx = Some(rx);
                        }
                    }
                }
            }
            self.last_ping = std::time::Instant::now();
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}
