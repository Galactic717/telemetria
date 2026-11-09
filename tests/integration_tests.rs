use std::time::Duration;
use telemetria::config::AppConfig;
use telemetria::economics::{
    calculate_mining_rental_metrics, calculate_power_breakdown, compute_weighted_24h_kwh_rate_uah,
    simulate_workload_scenarios,
};
use telemetria::models::{ExchangeRates, TariffProfile};
use telemetria::sensors::{detect_cpu_tdp_and_profile, HardwareMonitor};
use telemetria::storage::TelemetryStorage;
use telemetria::tariffs::{LiveTariffService, MarketCache};

fn sample_fx() -> ExchangeRates {
    ExchangeRates {
        usd_to_uah: 45.0,
        eur_to_uah: 49.5,
        pln_to_uah: 11.4,
        eur_to_usd: 1.1,
        updated_at: "2026-11-09 10:00:00".to_string(),
        source: "Test FX".to_string(),
        is_live: true,
    }
}

fn sample_tariff() -> TariffProfile {
    TariffProfile {
        profile_id: "ua_two_zone".to_string(),
        name: "Двозонний День/Ніч".to_string(),
        country: "UA".to_string(),
        currency: "UAH".to_string(),
        current_rate_uah_kwh: 4.32,
        day_rate_uah_kwh: 4.32,
        night_rate_uah_kwh: 2.16,
        peak_rate_uah_kwh: 4.32,
        current_zone_label: "ДЕНЬ".to_string(),
        is_night_discount_active: false,
        schedule_description: "Test".to_string(),
        source: "Test".to_string(),
        updated_at: "2026-11-09 10:00:00".to_string(),
        is_live: true,
    }
}

#[test]
fn test_power_breakdown_and_weighted_tariff() {
    let p = calculate_power_breakdown(60.0, 240.0, 5.0, 2.0, 21.0, "80+ Gold", 0.90);
    assert_eq!(p.cpu_watts, 60.0);
    assert_eq!(p.gpu_watts, 240.0);
    assert!(p.total_wall_watts > p.total_dc_watts);
    assert!((p.total_wall_watts * 0.90 - p.total_dc_watts).abs() < 0.25);

    let t = sample_tariff();
    let weighted = compute_weighted_24h_kwh_rate_uah(&t);
    // (16 * 4.32 + 8 * 2.16) / 24 = 3.60
    assert!((weighted - 3.60).abs() < 1e-4);
}

#[test]
fn test_two_zone_and_three_zone_schedules() {
    let market = MarketCache::default();

    let day_t = LiveTariffService::build_tariff_profile(&market, "ua_two_zone", 4.32, Some(14));
    assert!(!day_t.is_night_discount_active);
    assert!((day_t.current_rate_uah_kwh - 4.32).abs() < 1e-3);

    let night_t = LiveTariffService::build_tariff_profile(&market, "ua_two_zone", 4.32, Some(2));
    assert!(night_t.is_night_discount_active);
    assert!((night_t.current_rate_uah_kwh - 2.16).abs() < 1e-3);

    let peak_3z = LiveTariffService::build_tariff_profile(&market, "ua_three_zone", 4.32, Some(9));
    assert!((peak_3z.current_rate_uah_kwh - 6.48).abs() < 1e-3);

    let night_3z =
        LiveTariffService::build_tariff_profile(&market, "ua_three_zone", 4.32, Some(23));
    assert!((night_3z.current_rate_uah_kwh - 1.728).abs() < 1e-3);
}

#[test]
fn test_mining_profitability_and_simulator() {
    let p = calculate_power_breakdown(55.0, 210.0, 5.0, 2.0, 20.0, "80+ Gold", 0.90);
    let t = sample_tariff();
    let fx = sample_fx();
    let mut cfg = AppConfig::default();
    cfg.rental_revenue_usd_hr = 0.45;

    let m = calculate_mining_rental_metrics(&p, &t, &fx, &cfg);
    assert!(m.is_profitable_now);
    assert!(m.net_profit_uah_hr > 0.0);
    assert!(m.break_even_tariff_uah_kwh > t.current_rate_uah_kwh);
    assert!(m.break_even_rental_usd_hr < cfg.rental_revenue_usd_hr);

    let scenarios = simulate_workload_scenarios(120.0, 120.0, 360.0, 0.90, &t, &fx, 0.42);
    assert_eq!(scenarios.len(), 5);
    assert_eq!(scenarios[0].wall_watts, 120.0);
}

#[test]
fn test_cpu_profile_and_hardware_monitor_sample() {
    let (tdp, ppt, idle) = detect_cpu_tdp_and_profile("AMD Ryzen 7 7800X3D 8-Core Processor", 16);
    assert_eq!(tdp, 120.0);
    assert_eq!(ppt, 88.0);
    assert!(idle > 15.0);

    let cfg = AppConfig::default();
    let svc = LiveTariffService::new();
    let mut monitor = HardwareMonitor::new();
    let snap1 = monitor.sample(&cfg, &svc, false);
    std::thread::sleep(Duration::from_millis(40));
    let snap2 = monitor.sample(&cfg, &svc, false);

    assert!(snap2.cpu.logical_cores >= 1);
    assert!(snap2.power.total_wall_watts > snap2.power.total_dc_watts);
    assert_eq!(snap2.session.samples_count, 2);
    assert!(snap2.session.energy_wh >= snap1.session.energy_wh);

    let tmp_dir = std::env::temp_dir().join(format!("telemetria_test_{}", snap2.session.session_id));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let storage = TelemetryStorage::new(Some(tmp_dir.join("ledger.json")));
    storage.record_snapshot(&snap2);

    let summary = storage.get_lifetime_summary();
    assert_eq!(summary.total_sessions, 1);

    let csv_path = tmp_dir.join("report.csv");
    let json_path = tmp_dir.join("report.json");
    assert!(storage.export_sessions_csv(&csv_path).is_ok());
    assert!(storage.export_sessions_json(&json_path).is_ok());
    assert!(csv_path.exists());
    assert!(json_path.exists());
    let _ = std::fs::remove_dir_all(&tmp_dir);
}
