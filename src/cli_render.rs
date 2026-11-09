use crate::config::{get_config_path, AppConfig};
use crate::economics::compute_weighted_24h_kwh_rate_uah;
use crate::models::{
    CryptoMarketSnapshot, ExchangeRates, SessionAccumulator, SystemSnapshot, TariffProfile,
    WorkloadScenarioRow,
};
use crate::storage::LifetimeSummary;

pub fn format_duration(seconds: f64) -> String {
    let sec = seconds.max(0.0) as u64;
    let days = sec / 86400;
    let rem = sec % 86400;
    let hours = rem / 3600;
    let rem = rem % 3600;
    let mins = rem / 60;
    let s = rem % 60;
    if days > 0 {
        format!("{}д {:02}г {:02}хв", days, hours, mins)
    } else {
        format!("{:02}:{:02}:{:02}", hours, mins, s)
    }
}

pub fn make_bar(pct: f64, width: usize) -> String {
    let clamped = pct.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * (width as f64)).round() as usize;
    let empty = width.saturating_sub(filled);
    format!(
        "{}{} {:5.1}%",
        "█".repeat(filled),
        "░".repeat(empty),
        clamped
    )
}

pub fn print_snapshot_report(snap: &SystemSnapshot) {
    let live_str = if snap.fx.is_live {
        "● LIVE INTERNET"
    } else {
        "○ CACHED"
    };
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!(
        "║ ⚡ TELEMETRIA v2.0 (Rust Native) | {} ({}) | Плата: {} | Аптайм: {} | Сенсори: {} µs",
        snap.hostname,
        snap.os_name,
        snap.motherboard,
        format_duration(snap.uptime_seconds as f64),
        snap.sample_latency_us
    );
    println!(
        "║ 🌐 {} | Активна зона: [{}] | Тариф: {:.2} ₴/кВт·год (${:.4}) | USD: {:.2} ₴ | EUR: {:.2} ₴ | BTC: ${:.0} | RENDER: ${:.3}",
        live_str,
        snap.tariff.current_zone_label,
        snap.tariff.current_rate_uah_kwh,
        snap.tariff.rate_in_usd(&snap.fx),
        snap.fx.usd_to_uah,
        snap.fx.eur_to_uah,
        snap.crypto.btc_usd,
        snap.crypto.render_usd
    );
    println!("╠══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣");

    let c = &snap.cpu;
    println!("║ 🖥  ПРОЦЕСОР (CPU TELEMETRY)                                                                                              ║");
    println!(
        "║   • Модель:       {} ({}C / {}T) @ {:.0} / {:.0} MHz",
        c.name, c.physical_cores, c.logical_cores, c.frequency_current_mhz, c.frequency_max_mhz
    );
    println!(
        "║   • Завантаження: {}  |  Температура: {:.1} °C [{}]  |  Споживання: {:.1} W / {:.0} W TDP ({:.0}%)",
        make_bar(c.usage_percent, 18),
        c.temperature_c,
        c.temp_source,
        c.power_watts,
        c.tdp_watts,
        (c.power_watts / c.tdp_watts.max(1.0)) * 100.0
    );

    println!("╟──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╢");
    println!("║ 🎮 ВІДЕОКАРТА (GPU TELEMETRY)                                                                                            ║");
    if let Some(g) = snap.gpus.first() {
        let vram_pct = (g.vram_used_mb / g.vram_total_mb.max(1.0)) * 100.0;
        let fan_str = if g.fan_speed_percent > 0.0 {
            format!("{:.0}%", g.fan_speed_percent)
        } else {
            "0% (Zero-RPM Fan Stop)".to_string()
        };
        let energy_str = g
            .energy_total_kwh
            .map(|e| format!(" | Лічильник GPU: {:.4} кВт·год", e))
            .unwrap_or_default();
        println!(
            "║   • Модель:       {} (PCIe Gen{} x{}, {}, [{}])",
            g.name, g.pcie_gen, g.pcie_width, g.pstate, g.source
        );
        println!(
            "║   • Ядро / VRAM:  {}  |  VRAM: {:.0} / {:.0} MiB ({:.1}%)  |  PCIe TX/RX: {:.1}/{:.1} MB/s",
            make_bar(g.usage_percent, 18),
            g.vram_used_mb,
            g.vram_total_mb,
            vram_pct,
            g.pcie_tx_mbs,
            g.pcie_rx_mbs
        );
        println!(
            "║   • Терміка/Ват:  {:.1} °C | Кулер: {} | Споживання: {:.2} W / {:.0} W Limit{}",
            g.temperature_c, fan_str, g.power_watts, g.power_limit_watts, energy_str
        );
        println!(
            "║   • Частоти:      Core: {:.0} MHz (Boost {:.0} MHz) | Memory: {:.0} MHz",
            g.clock_core_mhz, g.clock_max_core_mhz, g.clock_mem_mhz
        );
    } else {
        println!("║   • Дискретна відеокарта не виявлена                                                                                     ║");
    }

    println!("╟──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╢");
    let r = &snap.ram;
    let d = &snap.disk;
    let n = &snap.network;
    println!("║ 💾 ПАМ'ЯТЬ, НАКОПИЧУВАЧІ ТА МЕРЕЖА (RAM · NVMe · NET)                                                                    ║");
    println!(
        "║   • ОЗП ({}x {}-{}): {:.1} / {:.1} GB ({:.1}%) | Swap: {:.1}/{:.1} GB | Споживання ОЗП: {:.1} W [{}]",
        r.dimm_count, r.memory_type, r.speed_mts, r.used_gb, r.total_gb, r.usage_percent, r.swap_used_gb, r.swap_total_gb, r.power_watts, r.part_number
    );
    let parts_str = d
        .partitions
        .iter()
        .take(3)
        .map(|p| format!("{} {:.0}/{:.0}GB ({:.0}%)", p.mountpoint, p.used_gb, p.total_gb, p.usage_percent))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "║   • Диск ({}): {} [{}] | Read: {:.2} MB/s, Write: {:.2} MB/s | {:.1} W | Розділи: {}",
        d.model_name, d.media_type, d.health_status, d.read_speed_mbs, d.write_speed_mbs, d.power_watts, parts_str
    );
    println!(
        "║   • Мережа ({}): ↓ {:.3} MB/s | ↑ {:.3} MB/s (Всього за сесію: ↓{:.2} GB / ↑{:.2} GB)",
        n.primary_interface, n.download_speed_mbs, n.upload_speed_mbs, n.total_recv_gb, n.total_sent_gb
    );

    println!("╟──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╢");
    let p = &snap.power;
    let t = &snap.tariff;
    let weighted_24h = compute_weighted_24h_kwh_rate_uah(t);
    let cost_hr_uah = p.total_wall_kw * t.current_rate_uah_kwh;
    let cost_hr_usd = cost_hr_uah / snap.fx.usd_to_uah.max(0.01);
    let cost_day_uah = p.total_wall_kw * 24.0 * weighted_24h;
    let cost_day_usd = cost_day_uah / snap.fx.usd_to_uah.max(0.01);
    let cost_month_uah = cost_day_uah * 30.0;
    let cost_month_usd = cost_day_usd * 30.0;

    println!("║ 🔌 ЕЛЕКТРОЕНЕРГІЯ ТА ЛІЧИЛЬНИК КОШТІВ (Скільки з'їв ПК)                                                                  ║");
    println!(
        "║   • З розетки (AC Wall Power): ⚡ {:.2} W ({:.4} кВт)  [БЖ {}: ККД {:.0}%, втрати тепла БЖ +{:.1} W]",
        p.total_wall_watts, p.total_wall_kw, p.psu_rating, p.psu_efficiency * 100.0, p.psu_loss_watts
    );
    println!(
        "║   • Компоненти (DC):           CPU {:.1}W + GPU {:.1}W + RAM {:.1}W + NVMe {:.1}W + Плата/Кулери {:.1}W = {:.1}W",
        p.cpu_watts, p.gpu_watts, p.ram_watts, p.disk_watts, p.motherboard_fans_watts, p.total_dc_watts
    );
    println!(
        "║   • Тариф ({}): {:.2} ₴/кВт·год (День: {:.2} ₴ | Ніч 23-07: {:.2} ₴) [{}]",
        t.profile_id, t.current_rate_uah_kwh, t.day_rate_uah_kwh, t.night_rate_uah_kwh, t.source
    );
    println!(
        "║   • Витрати на електрику:      Година: {:.2} ₴ (${:.3}) | Доба (Д+Н): {:.2} ₴ (${:.2}) | Місяць (30д): {:.0} ₴ (${:.1})",
        cost_hr_uah, cost_hr_usd, cost_day_uah, cost_day_usd, cost_month_uah, cost_month_usd
    );
    println!(
        "║   • З'їдено за поточну сесію:  {:.4} Вт·год ({:.6} кВт·год) → {:.4} ₴ (${:.5} / €{:.5})",
        snap.session.energy_wh, snap.session.energy_kwh, snap.session.cost_uah, snap.session.cost_usd, snap.session.cost_eur
    );

    println!("╟──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╢");
    let m = &snap.mining;
    println!("║ 💰 РЕЖИМ МАЙНІНГУ ТА ОРЕНДИ СЕРВЕРА (PROFITABILITY & ROI)                                                                ║");
    println!(
        "║   • Профіль роботи:            {} ({})",
        m.workload_profile, m.workload_id
    );
    println!(
        "║   • Валовий дохід:             +{:.2} ₴/год (+${:.3}/год)  |  +{:.2} ₴/доба",
        m.gross_revenue_uah_hr, m.gross_revenue_usd_hr, m.gross_revenue_uah_hr * 24.0
    );
    println!(
        "║   • Чистий прибуток (Net):     {:+.2} ₴/год (${:+.3}/год) | Доба (Д+Н): {:+.2} ₴ (${:+.2}) | Місяць 24/7: {:+.0} ₴ (${:+.1})",
        m.net_profit_uah_hr, m.net_profit_usd_hr, m.net_profit_uah_day_24h, m.net_profit_usd_day_24h, m.net_profit_uah_month_30d, m.net_profit_usd_month_30d
    );
    println!(
        "║   • Нічний режим (23:00-07:00):+{:.2} ₴ за 8 нічних годин (+{:.0} ₴/міс тільки вночі за тарифом {:.2} ₴/кВт·год)",
        m.night_only_profit_uah_8h, m.night_only_profit_uah_8h * 30.0, t.night_rate_uah_kwh
    );
    println!(
        "║   • Ефективність / Межа ROI:   [{}] | Електрика: {:.1}% доходу | Мін. оренда: ${:.3}/год | Макс. тариф: {:.1} ₴",
        m.efficiency_label, m.electricity_share_percent, m.break_even_rental_usd_hr, m.break_even_tariff_uah_kwh
    );

    if !snap.top_processes.is_empty() {
        println!("╟──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╢");
        println!("║ 🔥 ТОП ПРОЦЕСІВ ЗА ЕНЕРГОСПОЖИВАННЯМ (PER-PROCESS WATTAGE)                                                               ║");
        println!(
            "║   {:<8} {:<32} {:>10} {:>14} {:>16} {:>18}",
            "PID", "Процес", "CPU %", "RAM (MB)", "Споживання (W)", "Вартість (₴/доба)"
        );
        for pr in &snap.top_processes {
            println!(
                "║   {:<8} {:<32} {:>9.1}% {:>14.1} {:>14.2} W {:>16.3} ₴",
                pr.pid,
                truncate_str(&pr.name, 32),
                pr.cpu_percent,
                pr.memory_mb,
                pr.estimated_watts,
                pr.cost_uah_per_day
            );
        }
    }
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝");
}

pub fn print_tariffs_report(
    profiles: &[TariffProfile],
    active_profile_id: &str,
    fx: &ExchangeRates,
    crypto: &CryptoMarketSnapshot,
) {
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!(
        "║ 🌐 АВТООНОВЛЕННЯ ЦІН З ІНТЕРНЕТУ ({}, оновлено {})",
        fx.source, fx.updated_at
    );
    println!(
        "║ 💱 Курси НБУ / FX:     1 USD = {:.2} ₴  |  1 EUR = {:.2} ₴  |  1 PLN = {:.2} ₴",
        fx.usd_to_uah, fx.eur_to_uah, fx.pln_to_uah
    );
    println!(
        "║ 🪙 Крипто / Compute ({}): BTC = ${:.2}  |  ETH = ${:.2}  |  SOL = ${:.2}  |  RENDER = ${:.4}",
        crypto.source, crypto.btc_usd, crypto.eth_usd, crypto.sol_usd, crypto.render_usd
    );
    println!("╠══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣");
    println!(
        "║ {:<16} | {:<45} | {:>10} | {:>9} | {:>8} | {:>9} | {:<20}",
        "ID профілю", "Назва тарифу", "Зараз (₴)", "Зараз ($)", "День (₴)", "Ніч (₴)", "Активна зона"
    );
    println!("╟──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╢");
    for p in profiles {
        let marker = if p.profile_id == active_profile_id {
            "▶ "
        } else {
            "  "
        };
        let id_col = format!("{}{}", marker, p.profile_id);
        println!(
            "║ {:<16} | {:<45} | {:>8.3} ₴ | ${:>8.4} | {:>6.2} ₴ | {:>7.3} ₴ | {:<20}",
            id_col,
            truncate_str(&p.name, 45),
            p.current_rate_uah_kwh,
            p.rate_in_usd(fx),
            p.day_rate_uah_kwh,
            p.night_rate_uah_kwh,
            truncate_str(&p.current_zone_label, 20)
        );
    }
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝");
}

pub fn print_mining_report(snap: &SystemSnapshot, scenarios: &[WorkloadScenarioRow]) {
    print_snapshot_report(snap);
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║ 📊 СИМУЛЯЦІЯ РЕЖИМІВ ОРЕНДИ СЕРВЕРА ТА МАЙНІНГУ ДЛЯ ВАШОГО ПК (Порівняння 24/7 та Нічного Тарифу 23:00–07:00)            ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣");
    println!(
        "║ {:<43} | {:>9} | {:>8} | {:>11} | {:>11} | {:>12} | {:>14} | {:>11}",
        "Сценарій навантаження ПК", "Розетка", "кВт/доба", "Електр/доба", "Дохід/доба", "Чистий/доба", "Чистий/міс 24/7", "Ніч 8г/міс"
    );
    println!("╟──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╢");
    for r in scenarios {
        println!(
            "║ {:<43} | {:>7.1} W | {:>8.2} | {:>9.2} ₴ | {:>+9.2} ₴ | {:>+10.2} ₴ | {:>+8.0} ₴ (${:+.0}) | {:>+9.0} ₴",
            truncate_str(&r.scenario_name, 43),
            r.wall_watts,
            r.kwh_per_day,
            -r.cost_day_uah,
            r.gross_day_uah,
            r.net_day_uah,
            r.net_month_uah,
            r.net_month_usd,
            r.night_8h_net_month_uah
        );
    }
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝");
}

pub fn print_history_report(summary: &LifetimeSummary, sessions: &[SessionAccumulator]) {
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║ 📚 ІСТОРІЯ ЕНЕРГОСПОЖИВАННЯ ТА ФІНАНСОВИЙ БАЛАНС (Persistent Ledger)                                                     ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣");
    println!(
        "║ • Всього сесій: {}  |  Сумарний час моніторингу: {}  |  Середня / Пікова потужність: {:.1} W / {:.1} W",
        summary.total_sessions,
        format_duration(summary.total_seconds),
        summary.mean_wall_watts,
        summary.max_wall_watts
    );
    println!(
        "║ • Спожито електроенергії: {:.3} Вт·год ({:.5} кВт·год)  |  Пікові температури: CPU {:.1} °C / GPU {:.1} °C",
        summary.total_energy_wh, summary.total_energy_kwh, summary.max_cpu_temp_c, summary.max_gpu_temp_c
    );
    println!(
        "║ • Витрачено на електрику: {:.4} ₴ (${:.5})  |  Валовий дохід: +{:.4} ₴  |  Чистий прибуток: {:+.4} ₴",
        summary.total_cost_uah, summary.total_cost_usd, summary.total_gross_uah, summary.total_net_uah
    );
    println!("╠══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣");
    println!(
        "║ {:<14} | {:<19} | {:>10} | {:>15} | {:>12} | {:>12} | {:>13} | {:<14}",
        "ID сесії", "Початок", "Тривалість", "Сер./Пік (W)", "Енергія (Wh)", "З'їдено (₴)", "Чистими (₴)", "Тариф"
    );
    println!("╟──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╢");
    for s in sessions {
        println!(
            "║ {:<14} | {:<19} | {:>10} | {:>6.1}/{:>6.1} W | {:>9.3} Wh | {:>10.4} ₴ | {:>+11.4} ₴ | {:<14}",
            truncate_str(&s.session_id, 14),
            truncate_str(&s.started_at, 19),
            format_duration(s.elapsed_seconds),
            s.avg_wall_watts,
            s.peak_wall_watts,
            s.energy_wh,
            s.cost_uah,
            s.net_profit_uah,
            truncate_str(&s.tariff_profile, 14)
        );
    }
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝");
}

pub fn print_config_report(cfg: &AppConfig) {
    println!("╔══════════════════════════════════════════════════════════════════════════════════════╗");
    println!(
        "║ ⚙️  КОНФІГУРАЦІЯ TELEMETRIA ({})",
        get_config_path().display()
    );
    println!("╠══════════════════════════════════════════════════════════════════════════════════════╣");
    println!("║ • Тарифний профіль (tariff_profile):          {}", cfg.tariff_profile);
    println!("║ • Власний тариф (custom_tariff_uah_kwh):      {:.2} ₴/кВт·год", cfg.custom_tariff_uah_kwh);
    println!("║ • Сертифікат БЖ (psu_rating):                 {} (ККД {:.0}%)", cfg.psu_rating, cfg.psu_efficiency * 100.0);
    println!("║ • Базове споживання плати (mobo_watts):       {:.1} W", cfg.motherboard_base_watts);
    println!("║ • Профіль оренди/майнінгу (workload_profile): {}", cfg.workload_profile);
    println!("║ • Ставка доходу оренди (rental_revenue_usd):  ${:.3}/год", cfg.rental_revenue_usd_hr);
    println!("║ • Інтервал оновлення (refresh_interval_sec):  {:.2} с", cfg.refresh_interval_sec);
    println!("╚══════════════════════════════════════════════════════════════════════════════════════╝");
}

pub fn truncate_str(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else if max_chars > 2 {
        let mut out: String = s.chars().take(max_chars - 1).collect();
        out.push('…');
        out
    } else {
        s.chars().take(max_chars).collect()
    }
}
