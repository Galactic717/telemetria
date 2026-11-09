"""
Unit tests for LiveTariffService, Two-Zone / Three-Zone time-of-day tariff schedules, and caching.
"""

from __future__ import annotations

import datetime as dt
import os
from pathlib import Path

from telemetria.config import AppConfig
from telemetria.tariffs import LiveTariffService


def test_two_zone_day_and_night_switching(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("TELEMETRIA_HOME", str(tmp_path))
    cfg = AppConfig(tariff_profile="ua_two_zone")
    svc = LiveTariffService(cfg)

    day_dt = dt.datetime(2026, 11, 9, 14, 30, 0)
    day_profile = svc.get_tariff_profile("ua_two_zone", now=day_dt, offline=True)
    assert day_profile.is_night_discount_active is False
    assert abs(day_profile.current_rate_uah_kwh - 4.32) < 1e-3

    night_dt = dt.datetime(2026, 11, 9, 2, 15, 0)
    night_profile = svc.get_tariff_profile("ua_two_zone", now=night_dt, offline=True)
    assert night_profile.is_night_discount_active is True
    assert abs(night_profile.current_rate_uah_kwh - 2.16) < 1e-3


def test_three_zone_peak_day_night(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("TELEMETRIA_HOME", str(tmp_path))
    cfg = AppConfig(tariff_profile="ua_three_zone")
    svc = LiveTariffService(cfg)

    peak_dt = dt.datetime(2026, 11, 9, 9, 30, 0)
    peak_p = svc.get_tariff_profile("ua_three_zone", now=peak_dt, offline=True)
    assert abs(peak_p.current_rate_uah_kwh - 6.48) < 1e-3

    half_peak_dt = dt.datetime(2026, 11, 9, 15, 0, 0)
    half_p = svc.get_tariff_profile("ua_three_zone", now=half_peak_dt, offline=True)
    assert abs(half_p.current_rate_uah_kwh - 4.32) < 1e-3

    night_dt = dt.datetime(2026, 11, 9, 23, 30, 0)
    night_p = svc.get_tariff_profile("ua_three_zone", now=night_dt, offline=True)
    assert abs(night_p.current_rate_uah_kwh - 1.728) < 1e-3


def test_list_all_profiles_and_custom_rate(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("TELEMETRIA_HOME", str(tmp_path))
    cfg = AppConfig(tariff_profile="custom", custom_tariff_uah_kwh=5.50)
    svc = LiveTariffService(cfg)

    profiles = svc.list_all_profiles(offline=True)
    assert len(profiles) == 8
    custom_p = next(p for p in profiles if p.profile_id == "custom")
    assert abs(custom_p.current_rate_uah_kwh - 5.50) < 1e-3
