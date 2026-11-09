"""
Electricity cost, PSU wall-power conversion, and Server Rental / Mining profitability engine.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, List

from telemetria.config import AppConfig, WORKLOAD_PRESETS
from telemetria.models import (
    ExchangeRates,
    MiningRentalMetrics,
    PowerBreakdown,
    TariffProfile,
)


@dataclass
class WorkloadScenarioRow:
    scenario_name: str
    wall_watts: float
    kwh_per_day: float
    cost_day_uah: float
    cost_month_uah: float
    gross_day_uah: float
    net_day_uah: float
    net_month_uah: float
    net_month_usd: float
    night_8h_net_month_uah: float
    roi_status: str


def compute_weighted_24h_kwh_rate_uah(tariff: TariffProfile) -> float:
    """
    Computes the true 24-hour weighted average electricity rate (UAH/kWh)
    taking into account Day (16h) and Night (8h, 23:00-07:00) or Three-Zone schedules.
    """
    if tariff.profile_id == "ua_three_zone":
        # 5h peak + 11h day + 8h night = 24h
        return (
            5.0 * tariff.peak_rate_uah_kwh
            + 11.0 * tariff.day_rate_uah_kwh
            + 8.0 * tariff.night_rate_uah_kwh
        ) / 24.0
    if tariff.day_rate_uah_kwh != tariff.night_rate_uah_kwh:
        # 16h day (07:00-23:00) + 8h night (23:00-07:00)
        return (16.0 * tariff.day_rate_uah_kwh + 8.0 * tariff.night_rate_uah_kwh) / 24.0
    return tariff.current_rate_uah_kwh


def calculate_power_breakdown(
    cpu_watts: float,
    gpu_watts: float,
    ram_watts: float,
    disk_watts: float,
    motherboard_base_watts: float,
    psu_rating: str,
    psu_efficiency: float,
) -> PowerBreakdown:
    # Add dynamic fan / VRM overhead (~4% of CPU+GPU load)
    vrm_fan_overhead = max(0.0, (cpu_watts + gpu_watts) * 0.04)
    mobo_fans = round(motherboard_base_watts + vrm_fan_overhead, 2)
    total_dc = round(cpu_watts + gpu_watts + ram_watts + disk_watts + mobo_fans, 2)
    eff = max(0.50, min(0.99, psu_efficiency))
    total_wall = round(total_dc / eff, 2)
    psu_loss = round(total_wall - total_dc, 2)
    return PowerBreakdown(
        cpu_watts=round(cpu_watts, 2),
        gpu_watts=round(gpu_watts, 2),
        ram_watts=round(ram_watts, 2),
        disk_watts=round(disk_watts, 2),
        motherboard_fans_watts=mobo_fans,
        total_dc_watts=total_dc,
        psu_rating=psu_rating,
        psu_efficiency=round(eff, 4),
        psu_loss_watts=psu_loss,
        total_wall_watts=total_wall,
        total_wall_kw=round(total_wall / 1000.0, 5),
    )


def calculate_mining_rental_metrics(
    power: PowerBreakdown,
    tariff: TariffProfile,
    fx: ExchangeRates,
    config: AppConfig,
) -> MiningRentalMetrics:
    preset = WORKLOAD_PRESETS.get(
        config.workload_profile, WORKLOAD_PRESETS["vastai_rtx5080"]
    )
    gross_usd_hr = float(config.rental_revenue_usd_hr)
    gross_uah_hr = gross_usd_hr * fx.usd_to_uah

    wall_kw = power.total_wall_kw
    elec_uah_hr = wall_kw * tariff.current_rate_uah_kwh
    elec_usd_hr = elec_uah_hr / max(fx.usd_to_uah, 0.01)

    net_uah_hr = gross_uah_hr - elec_uah_hr
    net_usd_hr = gross_usd_hr - elec_usd_hr

    # 24h projection using weighted Day (16h) + Night (8h) schedule
    weighted_rate_uah = compute_weighted_24h_kwh_rate_uah(tariff)
    daily_elec_uah = wall_kw * 24.0 * weighted_rate_uah
    daily_gross_uah = gross_uah_hr * 24.0
    net_uah_day = daily_gross_uah - daily_elec_uah
    net_usd_day = net_uah_day / max(fx.usd_to_uah, 0.01)

    net_uah_month = net_uah_day * 30.0
    net_usd_month = net_usd_day * 30.0

    # Night-only 8h window (23:00-07:00) profit
    night_elec_8h_uah = wall_kw * 8.0 * tariff.night_rate_uah_kwh
    night_gross_8h_uah = gross_uah_hr * 8.0
    night_profit_8h_uah = night_gross_8h_uah - night_elec_8h_uah

    if gross_uah_hr > 0:
        elec_share_pct = round((elec_uah_hr / gross_uah_hr) * 100.0, 1)
    else:
        elec_share_pct = 100.0

    # Break-even electricity price (UAH/kWh) where Gross Revenue == Electricity Cost
    if wall_kw > 0:
        break_even_tariff_uah = round(gross_uah_hr / wall_kw, 2)
    else:
        break_even_tariff_uah = 999.0

    # Minimum rental rate ($/hr) needed to cover current electricity cost
    break_even_rental_usd = round(elec_usd_hr, 4)

    is_profitable = net_uah_hr > 0
    if not is_profitable:
        eff_label = "ЗБИТКОВО (Витрати > Дохід)"
    elif elec_share_pct <= 20.0:
        eff_label = "ВІДМІННА РЕНТАБЕЛЬНІСТЬ (Високий ROI)"
    elif elec_share_pct <= 40.0:
        eff_label = "ДОБРА РЕНТАБЕЛЬНІСТЬ (Оптимально)"
    elif elec_share_pct <= 65.0:
        eff_label = "ПОМІРНА МАРЖА (Вигідніше вночі)"
    else:
        eff_label = "НИЗЬКА МАРЖА (На межі окупності)"

    return MiningRentalMetrics(
        workload_profile=preset["name"],
        workload_description=preset["description"],
        gross_revenue_usd_hr=round(gross_usd_hr, 4),
        gross_revenue_uah_hr=round(gross_uah_hr, 2),
        electricity_cost_uah_hr=round(elec_uah_hr, 2),
        electricity_cost_usd_hr=round(elec_usd_hr, 4),
        net_profit_uah_hr=round(net_uah_hr, 2),
        net_profit_usd_hr=round(net_usd_hr, 4),
        net_profit_uah_day_24h=round(net_uah_day, 2),
        net_profit_usd_day_24h=round(net_usd_day, 2),
        net_profit_uah_month_30d=round(net_uah_month, 2),
        net_profit_usd_month_30d=round(net_usd_month, 2),
        night_only_profit_uah_8h=round(night_profit_8h_uah, 2),
        electricity_share_percent=elec_share_pct,
        break_even_tariff_uah_kwh=break_even_tariff_uah,
        break_even_rental_usd_hr=break_even_rental_usd,
        is_profitable_now=is_profitable,
        efficiency_label=eff_label,
    )


def simulate_workload_scenarios(
    current_wall_watts: float,
    cpu_tdp_watts: float,
    gpu_tdp_watts: float,
    psu_efficiency: float,
    tariff: TariffProfile,
    fx: ExchangeRates,
    rental_usd_hr: float,
) -> List[WorkloadScenarioRow]:
    """
    Generates a multi-scenario comparison table for Server Rental & Mining decision-making.
    """
    eff = max(0.50, min(0.99, psu_efficiency))
    weighted_rate_uah = compute_weighted_24h_kwh_rate_uah(tariff)

    idle_dc = 65.0
    cpu_srv_dc = 45.0 + cpu_tdp_watts * 0.75
    ai_eco_dc = 55.0 + cpu_tdp_watts * 0.45 + gpu_tdp_watts * 0.72
    full_load_dc = 65.0 + cpu_tdp_watts * 0.85 + gpu_tdp_watts * 0.96

    raw_scenarios = [
        ("1. Поточний стан ПК (Live Зараз)", current_wall_watts, rental_usd_hr),
        ("2. Простій / Очікування клієнта (Idle)", round(idle_dc / eff, 1), 0.04),
        ("3. Оренда CPU/RAM/NVMe Сервера (Docker/VPS)", round(cpu_srv_dc / eff, 1), rental_usd_hr * 0.55),
        ("4. AI Інференс / Vast.ai (GPU 75% Eco Limit)", round(ai_eco_dc / eff, 1), rental_usd_hr),
        ("5. Макс. Рендер / Майнінг (GPU+CPU 100% TDP)", round(full_load_dc / eff, 1), rental_usd_hr * 1.12),
    ]

    rows: List[WorkloadScenarioRow] = []
    for name, wall_w, rev_usd_hr in raw_scenarios:
        kw = wall_w / 1000.0
        kwh_day = round(kw * 24.0, 2)
        cost_day_uah = round(kwh_day * weighted_rate_uah, 2)
        cost_month_uah = round(cost_day_uah * 30.0, 2)

        gross_day_uah = round(rev_usd_hr * fx.usd_to_uah * 24.0, 2)
        net_day_uah = round(gross_day_uah - cost_day_uah, 2)
        net_month_uah = round(net_day_uah * 30.0, 2)
        net_month_usd = round(net_month_uah / max(fx.usd_to_uah, 0.01), 2)

        # Night 8h only (23:00-07:00) over 30 days
        night_cost_day = kw * 8.0 * tariff.night_rate_uah_kwh
        night_gross_day = rev_usd_hr * fx.usd_to_uah * 8.0
        night_net_month_uah = round((night_gross_day - night_cost_day) * 30.0, 2)

        if net_day_uah <= 0:
            status = "Збитково 24/7"
        elif cost_day_uah / max(gross_day_uah, 0.01) < 0.25:
            status = "Високий прибуток"
        else:
            status = "Прибутково"

        rows.append(
            WorkloadScenarioRow(
                scenario_name=name,
                wall_watts=round(wall_w, 1),
                kwh_per_day=kwh_day,
                cost_day_uah=cost_day_uah,
                cost_month_uah=cost_month_uah,
                gross_day_uah=gross_day_uah,
                net_day_uah=net_day_uah,
                net_month_uah=net_month_uah,
                net_month_usd=net_month_usd,
                night_8h_net_month_uah=night_net_month_uah,
                roi_status=status,
            )
        )
    return rows
