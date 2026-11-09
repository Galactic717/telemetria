use crate::config::{get_workload_preset, AppConfig};
use crate::models::{
    ExchangeRates, MiningRentalMetrics, PowerBreakdown, TariffProfile, WorkloadScenarioRow,
};

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

pub fn round4(v: f64) -> f64 {
    (v * 10000.0).round() / 10000.0
}

pub fn round5(v: f64) -> f64 {
    (v * 100000.0).round() / 100000.0
}

/// Computes the true 24-hour weighted average electricity tariff (UAH/kWh)
/// accounting for Day (16h, 07:00-23:00) and Night (8h, 23:00-07:00) or Three-Zone schedules.
pub fn compute_weighted_24h_kwh_rate_uah(tariff: &TariffProfile) -> f64 {
    if tariff.profile_id == "ua_three_zone" {
        (5.0 * tariff.peak_rate_uah_kwh
            + 11.0 * tariff.day_rate_uah_kwh
            + 8.0 * tariff.night_rate_uah_kwh)
            / 24.0
    } else if (tariff.day_rate_uah_kwh - tariff.night_rate_uah_kwh).abs() > 1e-4 {
        (16.0 * tariff.day_rate_uah_kwh + 8.0 * tariff.night_rate_uah_kwh) / 24.0
    } else {
        tariff.current_rate_uah_kwh
    }
}

pub fn calculate_power_breakdown(
    cpu_watts: f64,
    gpu_watts: f64,
    ram_watts: f64,
    disk_watts: f64,
    motherboard_base_watts: f64,
    psu_rating: &str,
    psu_efficiency: f64,
) -> PowerBreakdown {
    let vrm_fan_overhead = ((cpu_watts + gpu_watts) * 0.04).max(0.0);
    let mobo_fans = round2(motherboard_base_watts + vrm_fan_overhead);
    let total_dc = round2(cpu_watts + gpu_watts + ram_watts + disk_watts + mobo_fans);
    let eff = psu_efficiency.clamp(0.50, 0.99);
    let total_wall = round2(total_dc / eff);
    let psu_loss = round2(total_wall - total_dc);

    PowerBreakdown {
        cpu_watts: round2(cpu_watts),
        gpu_watts: round2(gpu_watts),
        ram_watts: round2(ram_watts),
        disk_watts: round2(disk_watts),
        motherboard_fans_watts: mobo_fans,
        total_dc_watts: total_dc,
        psu_rating: psu_rating.to_string(),
        psu_efficiency: round4(eff),
        psu_loss_watts: psu_loss,
        total_wall_watts: total_wall,
        total_wall_kw: round5(total_wall / 1000.0),
    }
}

pub fn calculate_mining_rental_metrics(
    power: &PowerBreakdown,
    tariff: &TariffProfile,
    fx: &ExchangeRates,
    config: &AppConfig,
) -> MiningRentalMetrics {
    let preset = get_workload_preset(&config.workload_profile);
    let gross_usd_hr = config.rental_revenue_usd_hr.max(0.0);
    let gross_uah_hr = gross_usd_hr * fx.usd_to_uah;

    let wall_kw = power.total_wall_kw;
    let elec_uah_hr = wall_kw * tariff.current_rate_uah_kwh;
    let elec_usd_hr = elec_uah_hr / fx.usd_to_uah.max(0.01);

    let net_uah_hr = gross_uah_hr - elec_uah_hr;
    let net_usd_hr = gross_usd_hr - elec_usd_hr;

    let weighted_rate_uah = compute_weighted_24h_kwh_rate_uah(tariff);
    let daily_elec_uah = wall_kw * 24.0 * weighted_rate_uah;
    let daily_gross_uah = gross_uah_hr * 24.0;
    let net_uah_day = daily_gross_uah - daily_elec_uah;
    let net_usd_day = net_uah_day / fx.usd_to_uah.max(0.01);

    let net_uah_month = net_uah_day * 30.0;
    let net_usd_month = net_usd_day * 30.0;

    let night_elec_8h_uah = wall_kw * 8.0 * tariff.night_rate_uah_kwh;
    let night_gross_8h_uah = gross_uah_hr * 8.0;
    let night_profit_8h_uah = night_gross_8h_uah - night_elec_8h_uah;

    let elec_share_pct = if gross_uah_hr > 0.0 {
        round2((elec_uah_hr / gross_uah_hr) * 100.0)
    } else {
        100.0
    };

    let break_even_tariff_uah = if wall_kw > 0.0 {
        round2(gross_uah_hr / wall_kw)
    } else {
        999.0
    };

    let break_even_rental_usd = round4(elec_usd_hr);
    let is_profitable = net_uah_hr > 0.0;

    let eff_label = if !is_profitable {
        "ЗБИТКОВО (Витрати > Дохід)"
    } else if elec_share_pct <= 20.0 {
        "ВІДМІННА РЕНТАБЕЛЬНІСТЬ (Високий ROI)"
    } else if elec_share_pct <= 40.0 {
        "ДОБРА РЕНТАБЕЛЬНІСТЬ (Оптимально)"
    } else if elec_share_pct <= 65.0 {
        "ПОМІРНА МАРЖА (Вигідніше вночі)"
    } else {
        "НИЗЬКА МАРЖА (На межі окупності)"
    };

    MiningRentalMetrics {
        workload_id: preset.id.to_string(),
        workload_profile: preset.name.to_string(),
        workload_description: preset.description.to_string(),
        gross_revenue_usd_hr: round4(gross_usd_hr),
        gross_revenue_uah_hr: round2(gross_uah_hr),
        electricity_cost_uah_hr: round2(elec_uah_hr),
        electricity_cost_usd_hr: round4(elec_usd_hr),
        net_profit_uah_hr: round2(net_uah_hr),
        net_profit_usd_hr: round4(net_usd_hr),
        net_profit_uah_day_24h: round2(net_uah_day),
        net_profit_usd_day_24h: round2(net_usd_day),
        net_profit_uah_month_30d: round2(net_uah_month),
        net_profit_usd_month_30d: round2(net_usd_month),
        night_only_profit_uah_8h: round2(night_profit_8h_uah),
        electricity_share_percent: elec_share_pct,
        break_even_tariff_uah_kwh: break_even_tariff_uah,
        break_even_rental_usd_hr: break_even_rental_usd,
        is_profitable_now: is_profitable,
        efficiency_label: eff_label.to_string(),
    }
}

pub fn simulate_workload_scenarios(
    current_wall_watts: f64,
    cpu_tdp_watts: f64,
    gpu_tdp_watts: f64,
    psu_efficiency: f64,
    tariff: &TariffProfile,
    fx: &ExchangeRates,
    rental_usd_hr: f64,
) -> Vec<WorkloadScenarioRow> {
    let eff = psu_efficiency.clamp(0.50, 0.99);
    let weighted_rate_uah = compute_weighted_24h_kwh_rate_uah(tariff);

    let idle_dc = 65.0;
    let cpu_srv_dc = 45.0 + cpu_tdp_watts * 0.75;
    let ai_eco_dc = 55.0 + cpu_tdp_watts * 0.45 + gpu_tdp_watts * 0.72;
    let full_load_dc = 65.0 + cpu_tdp_watts * 0.85 + gpu_tdp_watts * 0.96;

    let raw = [
        (
            "1. Поточний стан ПК (Live Зараз)",
            current_wall_watts,
            rental_usd_hr,
        ),
        (
            "2. Простій / Очікування клієнта (Idle)",
            round2(idle_dc / eff),
            0.04,
        ),
        (
            "3. Оренда CPU/RAM/NVMe Сервера (Docker/VPS)",
            round2(cpu_srv_dc / eff),
            rental_usd_hr * 0.55,
        ),
        (
            "4. AI Інференс / Vast.ai (GPU 75% Eco Limit)",
            round2(ai_eco_dc / eff),
            rental_usd_hr,
        ),
        (
            "5. Макс. Рендер / Майнінг (GPU+CPU 100% TDP)",
            round2(full_load_dc / eff),
            rental_usd_hr * 1.12,
        ),
    ];

    let mut rows = Vec::with_capacity(raw.len());
    for (name, wall_w, rev_usd_hr) in raw {
        let kw = wall_w / 1000.0;
        let kwh_day = round2(kw * 24.0);
        let cost_day_uah = round2(kwh_day * weighted_rate_uah);
        let cost_month_uah = round2(cost_day_uah * 30.0);

        let gross_day_uah = round2(rev_usd_hr * fx.usd_to_uah * 24.0);
        let net_day_uah = round2(gross_day_uah - cost_day_uah);
        let net_month_uah = round2(net_day_uah * 30.0);
        let net_month_usd = round2(net_month_uah / fx.usd_to_uah.max(0.01));

        let night_cost_day = kw * 8.0 * tariff.night_rate_uah_kwh;
        let night_gross_day = rev_usd_hr * fx.usd_to_uah * 8.0;
        let night_net_month_uah = round2((night_gross_day - night_cost_day) * 30.0);

        let roi_status = if net_day_uah <= 0.0 {
            "Збитково 24/7"
        } else if (cost_day_uah / gross_day_uah.max(0.01)) < 0.25 {
            "Високий прибуток"
        } else {
            "Прибутково"
        };

        rows.push(WorkloadScenarioRow {
            scenario_name: name.to_string(),
            wall_watts: round2(wall_w),
            kwh_per_day: kwh_day,
            cost_day_uah,
            cost_month_uah,
            gross_day_uah,
            net_day_uah,
            net_month_uah,
            net_month_usd,
            night_8h_net_month_uah: night_net_month_uah,
            roi_status: roi_status.to_string(),
        });
    }
    rows
}
