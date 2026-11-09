"""
Unit tests for power breakdown, electricity cost, and server rental / mining profitability calculations.
"""

from __future__ import annotations

from telemetria.config import AppConfig
from telemetria.economics import (
    calculate_mining_rental_metrics,
    calculate_power_breakdown,
    compute_weighted_24h_kwh_rate_uah,
    simulate_workload_scenarios,
)
from telemetria.models import ExchangeRates, TariffProfile


def _make_sample_tariff(
    profile_id: str = "ua_two_zone",
    current: float = 4.32,
    day: float = 4.32,
    night: float = 2.16,
    peak: float = 4.32,
) -> TariffProfile:
    return TariffProfile(
        profile_id=profile_id,
        name="Test Tariff",
        country="UA",
        currency="UAH",
        current_rate_uah_kwh=current,
        day_rate_uah_kwh=day,
        night_rate_uah_kwh=night,
        peak_rate_uah_kwh=peak,
        current_zone_label="DAY",
        is_night_discount_active=False,
        schedule_description="Test",
        source="Unit Test",
        updated_at="2026-11-09 10:00:00",
        is_live=True,
    )


def _make_sample_fx() -> ExchangeRates:
    return ExchangeRates(
        usd_to_uah=45.0,
        eur_to_uah=49.0,
        pln_to_uah=11.2,
        eur_to_usd=1.0889,
        updated_at="2026-11-09 10:00:00",
        source="NBU Test",
        is_live=True,
    )


def test_calculate_power_breakdown_gold_psu() -> None:
    p = calculate_power_breakdown(
        cpu_watts=65.0,
        gpu_watts=250.0,
        ram_watts=6.0,
        disk_watts=3.0,
        motherboard_base_watts=22.0,
        psu_rating="80+ Gold",
        psu_efficiency=0.90,
    )
    assert p.cpu_watts == 65.0
    assert p.gpu_watts == 250.0
    assert p.total_dc_watts > 346.0
    assert p.total_wall_watts > p.total_dc_watts
    assert abs(p.total_wall_watts * 0.90 - p.total_dc_watts) < 0.2
    assert abs(p.total_wall_kw - p.total_wall_watts / 1000.0) < 1e-4


def test_weighted_24h_rate_two_zone() -> None:
    tariff = _make_sample_tariff(day=4.32, night=2.16)
    weighted = compute_weighted_24h_kwh_rate_uah(tariff)
    # (16 * 4.32 + 8 * 2.16) / 24 = 3.60
    assert abs(weighted - 3.60) < 1e-4


def test_mining_rental_profitability_positive_and_breakeven() -> None:
    power = calculate_power_breakdown(
        cpu_watts=60.0,
        gpu_watts=220.0,
        ram_watts=5.0,
        disk_watts=2.0,
        motherboard_base_watts=20.0,
        psu_rating="80+ Gold",
        psu_efficiency=0.90,
    )
    tariff = _make_sample_tariff(current=4.32, day=4.32, night=2.16)
    fx = _make_sample_fx()
    cfg = AppConfig(
        workload_profile="vastai_rtx5080",
        rental_revenue_usd_hr=0.45,
    )

    m = calculate_mining_rental_metrics(power=power, tariff=tariff, fx=fx, config=cfg)
    assert m.gross_revenue_usd_hr == 0.45
    assert abs(m.gross_revenue_uah_hr - 20.25) < 0.01
    assert m.electricity_cost_uah_hr > 0
    assert m.net_profit_uah_hr > 0
    assert m.is_profitable_now is True
    assert m.break_even_tariff_uah_kwh > tariff.current_rate_uah_kwh
    assert m.break_even_rental_usd_hr < cfg.rental_revenue_usd_hr


def test_simulate_workload_scenarios() -> None:
    tariff = _make_sample_tariff()
    fx = _make_sample_fx()
    rows = simulate_workload_scenarios(
        current_wall_watts=110.0,
        cpu_tdp_watts=120.0,
        gpu_tdp_watts=360.0,
        psu_efficiency=0.90,
        tariff=tariff,
        fx=fx,
        rental_usd_hr=0.42,
    )
    assert len(rows) == 5
    assert rows[0].wall_watts == 110.0
    assert all(r.kwh_per_day > 0 for r in rows)
    assert all(r.cost_day_uah > 0 for r in rows)
