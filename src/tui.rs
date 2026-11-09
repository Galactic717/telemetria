use crate::cli_render::{format_duration, make_bar, print_snapshot_report, truncate_str};
use crate::config::{save_config, AppConfig};
use crate::economics::{compute_weighted_24h_kwh_rate_uah, simulate_workload_scenarios};
use crate::models::SystemSnapshot;
use crate::sensors::HardwareMonitor;
use crate::storage::TelemetryStorage;
use crate::tariffs::LiveTariffService;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table, Tabs};
use ratatui::{Frame, Terminal};
use std::collections::VecDeque;
use std::io::{self, IsTerminal};
use std::time::{Duration, Instant};

pub fn run_dashboard(
    mut cfg: AppConfig,
    interval_sec: f64,
    duration_sec: Option<f64>,
    once: bool,
    offline: bool,
    force_refresh: bool,
) -> Result<(), String> {
    let tariff_svc = LiveTariffService::new();
    tariff_svc.refresh_now(force_refresh, offline, cfg.tariff_cache_ttl_sec);

    let bg_tx = if !offline {
        Some(tariff_svc.spawn_background_updater(45))
    } else {
        None
    };

    let mut monitor = HardwareMonitor::new();
    let storage = TelemetryStorage::default();

    // Warm up first sample
    let first_snap = monitor.sample(&cfg, &tariff_svc, true);
    storage.record_snapshot(&first_snap);

    if once || !io::stdout().is_terminal() {
        print_snapshot_report(&first_snap);
        return Ok(());
    }

    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let mut power_history: VecDeque<u64> = VecDeque::with_capacity(80);
    power_history.push_back(first_snap.power.total_wall_watts.round() as u64);

    let mut current_snap = first_snap;
    let mut active_tab: usize = 0;
    let start_instant = Instant::now();
    let mut last_sample_instant = Instant::now();
    let tick_duration = Duration::from_secs_f64(interval_sec.clamp(0.15, 10.0));

    let loop_res: Result<(), String> = (|| {
        loop {
            if let Some(max_dur) = duration_sec {
                if start_instant.elapsed().as_secs_f64() >= max_dur {
                    break;
                }
            }

            let history_vec: Vec<u64> = power_history.iter().copied().collect();
            terminal
                .draw(|f| {
                    draw_tui_frame(
                        f,
                        &current_snap,
                        &cfg,
                        &tariff_svc,
                        &storage,
                        &history_vec,
                        active_tab,
                    )
                })
                .map_err(|e| e.to_string())?;

            let timeout = tick_duration
                .checked_sub(last_sample_instant.elapsed())
                .unwrap_or(Duration::from_millis(20));

            if event::poll(timeout).map_err(|e| e.to_string())? {
                if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            KeyCode::Char('c')
                                if key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                break
                            }
                            KeyCode::Char('1') => active_tab = 0,
                            KeyCode::Char('2') => active_tab = 1,
                            KeyCode::Char('3') => active_tab = 2,
                            KeyCode::Tab => active_tab = (active_tab + 1) % 3,
                            KeyCode::Char('t') => {
                                cfg.cycle_tariff_profile();
                                save_config(&cfg);
                                current_snap = monitor.sample(&cfg, &tariff_svc, true);
                            }
                            KeyCode::Char('m') => {
                                cfg.cycle_workload_profile();
                                save_config(&cfg);
                                current_snap = monitor.sample(&cfg, &tariff_svc, true);
                            }
                            KeyCode::Char('p') => {
                                cfg.cycle_psu_rating();
                                save_config(&cfg);
                                current_snap = monitor.sample(&cfg, &tariff_svc, true);
                            }
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                cfg.rental_revenue_usd_hr =
                                    ((cfg.rental_revenue_usd_hr + 0.02) * 100.0).round() / 100.0;
                                save_config(&cfg);
                                current_snap = monitor.sample(&cfg, &tariff_svc, true);
                            }
                            KeyCode::Char('-') | KeyCode::Char('_') => {
                                cfg.rental_revenue_usd_hr = (((cfg.rental_revenue_usd_hr - 0.02)
                                    .max(0.02))
                                    * 100.0)
                                    .round()
                                    / 100.0;
                                save_config(&cfg);
                                current_snap = monitor.sample(&cfg, &tariff_svc, true);
                            }
                            KeyCode::Char('r') => {
                                if let Some(ref tx) = bg_tx {
                                    let _ = tx.send(());
                                }
                            }
                            KeyCode::Char('x') => {
                                monitor.reset_session();
                                current_snap = monitor.sample(&cfg, &tariff_svc, true);
                            }
                            _ => {}
                        }
                    }
                }
            }

            if last_sample_instant.elapsed() >= tick_duration {
                current_snap = monitor.sample(&cfg, &tariff_svc, true);
                storage.record_snapshot(&current_snap);
                if power_history.len() >= 80 {
                    power_history.pop_front();
                }
                power_history.push_back(current_snap.power.total_wall_watts.round() as u64);
                last_sample_instant = Instant::now();
            }
        }
        Ok(())
    })();

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    loop_res
}

fn draw_tui_frame(
    f: &mut Frame,
    snap: &SystemSnapshot,
    cfg: &AppConfig,
    tariff_svc: &LiveTariffService,
    storage: &TelemetryStorage,
    power_history: &[u64],
    active_tab: usize,
) {
    let root_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(16),
            Constraint::Length(2),
        ])
        .split(f.area());

    draw_header(f, root_chunks[0], snap, active_tab);

    match active_tab {
        0 => draw_tab_dashboard(f, root_chunks[1], snap, power_history),
        1 => draw_tab_mining_simulator(f, root_chunks[1], snap, cfg),
        _ => draw_tab_tariffs_and_history(f, root_chunks[1], snap, cfg, tariff_svc, storage),
    }

    draw_footer(f, root_chunks[2]);
}

fn draw_header(f: &mut Frame, area: Rect, snap: &SystemSnapshot, active_tab: usize) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " ⚡ TELEMETRIA v2.0 (Rust Native Real-Time Engine) ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let live_badge = if snap.fx.is_live {
        Span::styled(
            " ● LIVE NET ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " ○ CACHED ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    };

    let zone_badge = if snap.tariff.is_night_discount_active {
        Span::styled(
            format!(" {} ", snap.tariff.current_zone_label),
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!(" {} ", snap.tariff.current_zone_label),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    };

    let top_line = Line::from(vec![
        live_badge,
        Span::raw(format!(
            " {} ({}) | Плата: {} | Аптайм: {} | Сенсори: {} µs | ",
            snap.hostname,
            snap.os_name,
            snap.motherboard,
            format_duration(snap.uptime_seconds as f64),
            snap.sample_latency_us
        )),
        zone_badge,
        Span::styled(
            format!(
                " USD:{:.2}₴ EUR:{:.2}₴ | BTC:${:.0} RENDER:${:.2}",
                snap.fx.usd_to_uah, snap.fx.eur_to_uah, snap.crypto.btc_usd, snap.crypto.render_usd
            ),
            Style::default().fg(Color::LightMagenta),
        ),
    ]);
    f.render_widget(Paragraph::new(top_line), rows[0]);

    let titles = vec![
        Line::from(" [1] Живий Дашборд ПК та Витрат "),
        Line::from(" [2] Симулятор Оренди Сервера & Майнінгу "),
        Line::from(" [3] Живі Тарифи з Інтернету & Історія Сесій "),
    ];
    let tabs = Tabs::new(titles)
        .select(active_tab)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    f.render_widget(tabs, rows[1]);
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let text = Line::from(vec![
        Span::styled(
            " Гарячі клавіші: ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("[1/2/3/Tab]", Style::default().fg(Color::Yellow)),
        Span::raw(" Вкладки  "),
        Span::styled("[t]", Style::default().fg(Color::Yellow)),
        Span::raw(" Змінити тариф  "),
        Span::styled("[m]", Style::default().fg(Color::Yellow)),
        Span::raw(" Профіль оренди/майнінгу  "),
        Span::styled("[+/-]", Style::default().fg(Color::Yellow)),
        Span::raw(" Ставка $/год  "),
        Span::styled("[p]", Style::default().fg(Color::Yellow)),
        Span::raw(" БЖ 80+  "),
        Span::styled("[r]", Style::default().fg(Color::Yellow)),
        Span::raw(" Оновити з Інтернету  "),
        Span::styled("[x]", Style::default().fg(Color::Yellow)),
        Span::raw(" Скинути сесію  "),
        Span::styled("[q/Esc]", Style::default().fg(Color::Red)),
        Span::raw(" Вихід"),
    ]);
    f.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::TOP)),
        area,
    );
}

fn draw_tab_dashboard(f: &mut Frame, area: Rect, snap: &SystemSnapshot, power_history: &[u64]) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Min(6),
        ])
        .split(area);

    // Row 1: CPU + GPU
    let hw_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);

    let c = &snap.cpu;
    let core_preview = c
        .per_core_percent
        .iter()
        .take(8)
        .enumerate()
        .map(|(i, v)| format!("C{}:{:2.0}%", i, v))
        .collect::<Vec<_>>()
        .join(" ");
    let cpu_lines = vec![
        Line::from(vec![
            Span::styled("Процесор:        ", Style::default().fg(Color::Gray)),
            Span::styled(
                &c.name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Навантаження:    ", Style::default().fg(Color::Gray)),
            Span::styled(make_bar(c.usage_percent, 20), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("Температура:     ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.1} °C  [{}]", c.temperature_c, c.temp_source),
                Style::default().fg(if c.temperature_c >= 78.0 {
                    Color::Red
                } else {
                    Color::LightGreen
                }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Споживання CPU:  ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "{:.1} W / {:.0} W TDP ({:.0}%)",
                    c.power_watts,
                    c.tdp_watts,
                    (c.power_watts / c.tdp_watts.max(1.0)) * 100.0
                ),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Частота / Ядра:  ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "{:.0} / {:.0} MHz ({}C/{}T)",
                    c.frequency_current_mhz,
                    c.frequency_max_mhz,
                    c.physical_cores,
                    c.logical_cores
                ),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("Потоки:          ", Style::default().fg(Color::Gray)),
            Span::raw(core_preview),
        ]),
    ];
    f.render_widget(
        Paragraph::new(cpu_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" 🖥  ПРОЦЕСОР (CPU TELEMETRY) "),
        ),
        hw_cols[0],
    );

    let gpu_lines = if let Some(g) = snap.gpus.first() {
        let vram_pct = (g.vram_used_mb / g.vram_total_mb.max(1.0)) * 100.0;
        let fan_str = if g.fan_speed_percent > 0.0 {
            format!("{:.0}%", g.fan_speed_percent)
        } else {
            "0% (Zero-RPM Fan Stop)".to_string()
        };
        let energy_str = g
            .energy_total_kwh
            .map(|e| format!(" | Лічильник: {:.4} кВт·год", e))
            .unwrap_or_default();
        vec![
            Line::from(vec![
                Span::styled("Відеокарта:      ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!(
                        "{} (Gen{} x{}, {})",
                        g.name, g.pcie_gen, g.pcie_width, g.pstate
                    ),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Ядро GPU:        ", Style::default().fg(Color::Gray)),
                Span::styled(
                    make_bar(g.usage_percent, 20),
                    Style::default().fg(Color::LightMagenta),
                ),
            ]),
            Line::from(vec![
                Span::styled("Пам'ять VRAM:    ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!(
                        "{:.0} / {:.0} MiB ({:.1}%) | PCIe TX/RX: {:.1}/{:.1} MB/s",
                        g.vram_used_mb, g.vram_total_mb, vram_pct, g.pcie_tx_mbs, g.pcie_rx_mbs
                    ),
                    Style::default().fg(Color::LightCyan),
                ),
            ]),
            Line::from(vec![
                Span::styled("Темп. / Кулер:   ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{:.1} °C  |  Fan: {}", g.temperature_c, fan_str),
                    Style::default().fg(Color::LightGreen),
                ),
            ]),
            Line::from(vec![
                Span::styled("Споживання GPU:  ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!(
                        "{:.2} W / {:.0} W Limit{}",
                        g.power_watts, g.power_limit_watts, energy_str
                    ),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Частоти GPU:     ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!(
                        "Core: {:.0} MHz (Boost {:.0}) | Mem: {:.0} MHz",
                        g.clock_core_mhz, g.clock_max_core_mhz, g.clock_mem_mhz
                    ),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
        ]
    } else {
        vec![Line::from("Дискретна відеокарта не виявлена")]
    };
    f.render_widget(
        Paragraph::new(gpu_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightMagenta))
                .title(" 🎮 ВІДЕОКАРТА (NVML Native FFI) "),
        ),
        hw_cols[1],
    );

    // Row 2: System I/O + Power Sparkline
    let io_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(rows[1]);

    let r = &snap.ram;
    let d = &snap.disk;
    let n = &snap.network;
    let parts_str = d
        .partitions
        .iter()
        .take(2)
        .map(|p| format!("{} {:.0}/{:.0}GB ({:.0}%)", p.mountpoint, p.used_gb, p.total_gb, p.usage_percent))
        .collect::<Vec<_>>()
        .join(", ");
    let sys_lines = vec![
        Line::from(vec![
            Span::styled(
                format!("ОЗП ({}x {}-{}): ", r.dimm_count, r.memory_type, r.speed_mts),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!(
                    "{:.1} / {:.1} GB ({:.1}%) | {:.1} W [{}]",
                    r.used_gb, r.total_gb, r.usage_percent, r.power_watts, r.part_number
                ),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("Диск ({}): ", d.model_name),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!(
                    "{} [{}] | R: {:.2} MB/s, W: {:.2} MB/s | {:.1} W ({})",
                    d.media_type,
                    d.health_status,
                    d.read_speed_mbs,
                    d.write_speed_mbs,
                    d.power_watts,
                    parts_str
                ),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("Мережа ({}): ", n.primary_interface),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!(
                    "↓ {:.3} MB/s | ↑ {:.3} MB/s  (За сесію: ↓{:.2} GB / ↑{:.2} GB)",
                    n.download_speed_mbs, n.upload_speed_mbs, n.total_recv_gb, n.total_sent_gb
                ),
                Style::default().fg(Color::LightGreen),
            ),
        ]),
    ];
    f.render_widget(
        Paragraph::new(sys_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue))
                .title(" 💾 ПАМ'ЯТЬ, ДИСК ТА МЕРЕЖА (RAM · NVMe · NET) "),
        ),
        io_cols[0],
    );

    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(format!(
                    " 📈 ДИНАМІКА СПОЖИВАННЯ З РОЗЕТКИ: {:.1} W (Пік {:.1} W) ",
                    snap.power.total_wall_watts, snap.session.peak_wall_watts
                )),
        )
        .data(power_history)
        .style(Style::default().fg(Color::Yellow));
    f.render_widget(sparkline, io_cols[1]);

    // Row 3: Electricity Cost Meter + Mining/Server Rental Profitability
    let fin_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[2]);

    let p = &snap.power;
    let t = &snap.tariff;
    let s = &snap.session;
    let weighted_24h = compute_weighted_24h_kwh_rate_uah(t);
    let cost_hr_uah = p.total_wall_kw * t.current_rate_uah_kwh;
    let cost_hr_usd = cost_hr_uah / snap.fx.usd_to_uah.max(0.01);
    let cost_day_uah = p.total_wall_kw * 24.0 * weighted_24h;
    let cost_day_usd = cost_day_uah / snap.fx.usd_to_uah.max(0.01);
    let cost_month_uah = cost_day_uah * 30.0;
    let cost_month_usd = cost_day_usd * 30.0;

    let elec_lines = vec![
        Line::from(vec![
            Span::styled("З розетки (AC):   ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "⚡ {:.2} W ({:.3} кВт) | БЖ {}: ККД {:.0}% (+{:.1}W тепла)",
                    p.total_wall_watts,
                    p.total_wall_kw,
                    p.psu_rating,
                    p.psu_efficiency * 100.0,
                    p.psu_loss_watts
                ),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Баланс (DC):      ", Style::default().fg(Color::Gray)),
            Span::raw(format!(
                "CPU {:.1}W + GPU {:.1}W + RAM {:.1}W + NVMe {:.1}W + Плата {:.1}W = {:.1}W",
                p.cpu_watts,
                p.gpu_watts,
                p.ram_watts,
                p.disk_watts,
                p.motherboard_fans_watts,
                p.total_dc_watts
            )),
        ]),
        Line::from(vec![
            Span::styled("Активний тариф:   ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "{} → {:.2} ₴/кВт (День {:.2} ₴ / Ніч {:.2} ₴)",
                    t.profile_id, t.current_rate_uah_kwh, t.day_rate_uah_kwh, t.night_rate_uah_kwh
                ),
                Style::default().fg(Color::LightGreen),
            ),
        ]),
        Line::from(vec![
            Span::styled("Витрати (Прогноз):", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "Година: {:.2} ₴ (${:.3}) | Доба: {:.2} ₴ (${:.2}) | Міс: {:.0} ₴ (${:.1})",
                    cost_hr_uah, cost_hr_usd, cost_day_uah, cost_day_usd, cost_month_uah, cost_month_usd
                ),
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("З'їдено ({}): ", format_duration(s.elapsed_seconds)),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!(
                    "{:.4} Вт·год ({:.6} кВт·год) → {:.4} ₴ (${:.5} / €{:.5})",
                    s.energy_wh, s.energy_kwh, s.cost_uah, s.cost_usd, s.cost_eur
                ),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Джерело тарифу:   ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{} (оновлено {})", t.source, t.updated_at),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];
    f.render_widget(
        Paragraph::new(elec_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" 🔌 ЕЛЕКТРОЕНЕРГІЯ ТА ЛІЧИЛЬНИК КОШТІВ (Скільки з'їв ПК) "),
        ),
        fin_cols[0],
    );

    let m = &snap.mining;
    let prof_color = if m.is_profitable_now {
        Color::LightGreen
    } else {
        Color::LightRed
    };
    let mining_lines = vec![
        Line::from(vec![
            Span::styled("Профіль сервера:  ", Style::default().fg(Color::Gray)),
            Span::styled(
                &m.workload_profile,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Валовий дохід:    ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "+{:.2} ₴/год (+${:.3}/год)  |  Доба: +{:.1} ₴",
                    m.gross_revenue_uah_hr,
                    m.gross_revenue_usd_hr,
                    m.gross_revenue_uah_hr * 24.0
                ),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("Чистий прибуток:  ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "{:+.2} ₴/год (${:+.3}/год)  |  Доба (Д+Н): {:+.1} ₴ (${:+.2})",
                    m.net_profit_uah_hr,
                    m.net_profit_usd_hr,
                    m.net_profit_uah_day_24h,
                    m.net_profit_usd_day_24h
                ),
                Style::default().fg(prof_color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Прогноз за місяць:", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "24/7: {:+.0} ₴ (${:+.1})  |  Тільки ніч (8г 23-07): +{:0.0} ₴/міс",
                    m.net_profit_uah_month_30d,
                    m.net_profit_usd_month_30d,
                    m.night_only_profit_uah_8h * 30.0
                ),
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Беззбитковість:   ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "Електрика: {:.1}% доходу | Мін. оренда: ${:.3}/год | Макс. тариф: {:.1} ₴",
                    m.electricity_share_percent,
                    m.break_even_rental_usd_hr,
                    m.break_even_tariff_uah_kwh
                ),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("Статус / Сесія:   ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "[{}] | За сесію чистими: {:+.4} ₴ (${:+.5})",
                    m.efficiency_label, s.net_profit_uah, s.net_profit_usd
                ),
                Style::default().fg(prof_color),
            ),
        ]),
    ];
    f.render_widget(
        Paragraph::new(mining_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(" 💰 РЕЖИМ МАЙНІНГУ ТА ОРЕНДИ СЕРВЕРА (PROFITABILITY & ROI) "),
        ),
        fin_cols[1],
    );

    // Row 4: Top Processes Table
    let header_cells = [
        "PID",
        "Процес (Process)",
        "CPU %",
        "RAM (MB)",
        "Споживання (W)",
        "Вартість (₴/доба)",
    ]
    .iter()
    .map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    });
    let header = Row::new(header_cells).height(1);
    let proc_rows = snap.top_processes.iter().map(|pr| {
        Row::new(vec![
            Cell::from(pr.pid.to_string()),
            Cell::from(truncate_str(&pr.name, 36)),
            Cell::from(format!("{:.1}%", pr.cpu_percent)),
            Cell::from(format!("{:.1} MB", pr.memory_mb)),
            Cell::from(format!("{:.2} W", pr.estimated_watts))
                .style(Style::default().fg(Color::Yellow)),
            Cell::from(format!("{:.3} ₴/доба", pr.cost_uah_per_day))
                .style(Style::default().fg(Color::LightRed)),
        ])
    });
    let proc_table = Table::new(
        proc_rows,
        [
            Constraint::Length(8),
            Constraint::Min(28),
            Constraint::Length(10),
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Length(18),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White))
            .title(" 🔥 ТОП ПРОЦЕСІВ ЗА ЕНЕРГОСПОЖИВАННЯМ (PER-PROCESS WATTAGE) "),
    );
    f.render_widget(proc_table, rows[3]);
}

fn draw_tab_mining_simulator(
    f: &mut Frame,
    area: Rect,
    snap: &SystemSnapshot,
    cfg: &AppConfig,
) {
    let gpu_tdp = snap
        .gpus
        .first()
        .map(|g| g.power_limit_watts)
        .unwrap_or(250.0);
    let scenarios = simulate_workload_scenarios(
        snap.power.total_wall_watts,
        snap.cpu.tdp_watts,
        gpu_tdp,
        snap.power.psu_efficiency,
        &snap.tariff,
        &snap.fx,
        cfg.rental_revenue_usd_hr,
    );

    let header = Row::new(vec![
        "Сценарій навантаження ПК",
        "З розетки (W)",
        "кВт·год/доба",
        "Електрика/доба",
        "Дохід/доба",
        "Чистий/доба (₴)",
        "Чистий/міс 24/7",
        "Тільки Ніч (8г/міс)",
        "Статус ROI",
    ])
    .style(
        Style::default()
            .fg(Color::LightGreen)
            .add_modifier(Modifier::BOLD),
    );

    let rows = scenarios.iter().map(|r| {
        let col = if r.net_day_uah >= 0.0 {
            Color::LightGreen
        } else {
            Color::LightRed
        };
        Row::new(vec![
            Cell::from(r.scenario_name.clone()),
            Cell::from(format!("{:.1} W", r.wall_watts)).style(Style::default().fg(Color::Yellow)),
            Cell::from(format!("{:.2}", r.kwh_per_day)),
            Cell::from(format!("-{:.2} ₴", r.cost_day_uah)).style(Style::default().fg(Color::Red)),
            Cell::from(format!("+{:.2} ₴", r.gross_day_uah))
                .style(Style::default().fg(Color::Cyan)),
            Cell::from(format!("{:+.2} ₴", r.net_day_uah)).style(Style::default().fg(col)),
            Cell::from(format!("{:+.0} ₴ (${:+.1})", r.net_month_uah, r.net_month_usd))
                .style(Style::default().fg(col).add_modifier(Modifier::BOLD)),
            Cell::from(format!("+{:.0} ₴", r.night_8h_net_month_uah))
                .style(Style::default().fg(Color::LightCyan)),
            Cell::from(r.roi_status.clone()),
        ])
        .height(2)
    });

    let tbl = Table::new(
        rows,
        [
            Constraint::Min(38),
            Constraint::Length(14),
            Constraint::Length(13),
            Constraint::Length(15),
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Length(20),
            Constraint::Length(18),
            Constraint::Length(16),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightGreen))
            .title(format!(
                " 📊 СИМУЛЯЦІЯ ОРЕНДИ СЕРВЕРА ТА МАЙНІНГУ | Ставка: ${:.2}/год ({:.2} ₴/год) — натисніть [+/-] або [m] ",
                cfg.rental_revenue_usd_hr,
                cfg.rental_revenue_usd_hr * snap.fx.usd_to_uah
            )),
    );
    f.render_widget(tbl, area);
}

fn draw_tab_tariffs_and_history(
    f: &mut Frame,
    area: Rect,
    snap: &SystemSnapshot,
    cfg: &AppConfig,
    tariff_svc: &LiveTariffService,
    storage: &TelemetryStorage,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let profiles = tariff_svc.list_all_profiles(cfg, None);
    let t_header = Row::new(vec![
        "ID профілю",
        "Назва тарифу",
        "Зараз (₴/кВт)",
        "Зараз ($/кВт)",
        "День (₴)",
        "Ніч 23-07 (₴)",
        "Активна зона",
        "Джерело",
    ])
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let t_rows = profiles.iter().map(|p| {
        let is_active = p.profile_id == cfg.tariff_profile;
        let style = if is_active {
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_active { "▶ " } else { "  " };
        Row::new(vec![
            Cell::from(format!("{}{}", prefix, p.profile_id)),
            Cell::from(truncate_str(&p.name, 46)),
            Cell::from(format!("{:.3} ₴", p.current_rate_uah_kwh)),
            Cell::from(format!("${:.4}", p.rate_in_usd(&snap.fx))),
            Cell::from(format!("{:.2} ₴", p.day_rate_uah_kwh)),
            Cell::from(format!("{:.3} ₴", p.night_rate_uah_kwh)),
            Cell::from(truncate_str(&p.current_zone_label, 24)),
            Cell::from(truncate_str(&p.source, 24)),
        ])
        .style(style)
    });

    let t_table = Table::new(
        t_rows,
        [
            Constraint::Length(16),
            Constraint::Min(36),
            Constraint::Length(14),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(14),
            Constraint::Length(24),
            Constraint::Length(24),
        ],
    )
    .header(t_header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" 🌐 ДОСТУПНІ ТАРИФИ З ІНТЕРНЕТУ (натисніть [t] для перемикання, [r] для оновлення) "),
    );
    f.render_widget(t_table, chunks[0]);

    let summary = storage.get_lifetime_summary();
    let sessions = storage.list_sessions(8);
    let h_header = Row::new(vec![
        "ID сесії",
        "Початок",
        "Тривалість",
        "Сер./Пік (W)",
        "Енергія (Wh)",
        "Витрачено (₴)",
        "Чистий прибуток (₴)",
        "Тариф",
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    let h_rows = sessions.iter().map(|s| {
        Row::new(vec![
            Cell::from(s.session_id.clone()),
            Cell::from(s.started_at.clone()),
            Cell::from(format_duration(s.elapsed_seconds)),
            Cell::from(format!("{:.1} / {:.1} W", s.avg_wall_watts, s.peak_wall_watts)),
            Cell::from(format!("{:.3} Wh", s.energy_wh)),
            Cell::from(format!("{:.4} ₴", s.cost_uah)),
            Cell::from(format!("{:+.4} ₴", s.net_profit_uah)),
            Cell::from(s.tariff_profile.clone()),
        ])
    });
    let h_table = Table::new(
        h_rows,
        [
            Constraint::Length(14),
            Constraint::Length(20),
            Constraint::Length(12),
            Constraint::Length(16),
            Constraint::Length(14),
            Constraint::Length(14),
            Constraint::Length(18),
            Constraint::Min(14),
        ],
    )
    .header(h_header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(format!(
                " 📚 ІСТОРІЯ СЕСІЙ (Всього: {} сесій | Спожито: {:.3} Wh | Витрачено: {:.4} ₴ | Чистими: {:+.4} ₴) ",
                summary.total_sessions,
                summary.total_energy_wh,
                summary.total_cost_uah,
                summary.total_net_uah
            )),
    );
    f.render_widget(h_table, chunks[1]);
}
