use crate::cli_render::{
    print_config_report, print_history_report, print_mining_report, print_snapshot_report,
    print_tariffs_report,
};
use crate::config::{
    load_config, lookup_psu_efficiency, save_config, AppConfig, TARIFF_PROFILE_IDS,
    WORKLOAD_PRESETS,
};
use crate::economics::simulate_workload_scenarios;
use crate::sensors::HardwareMonitor;
use crate::storage::TelemetryStorage;
use crate::tariffs::LiveTariffService;
use crate::tui::run_dashboard;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(
    name = "telemetria",
    version = "2.0.0",
    about = "⚡ TELEMETRIA v2.0 (Rust Native) — Real-time PC Hardware Telemetry, AC Wall Power, Live Internet Electricity Tariffs & Server Rental / Mining Profitability CLI & TUI"
)]
pub struct Cli {
    /// Вивести один знімок дашборду та завершити роботу
    #[arg(long, global = true)]
    pub once: bool,

    /// Працювати без мережевих запитів (використовувати локальний кеш)
    #[arg(long, global = true)]
    pub offline: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Запустити інтерактивний Live TUI дашборд у реальному часі (60 FPS, гарячі клавіші)
    Dashboard {
        /// Інтервал оновлення телеметрії в секундах
        #[arg(short, long, default_value_t = 1.0)]
        interval: f64,

        /// Тривалість роботи в секундах (за замовчуванням безперервно)
        #[arg(short, long)]
        duration: Option<f64>,

        /// Зробити один кадр дашборду та вийти
        #[arg(long)]
        once: bool,

        /// Не виконувати мережеві запити
        #[arg(long)]
        offline: bool,

        /// Примусово оновити тарифи та курси валют з інтернету
        #[arg(short, long)]
        refresh: bool,
    },

    /// Миттєвий знімок усіх комплектуючих ПК, споживання (W), вартості електрики та прибутку
    Status {
        /// Вивести повну телеметрію у форматі JSON
        #[arg(short, long)]
        json: bool,

        /// Працювати офлайн з локальним кешем
        #[arg(long)]
        offline: bool,

        /// Примусово оновити ціни на електроенергію з інтернету
        #[arg(short, long)]
        refresh: bool,
    },

    /// Переглянути та оновити з інтернету поточні ціни на електроенергію, нічний тариф та курси НБУ
    Tariffs {
        /// Встановити активний тарифний профіль (ua_two_zone, ua_single, ua_three_zone, ua_heating, ua_commercial, eu_spot_pl, eu_spot_de, custom)
        #[arg(short, long = "set")]
        set_profile: Option<String>,

        /// Встановити власну ціну за 1 кВт·год у грн (вмикає профіль custom)
        #[arg(long = "custom-rate")]
        custom_rate: Option<f64>,

        /// Примусово оновити тарифи та курси з інтернету
        #[arg(short, long, default_value_t = true)]
        refresh: bool,

        /// Працювати офлайн з локальним кешем
        #[arg(long)]
        offline: bool,

        /// Вивести тарифи у форматі JSON
        #[arg(short, long)]
        json: bool,
    },

    /// Режим майнінгу та здачі ПК в оренду як сервера: розрахунок витрат електрики, ROI та чистого прибутку
    Mining {
        /// Встановити очікуваний дохід від оренди сервера або майнінгу в USD за годину (напр. 0.42)
        #[arg(long = "rate-usd")]
        rate_usd: Option<f64>,

        /// Встановити очікуваний дохід у грн за годину
        #[arg(long = "rate-uah")]
        rate_uah: Option<f64>,

        /// Обрати профіль роботи (vastai_rtx5080, render_network, salad_container, nicehash_mining, vps_dedicated, custom)
        #[arg(short, long)]
        profile: Option<String>,

        /// Працювати офлайн
        #[arg(long)]
        offline: bool,

        /// Вивести розрахунок у форматі JSON
        #[arg(short, long)]
        json: bool,
    },

    /// Історія роботи ПК: скільки кВт·год та гривень з'їв ПК за всі сесії та скільки заробив
    History {
        /// Кількість останніх сесій для відображення
        #[arg(short = 'n', long, default_value_t = 15)]
        limit: usize,

        /// Вивести історію у форматі JSON
        #[arg(short, long)]
        json: bool,
    },

    /// Експортувати фінансовий та енергетичний звіт у CSV або JSON
    Export {
        /// Формат експорту: csv або json
        #[arg(short, long, default_value = "csv")]
        format: String,

        /// Шлях до файлу звіту
        #[arg(short, long, default_value = "telemetria_report.csv")]
        output: PathBuf,
    },

    /// Переглянути або змінити конфігурацію системи (БЖ 80 PLUS, базове споживання, тарифи)
    Config {
        /// Сертифікат ККД блоку живлення (80+ Titanium, 80+ Platinum, 80+ Gold, 80+ Silver, 80+ Bronze, 80+ White)
        #[arg(long)]
        psu: Option<String>,

        /// Базове споживання материнської плати та вентиляторів (W)
        #[arg(long = "mobo-watts")]
        mobo_watts: Option<f64>,

        /// Скинути налаштування до стандартних
        #[arg(long)]
        reset: bool,
    },

    /// Запустити тест продуктивності сенсорів у реальному часі (вимірювання затримки NVML / CPU / RAM / I/O у мікросекундах)
    Benchmark {
        /// Кількість ітерацій замірів у реальному часі
        #[arg(short, long, default_value_t = 20)]
        iterations: usize,
    },
}

pub fn run_cli() {
    let cli = Cli::parse();
    let mut cfg = load_config();

    match cli.command {
        None => {
            if let Err(e) = run_dashboard(cfg, 1.0, None, cli.once, cli.offline, false) {
                eprintln!("Помилка роботи дашборду: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Dashboard {
            interval,
            duration,
            once,
            offline,
            refresh,
        }) => {
            if let Err(e) = run_dashboard(
                cfg,
                interval,
                duration,
                once || cli.once,
                offline || cli.offline,
                refresh,
            ) {
                eprintln!("Помилка роботи дашборду: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Status {
            json,
            offline,
            refresh,
        }) => {
            let is_offline = offline || cli.offline;
            let svc = LiveTariffService::new();
            svc.refresh_now(refresh, is_offline, cfg.tariff_cache_ttl_sec);
            let mut monitor = HardwareMonitor::new();
            thread::sleep(Duration::from_millis(80));
            let snap = monitor.sample(&cfg, &svc, true);
            TelemetryStorage::default().record_snapshot(&snap);

            if json {
                if let Ok(out) = serde_json::to_string_pretty(&snap) {
                    println!("{}", out);
                }
            } else {
                print_snapshot_report(&snap);
            }
        }
        Some(Commands::Tariffs {
            set_profile,
            custom_rate,
            refresh,
            offline,
            json,
        }) => {
            let is_offline = offline || cli.offline;
            let mut changed = false;
            if let Some(rate) = custom_rate {
                if rate > 0.0 {
                    cfg.custom_tariff_uah_kwh = (rate * 1000.0).round() / 1000.0;
                    cfg.tariff_profile = "custom".to_string();
                    changed = true;
                }
            }
            if let Some(ref prof) = set_profile {
                if !TARIFF_PROFILE_IDS.iter().any(|&p| p == prof) {
                    eprintln!(
                        "Помилка: Невідомий тариф '{}'. Доступні: {}",
                        prof,
                        TARIFF_PROFILE_IDS.join(", ")
                    );
                    std::process::exit(1);
                }
                cfg.tariff_profile = prof.clone();
                changed = true;
            }
            if changed {
                save_config(&cfg);
            }

            let svc = LiveTariffService::new();
            let market = svc.refresh_now(refresh && !is_offline, is_offline, cfg.tariff_cache_ttl_sec);
            let profiles = svc.list_all_profiles(&cfg, None);

            if json {
                let payload = serde_json::json!({
                    "active_profile": cfg.tariff_profile,
                    "exchange_rates": market.fx,
                    "crypto_market": market.crypto,
                    "profiles": profiles,
                });
                if let Ok(out) = serde_json::to_string_pretty(&payload) {
                    println!("{}", out);
                }
            } else {
                if changed {
                    println!("✓ Тарифний профіль успішно збережено: {}", cfg.tariff_profile);
                }
                print_tariffs_report(&profiles, &cfg.tariff_profile, &market.fx, &market.crypto);
            }
        }
        Some(Commands::Mining {
            rate_usd,
            rate_uah,
            profile,
            offline,
            json,
        }) => {
            let is_offline = offline || cli.offline;
            let svc = LiveTariffService::new();
            let market = svc.refresh_now(false, is_offline, cfg.tariff_cache_ttl_sec);
            let mut changed = false;

            if let Some(ref p) = profile {
                if let Some(preset) = WORKLOAD_PRESETS.iter().find(|w| w.id == p) {
                    cfg.workload_profile = preset.id.to_string();
                    if rate_usd.is_none() && rate_uah.is_none() {
                        cfg.rental_revenue_usd_hr = preset.default_revenue_usd_hr;
                    }
                    changed = true;
                } else {
                    let available: Vec<&str> = WORKLOAD_PRESETS.iter().map(|w| w.id).collect();
                    eprintln!(
                        "Помилка: Невідомий профіль '{}'. Доступні: {}",
                        p,
                        available.join(", ")
                    );
                    std::process::exit(1);
                }
            }

            if let Some(usd) = rate_usd {
                if usd >= 0.0 {
                    cfg.rental_revenue_usd_hr = (usd * 10000.0).round() / 10000.0;
                    changed = true;
                }
            } else if let Some(uah) = rate_uah {
                if uah >= 0.0 {
                    cfg.rental_revenue_usd_hr =
                        ((uah / market.fx.usd_to_uah.max(0.01)) * 10000.0).round() / 10000.0;
                    changed = true;
                }
            }

            if changed {
                save_config(&cfg);
            }

            let mut monitor = HardwareMonitor::new();
            thread::sleep(Duration::from_millis(60));
            let snap = monitor.sample(&cfg, &svc, false);
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

            if json {
                let payload = serde_json::json!({
                    "mining_metrics": snap.mining,
                    "power": snap.power,
                    "tariff": snap.tariff,
                    "scenarios": scenarios,
                });
                if let Ok(out) = serde_json::to_string_pretty(&payload) {
                    println!("{}", out);
                }
            } else {
                if changed {
                    println!(
                        "✓ Налаштування оренди/майнінгу збережено: профіль={}, ставка=${:.3}/год ({:.2} ₴/год)",
                        cfg.workload_profile,
                        cfg.rental_revenue_usd_hr,
                        cfg.rental_revenue_usd_hr * market.fx.usd_to_uah
                    );
                }
                print_mining_report(&snap, &scenarios);
            }
        }
        Some(Commands::History { limit, json }) => {
            let storage = TelemetryStorage::default();
            let summary = storage.get_lifetime_summary();
            let sessions = storage.list_sessions(limit);
            if json {
                let payload = serde_json::json!({
                    "summary": summary,
                    "sessions": sessions,
                });
                if let Ok(out) = serde_json::to_string_pretty(&payload) {
                    println!("{}", out);
                }
            } else {
                print_history_report(&summary, &sessions);
            }
        }
        Some(Commands::Export { format, output }) => {
            let storage = TelemetryStorage::default();
            let fmt = format.trim().to_lowercase();
            let res = if fmt == "json" {
                let out_path = if output.extension().and_then(|e| e.to_str()) != Some("json") {
                    output.with_extension("json")
                } else {
                    output
                };
                storage.export_sessions_json(&out_path)
            } else {
                storage.export_sessions_csv(&output)
            };
            match res {
                Ok(saved) => println!("✓ Звіт успішно експортовано у файл: {}", saved.display()),
                Err(e) => {
                    eprintln!("Помилка експорту: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Config {
            psu,
            mobo_watts,
            reset,
        }) => {
            if reset {
                cfg = AppConfig::default();
                save_config(&cfg);
                println!("✓ Конфігурацію скинуто до заводських налаштувань.");
            } else {
                let mut changed = false;
                if let Some(ref p) = psu {
                    if let Some((canonical, eff)) = lookup_psu_efficiency(p) {
                        cfg.psu_rating = canonical.to_string();
                        cfg.psu_efficiency = eff;
                        changed = true;
                    } else {
                        eprintln!("Невідомий сертифікат БЖ '{}'.", p);
                        std::process::exit(1);
                    }
                }
                if let Some(mw) = mobo_watts {
                    if mw >= 5.0 {
                        cfg.motherboard_base_watts = (mw * 10.0).round() / 10.0;
                        changed = true;
                    }
                }
                if changed {
                    save_config(&cfg);
                    println!("✓ Налаштування успішно оновлено!");
                }
            }
            print_config_report(&cfg);
        }
        Some(Commands::Benchmark { iterations }) => {
            let count = iterations.clamp(3, 200);
            println!(
                "⚡ Запуск тесту продуктивності сенсорів Telemetria Rust Engine ({} ітерацій у реальному часі)...",
                count
            );
            let svc = LiveTariffService::new();
            svc.refresh_now(false, cli.offline, cfg.tariff_cache_ttl_sec);
            let mut monitor = HardwareMonitor::new();

            let mut latencies_us = Vec::with_capacity(count);
            let mut watts_samples = Vec::with_capacity(count);
            let start_total = Instant::now();

            for i in 1..=count {
                thread::sleep(Duration::from_millis(40));
                let snap = monitor.sample(&cfg, &svc, false);
                latencies_us.push(snap.sample_latency_us);
                watts_samples.push(snap.power.total_wall_watts);
                let gpu_w = snap.gpus.first().map(|g| g.power_watts).unwrap_or(0.0);
                let gpu_t = snap.gpus.first().map(|g| g.temperature_c).unwrap_or(0.0);
                println!(
                    "  [{:02}/{}] Час опитування: {:>5} µs ({:.2} ms) | CPU: {:>4.1}% ({:.1}°C, {:.1}W) | GPU: {:.1}°C, {:.2}W | Розетка AC: {:.2}W",
                    i,
                    count,
                    snap.sample_latency_us,
                    snap.sample_latency_us as f64 / 1000.0,
                    snap.cpu.usage_percent,
                    snap.cpu.temperature_c,
                    snap.cpu.power_watts,
                    gpu_t,
                    gpu_w,
                    snap.power.total_wall_watts
                );
            }

            let min_us = *latencies_us.iter().min().unwrap_or(&0);
            let max_us = *latencies_us.iter().max().unwrap_or(&0);
            let avg_us = latencies_us.iter().sum::<u64>() as f64 / (count as f64);
            let avg_w = watts_samples.iter().sum::<f64>() / (count as f64);
            println!("────────────────────────────────────────────────────────────────────────────────────────────────────");
            println!(
                "✓ Результат Rust Engine: Мін: {} µs | Сер: {:.0} µs ({:.2} ms) | Макс: {} µs | Сер. потужність AC: {:.2} W | Загальний час: {:.2} с",
                min_us,
                avg_us,
                avg_us / 1000.0,
                max_us,
                avg_w,
                start_total.elapsed().as_secs_f64()
            );
        }
    }
}
