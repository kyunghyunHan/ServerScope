use chrono::Local;
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui;
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::fs;
use std::io::Read;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1240.0, 760.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ServerScope",
        options,
        Box::new(|cc| Ok(Box::new(ServerScopeApp::new(cc)))),
    )
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ServerConfig {
    name: String,
    host: String,
    port: u16,
    username: String,
    private_key_path: String,
    services: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AppConfig {
    servers: Vec<ServerConfig>,
}

#[derive(Clone, Debug, Default)]
struct ServerSnapshot {
    timestamp: String,
    uptime: String,
    cpu_usage: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    disk_used: String,
    disk_total: String,
    load_average: String,
    network_rx: String,
    network_tx: String,
    connections: Vec<NetworkConnection>,
    services: Vec<ServiceStatus>,
    error_logs: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct NetworkConnection {
    protocol: String,
    local: String,
    remote: String,
    state: String,
    process: String,
}

#[derive(Clone, Debug, Default)]
struct ServiceStatus {
    name: String,
    status: String,
    last_checked: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Servers,
    Dashboard,
    Network,
    Services,
    Logs,
    Settings,
}

impl View {
    fn label(self) -> &'static str {
        match self {
            Self::Servers => "Servers",
            Self::Dashboard => "Dashboard",
            Self::Network => "Network",
            Self::Services => "Services",
            Self::Logs => "Logs",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Refreshing,
    Error,
}

impl ConnectionState {
    fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting",
            Self::Connected => "Connected",
            Self::Refreshing => "Refreshing",
            Self::Error => "Error",
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Self::Connected => egui::Color32::from_rgb(42, 157, 86),
            Self::Connecting | Self::Refreshing => egui::Color32::from_rgb(214, 154, 58),
            Self::Error => egui::Color32::from_rgb(214, 73, 73),
            Self::Disconnected => egui::Color32::from_rgb(132, 140, 152),
        }
    }
}

enum WorkerMessage {
    Test(Result<(), String>),
    Snapshot(Result<ServerSnapshot, String>),
}

struct ServerScopeApp {
    config: AppConfig,
    config_path: PathBuf,
    selected_server: Option<usize>,
    view: View,
    connection_state: ConnectionState,
    snapshot: Option<ServerSnapshot>,
    error: Option<String>,
    tx: Sender<WorkerMessage>,
    rx: Receiver<WorkerMessage>,
    draft_server: ServerConfig,
    service_draft: String,
    auto_refresh_secs: u64,
    auto_refresh: bool,
    last_refresh_started: Option<Instant>,
    worker_busy: bool,
}

impl ServerScopeApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_pixels_per_point(1.1);
        let (tx, rx) = unbounded();
        let config_path = config_path();
        let config = load_config(&config_path);

        Self {
            selected_server: (!config.servers.is_empty()).then_some(0),
            config,
            config_path,
            view: View::Servers,
            connection_state: ConnectionState::Disconnected,
            snapshot: None,
            error: None,
            tx,
            rx,
            draft_server: ServerConfig {
                port: 22,
                services: vec!["nginx".into(), "docker".into()],
                ..Default::default()
            },
            service_draft: String::new(),
            auto_refresh_secs: 10,
            auto_refresh: false,
            last_refresh_started: None,
            worker_busy: false,
        }
    }

    fn selected_config(&self) -> Option<ServerConfig> {
        self.selected_server
            .and_then(|idx| self.config.servers.get(idx))
            .cloned()
    }

    fn save_config(&mut self) {
        if let Err(err) = save_config(&self.config_path, &self.config) {
            self.error = Some(err);
            self.connection_state = ConnectionState::Error;
        }
    }

    fn spawn_test(&mut self) {
        let Some(server) = self.selected_config() else {
            self.error = Some("Select a server first.".to_owned());
            return;
        };

        self.connection_state = ConnectionState::Connecting;
        self.error = None;
        self.worker_busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = test_connection(&server);
            let _ = tx.send(WorkerMessage::Test(result));
        });
    }

    fn spawn_refresh(&mut self) {
        let Some(server) = self.selected_config() else {
            self.error = Some("Select a server first.".to_owned());
            return;
        };

        self.connection_state = ConnectionState::Refreshing;
        self.error = None;
        self.worker_busy = true;
        self.last_refresh_started = Some(Instant::now());
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = collect_snapshot(&server);
            let _ = tx.send(WorkerMessage::Snapshot(result));
        });
    }

    fn handle_worker_messages(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            self.worker_busy = false;
            match message {
                WorkerMessage::Test(Ok(())) => {
                    self.connection_state = ConnectionState::Connected;
                    self.error = None;
                    self.spawn_refresh();
                }
                WorkerMessage::Test(Err(err)) => {
                    self.connection_state = ConnectionState::Error;
                    self.error = Some(err);
                }
                WorkerMessage::Snapshot(Ok(snapshot)) => {
                    self.snapshot = Some(snapshot);
                    self.connection_state = ConnectionState::Connected;
                    self.error = None;
                }
                WorkerMessage::Snapshot(Err(err)) => {
                    self.connection_state = ConnectionState::Error;
                    self.error = Some(err);
                }
            }
        }
    }

    fn maybe_auto_refresh(&mut self, ctx: &egui::Context) {
        if !self.auto_refresh
            || self.worker_busy
            || self.connection_state == ConnectionState::Disconnected
        {
            return;
        }

        let due = self
            .last_refresh_started
            .map(|time| time.elapsed() >= Duration::from_secs(self.auto_refresh_secs))
            .unwrap_or(true);

        if due {
            self.spawn_refresh();
        }

        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn draw_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("ServerScope");
            ui.separator();

            let selected_name = self
                .selected_config()
                .map(|server| server.name)
                .unwrap_or_else(|| "No server selected".to_owned());
            ui.label(format!("Server: {selected_name}"));

            ui.add_space(12.0);
            let state = self.connection_state;
            ui.colored_label(state.color(), state.label());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Refresh").clicked() {
                    self.spawn_refresh();
                }

                if self.connection_state == ConnectionState::Disconnected {
                    if ui.button("Connect").clicked() {
                        self.spawn_test();
                    }
                } else if ui.button("Disconnect").clicked() {
                    self.connection_state = ConnectionState::Disconnected;
                    self.auto_refresh = false;
                    self.worker_busy = false;
                }
            });
        });
    }

    fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading("ServerScope");
        ui.add_space(8.0);

        for view in [
            View::Servers,
            View::Dashboard,
            View::Network,
            View::Services,
            View::Logs,
            View::Settings,
        ] {
            if ui
                .selectable_label(self.view == view, view.label())
                .clicked()
            {
                self.view = view;
            }
        }

        ui.separator();
        ui.label("Servers");
        for (idx, server) in self.config.servers.iter().enumerate() {
            if ui
                .selectable_label(self.selected_server == Some(idx), &server.name)
                .clicked()
            {
                self.selected_server = Some(idx);
                self.snapshot = None;
                self.connection_state = ConnectionState::Disconnected;
                self.error = None;
            }
        }
    }

    fn draw_servers(&mut self, ui: &mut egui::Ui) {
        ui.heading("Servers");
        ui.add_space(10.0);

        egui::Grid::new("server_form")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut self.draft_server.name);
                ui.end_row();
                ui.label("Host");
                ui.text_edit_singleline(&mut self.draft_server.host);
                ui.end_row();
                ui.label("Port");
                ui.add(egui::DragValue::new(&mut self.draft_server.port).range(1..=65535));
                ui.end_row();
                ui.label("Username");
                ui.text_edit_singleline(&mut self.draft_server.username);
                ui.end_row();
                ui.label("Private key path");
                ui.text_edit_singleline(&mut self.draft_server.private_key_path);
                ui.end_row();
            });

        ui.horizontal(|ui| {
            if ui.button("Add Server").clicked() {
                if self.draft_server.name.trim().is_empty()
                    || self.draft_server.host.trim().is_empty()
                    || self.draft_server.username.trim().is_empty()
                    || self.draft_server.private_key_path.trim().is_empty()
                {
                    self.error =
                        Some("Name, host, username, and private key path are required.".into());
                } else {
                    self.config.servers.push(self.draft_server.clone());
                    self.selected_server = Some(self.config.servers.len() - 1);
                    self.draft_server = ServerConfig {
                        port: 22,
                        services: vec!["nginx".into(), "docker".into()],
                        ..Default::default()
                    };
                    self.save_config();
                }
            }

            if ui.button("Remove Selected").clicked() {
                if let Some(idx) = self.selected_server {
                    if idx < self.config.servers.len() {
                        self.config.servers.remove(idx);
                        self.selected_server = (!self.config.servers.is_empty()).then_some(0);
                        self.snapshot = None;
                        self.connection_state = ConnectionState::Disconnected;
                        self.save_config();
                    }
                }
            }
        });

        ui.separator();
        ui.label(format!("Config: {}", self.config_path.display()));
    }

    fn draw_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dashboard");
        ui.add_space(10.0);

        if let Some(snapshot) = &self.snapshot {
            egui::Grid::new("dashboard_cards")
                .num_columns(3)
                .spacing([12.0, 12.0])
                .show(ui, |ui| {
                    metric_card(ui, "CPU", format!("{:.1}%", snapshot.cpu_usage));
                    metric_card(
                        ui,
                        "RAM",
                        format!("{} / {} MB", snapshot.ram_used_mb, snapshot.ram_total_mb),
                    );
                    metric_card(
                        ui,
                        "Disk",
                        format!("{} / {}", snapshot.disk_used, snapshot.disk_total),
                    );
                    ui.end_row();
                    metric_card(
                        ui,
                        "Network RX/TX",
                        format!("{} / {}", snapshot.network_rx, snapshot.network_tx),
                    );
                    metric_card(ui, "Uptime", snapshot.uptime.clone());
                    metric_card(ui, "Load Average", snapshot.load_average.clone());
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.label(format!("Last updated: {}", snapshot.timestamp));
        } else {
            ui.label("Connect to a server and refresh to load dashboard metrics.");
        }
    }

    fn draw_network(&mut self, ui: &mut egui::Ui) {
        ui.heading("Network");
        ui.add_space(10.0);

        let Some(snapshot) = &self.snapshot else {
            ui.label("No network data yet.");
            return;
        };

        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("network_table")
                .striped(true)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    ui.strong("Protocol");
                    ui.strong("Local Address");
                    ui.strong("Remote Address");
                    ui.strong("State");
                    ui.strong("Process");
                    ui.end_row();

                    for connection in &snapshot.connections {
                        ui.label(&connection.protocol);
                        ui.label(&connection.local);
                        ui.label(&connection.remote);
                        ui.label(&connection.state);
                        ui.label(&connection.process);
                        ui.end_row();
                    }
                });
        });
    }

    fn draw_services(&mut self, ui: &mut egui::Ui) {
        ui.heading("Services");
        ui.add_space(10.0);

        if let Some(idx) = self.selected_server {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.service_draft);
                if ui.button("Add Service").clicked() {
                    let service = self.service_draft.trim().to_owned();
                    if !service.is_empty() && service_name_is_safe(&service) {
                        if let Some(server) = self.config.servers.get_mut(idx) {
                            server.services.push(service);
                            self.service_draft.clear();
                            self.save_config();
                        }
                    } else {
                        self.error = Some("Service names may only contain letters, numbers, '.', '_', '-', ':', and '@'.".into());
                    }
                }
            });
        }

        ui.separator();
        if let Some(snapshot) = &self.snapshot {
            egui::Grid::new("services_table")
                .striped(true)
                .spacing([18.0, 6.0])
                .show(ui, |ui| {
                    ui.strong("Service");
                    ui.strong("Status");
                    ui.strong("Last Checked");
                    ui.end_row();

                    for service in &snapshot.services {
                        ui.label(&service.name);
                        ui.colored_label(service_color(&service.status), &service.status);
                        ui.label(&service.last_checked);
                        ui.end_row();
                    }
                });
        } else if let Some(server) = self.selected_config() {
            for service in server.services {
                ui.label(service);
            }
        }
    }

    fn draw_logs(&mut self, ui: &mut egui::Ui) {
        ui.heading("Logs");
        ui.add_space(10.0);

        let Some(snapshot) = &self.snapshot else {
            ui.label("No logs yet.");
            return;
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            for log in &snapshot.error_logs {
                ui.colored_label(egui::Color32::from_rgb(224, 92, 92), log);
            }
        });
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.add_space(10.0);

        ui.checkbox(&mut self.auto_refresh, "Auto refresh");
        ui.horizontal(|ui| {
            ui.label("Interval");
            ui.selectable_value(&mut self.auto_refresh_secs, 5, "5s");
            ui.selectable_value(&mut self.auto_refresh_secs, 10, "10s");
            ui.selectable_value(&mut self.auto_refresh_secs, 30, "30s");
        });
        ui.separator();
        ui.label("Private key contents are never stored. ServerScope saves only the configured key path.");
        ui.label("MVP command execution is limited to fixed monitoring commands.");
    }
}

impl eframe::App for ServerScopeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_worker_messages();
        self.maybe_auto_refresh(&ctx);

        egui::Panel::left("sidebar")
            .resizable(false)
            .default_size(180.0)
            .show_inside(ui, |ui| self.draw_sidebar(ui));

        egui::Panel::top("top_bar").show_inside(ui, |ui| self.draw_top_bar(ui));

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(214, 73, 73), error);
                ui.separator();
            }

            match self.view {
                View::Servers => self.draw_servers(ui),
                View::Dashboard => self.draw_dashboard(ui),
                View::Network => self.draw_network(ui),
                View::Services => self.draw_services(ui),
                View::Logs => self.draw_logs(ui),
                View::Settings => self.draw_settings(ui),
            }
        });
    }
}

fn metric_card(ui: &mut egui::Ui, title: &str, value: String) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(220.0, 82.0));
            ui.label(title);
            ui.heading(value);
        });
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("serverscope")
        .join("config.json")
}

fn load_config(path: &Path) -> AppConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

fn save_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let data = serde_json::to_string_pretty(config).map_err(|err| err.to_string())?;
    fs::write(path, data).map_err(|err| err.to_string())
}

fn connect(server: &ServerConfig) -> Result<Session, String> {
    let tcp = TcpStream::connect((&*server.host, server.port))
        .map_err(|err| format!("TCP connection failed: {err}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(12))).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(12))).ok();

    let mut session = Session::new().map_err(|err| format!("SSH session failed: {err}"))?;
    session.set_tcp_stream(tcp);
    session
        .handshake()
        .map_err(|err| format!("SSH handshake failed: {err}"))?;
    session
        .userauth_pubkey_file(
            &server.username,
            None,
            Path::new(&server.private_key_path),
            None,
        )
        .map_err(|err| format!("SSH key authentication failed: {err}"))?;

    if session.authenticated() {
        Ok(session)
    } else {
        Err("SSH authentication failed.".to_owned())
    }
}

fn test_connection(server: &ServerConfig) -> Result<(), String> {
    let session = connect(server)?;
    let _ = run_command(&session, "uptime")?;
    Ok(())
}

fn collect_snapshot(server: &ServerConfig) -> Result<ServerSnapshot, String> {
    let session = connect(server)?;
    let uptime = run_command(&session, "uptime")?;
    let free = run_command(&session, "free -m")?;
    let disk = run_command(&session, "df -h /")?;
    let load = run_command(&session, "cat /proc/loadavg")?;
    let cpu = run_command(&session, "top -bn1 | grep \"Cpu(s)\"")?;
    let ss = run_command(&session, "ss -tunlp")?;
    let logs = run_command(&session, "journalctl -p err -n 50 --no-pager")?;
    let netdev = run_command(&session, "cat /proc/net/dev").unwrap_or_default();
    let checked_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let services = server
        .services
        .iter()
        .map(|service| {
            let status = if service_name_is_safe(service) {
                run_command(&session, &format!("systemctl is-active {service}"))
                    .unwrap_or_else(|err| err)
                    .trim()
                    .to_owned()
            } else {
                "invalid-name".to_owned()
            };

            ServiceStatus {
                name: service.clone(),
                status,
                last_checked: checked_at.clone(),
            }
        })
        .collect();

    let (ram_used_mb, ram_total_mb) = parse_memory(&free);
    let (disk_used, disk_total) = parse_disk(&disk);
    let (network_rx, network_tx) = parse_network_totals(&netdev);

    Ok(ServerSnapshot {
        timestamp: checked_at,
        uptime: uptime.trim().to_owned(),
        cpu_usage: parse_cpu(&cpu),
        ram_used_mb,
        ram_total_mb,
        disk_used,
        disk_total,
        load_average: parse_load(&load),
        network_rx,
        network_tx,
        connections: parse_connections(&ss),
        services,
        error_logs: logs.lines().map(str::to_owned).collect(),
    })
}

fn run_command(session: &Session, command: &str) -> Result<String, String> {
    let mut channel = session.channel_session().map_err(|err| err.to_string())?;
    channel.exec(command).map_err(|err| err.to_string())?;

    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|err| err.to_string())?;
    channel.wait_close().map_err(|err| err.to_string())?;
    Ok(output)
}

fn parse_memory(output: &str) -> (u64, u64) {
    for line in output.lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.first() == Some(&"Mem:") && parts.len() >= 3 {
            let total = parts[1].parse().unwrap_or(0);
            let used = parts[2].parse().unwrap_or(0);
            return (used, total);
        }
    }
    (0, 0)
}

fn parse_disk(output: &str) -> (String, String) {
    output
        .lines()
        .nth(1)
        .and_then(|line| {
            let parts: Vec<_> = line.split_whitespace().collect();
            (parts.len() >= 3).then(|| (parts[2].to_owned(), parts[1].to_owned()))
        })
        .unwrap_or_else(|| ("unknown".to_owned(), "unknown".to_owned()))
}

fn parse_load(output: &str) -> String {
    let parts: Vec<_> = output.split_whitespace().take(3).collect();
    if parts.is_empty() {
        "unknown".to_owned()
    } else {
        parts.join(" / ")
    }
}

fn parse_cpu(output: &str) -> f32 {
    let normalized = output.replace(',', " ");
    let parts: Vec<_> = normalized.split_whitespace().collect();

    for window in parts.windows(2) {
        if window[1] == "id" {
            if let Ok(idle) = window[0].parse::<f32>() {
                return (100.0 - idle).clamp(0.0, 100.0);
            }
        }
    }

    let mut usage = 0.0;
    for window in parts.windows(2) {
        if matches!(window[1], "us" | "sy" | "ni" | "hi" | "si" | "st") {
            usage += window[0].parse::<f32>().unwrap_or(0.0);
        }
    }
    usage.clamp(0.0, 100.0)
}

fn parse_network_totals(output: &str) -> (String, String) {
    let mut rx = 0_u64;
    let mut tx = 0_u64;

    for line in output.lines().skip(2) {
        let Some((iface, data)) = line.split_once(':') else {
            continue;
        };
        if iface.trim() == "lo" {
            continue;
        }

        let parts: Vec<_> = data.split_whitespace().collect();
        if parts.len() >= 16 {
            rx += parts[0].parse::<u64>().unwrap_or(0);
            tx += parts[8].parse::<u64>().unwrap_or(0);
        }
    }

    (format_bytes(rx), format_bytes(tx))
}

fn parse_connections(output: &str) -> Vec<NetworkConnection> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() < 6 {
                return None;
            }

            Some(NetworkConnection {
                protocol: parts[0].to_owned(),
                state: parts[1].to_owned(),
                local: parts[4].to_owned(),
                remote: parts[5].to_owned(),
                process: parts.get(6).unwrap_or(&"").to_string(),
            })
        })
        .collect()
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes = bytes as f64;

    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn service_name_is_safe(service: &str) -> bool {
    !service.is_empty()
        && service
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':' | '@'))
}

fn service_color(status: &str) -> egui::Color32 {
    match status.trim() {
        "active" => egui::Color32::from_rgb(42, 157, 86),
        "inactive" => egui::Color32::from_rgb(132, 140, 152),
        "failed" => egui::Color32::from_rgb(214, 73, 73),
        _ => egui::Color32::from_rgb(214, 154, 58),
    }
}
