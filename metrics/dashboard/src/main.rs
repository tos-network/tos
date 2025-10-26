use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Sparkline},
    Terminal,
};
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tos_perf_monitor::{MetricsSnapshot, PerformanceMonitor};

const UPDATE_INTERVAL: Duration = Duration::from_secs(1);
const HISTORY_SIZE: usize = 60; // Keep 60 seconds of history

struct App {
    monitor: Arc<PerformanceMonitor>,
    current_snapshot: Option<MetricsSnapshot>,
    // History for sparklines
    cpu_history: Vec<u64>,
    tps_history: Vec<u64>,
    mempool_history: Vec<u64>,
    should_quit: bool,
}

impl App {
    fn new(monitor: Arc<PerformanceMonitor>) -> Self {
        Self {
            monitor,
            current_snapshot: None,
            cpu_history: Vec::with_capacity(HISTORY_SIZE),
            tps_history: Vec::with_capacity(HISTORY_SIZE),
            mempool_history: Vec::with_capacity(HISTORY_SIZE),
            should_quit: false,
        }
    }

    fn update(&mut self) {
        if let Ok(snapshot) = self.monitor.snapshot() {
            // Update history
            self.cpu_history.push(snapshot.cpu_usage_percent as u64);
            if self.cpu_history.len() > HISTORY_SIZE {
                self.cpu_history.remove(0);
            }

            self.tps_history.push(snapshot.confirmed_tps as u64);
            if self.tps_history.len() > HISTORY_SIZE {
                self.tps_history.remove(0);
            }

            self.mempool_history.push(snapshot.mempool_size as u64);
            if self.mempool_history.len() > HISTORY_SIZE {
                self.mempool_history.remove(0);
            }

            self.current_snapshot = Some(snapshot);
        }
    }

    fn quit(&mut self) {
        self.should_quit = true;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let monitor = Arc::new(PerformanceMonitor::new());
    let mut app = App::new(monitor);

    // Run app
    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {:?}", err);
    }

    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    let mut last_update = std::time::Instant::now();

    loop {
        // Update metrics if interval elapsed
        if last_update.elapsed() >= UPDATE_INTERVAL {
            app.update();
            last_update = std::time::Instant::now();
        }

        terminal.draw(|f| ui(f, app))?;

        // Handle input with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => app.quit(),
                    _ => {}
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(10), // System metrics
            Constraint::Length(8),  // TOS metrics
            Constraint::Min(0),     // Charts
        ])
        .split(f.area());

    // Title
    let title = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "TOS Performance Monitor",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![Span::styled(
            "Press 'q' or ESC to quit",
            Style::default().fg(Color::DarkGray),
        )]),
    ])
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    if let Some(snapshot) = &app.current_snapshot {
        // System metrics
        render_system_metrics(f, chunks[1], snapshot);

        // TOS metrics
        render_tos_metrics(f, chunks[2], snapshot);

        // Charts
        render_charts(f, chunks[3], app);
    } else {
        let loading = Paragraph::new("Loading metrics...")
            .block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(loading, chunks[1]);
    }
}

fn render_system_metrics(f: &mut ratatui::Frame, area: Rect, snapshot: &MetricsSnapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

    // CPU usage
    let cpu_percent = (snapshot.cpu_usage_percent / 100.0).min(1.0);
    let cpu_gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("CPU Usage"))
        .gauge_style(Style::default().fg(Color::Yellow))
        .ratio(cpu_percent)
        .label(format!("{:.1}%", snapshot.cpu_usage_percent));
    f.render_widget(cpu_gauge, chunks[0]);

    // Memory usage
    let memory_mb = snapshot.resident_set_size as f64 / (1024.0 * 1024.0);
    let memory_text = Paragraph::new(format!(
        "RSS: {:.2} MB  |  Virtual: {:.2} MB  |  FDs: {}",
        memory_mb,
        snapshot.virtual_memory_size as f64 / (1024.0 * 1024.0),
        snapshot.fd_count
    ))
    .block(Block::default().borders(Borders::ALL).title("Memory"));
    f.render_widget(memory_text, chunks[1]);

    // Disk I/O
    let disk_text = Paragraph::new(format!(
        "Read: {:.2} MB/s  |  Write: {:.2} MB/s",
        snapshot.disk_read_per_sec / (1024.0 * 1024.0),
        snapshot.disk_write_per_sec / (1024.0 * 1024.0)
    ))
    .block(Block::default().borders(Borders::ALL).title("Disk I/O"));
    f.render_widget(disk_text, chunks[2]);
}

fn render_tos_metrics(f: &mut ratatui::Frame, area: Rect, snapshot: &MetricsSnapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(4)])
        .split(area);

    // TPS gauge
    let tps_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Confirmed TPS"),
        )
        .gauge_style(Style::default().fg(Color::Green))
        .ratio((snapshot.confirmed_tps / 1000.0).min(1.0))
        .label(format!("{:.2} TPS", snapshot.confirmed_tps));
    f.render_widget(tps_gauge, chunks[0]);

    // Blockchain info
    let blockchain_text = Paragraph::new(vec![
        Line::from(format!("Block Height: {}", snapshot.current_block_height)),
        Line::from(format!(
            "Mempool: {} txs  |  Pending TPS: {:.2}",
            snapshot.mempool_size, snapshot.pending_tps
        )),
        Line::from(format!(
            "Avg Confirmation: {:.2} ms",
            snapshot.avg_confirmation_time_ms
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("Blockchain"));
    f.render_widget(blockchain_text, chunks[1]);
}

fn render_charts(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(area);

    // CPU history
    if !app.cpu_history.is_empty() {
        let cpu_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("CPU History (60s)"),
            )
            .data(&app.cpu_history)
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(cpu_sparkline, chunks[0]);
    }

    // TPS history
    if !app.tps_history.is_empty() {
        let tps_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("TPS History (60s)"),
            )
            .data(&app.tps_history)
            .style(Style::default().fg(Color::Green));
        f.render_widget(tps_sparkline, chunks[1]);
    }

    // Mempool history
    if !app.mempool_history.is_empty() {
        let mempool_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Mempool Size History (60s)"),
            )
            .data(&app.mempool_history)
            .style(Style::default().fg(Color::Cyan));
        f.render_widget(mempool_sparkline, chunks[2]);
    }
}
