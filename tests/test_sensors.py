"""
Tests for HardwareMonitor telemetry collection and session accumulation.
"""

from __future__ import annotations

import time
from pathlib import Path

from telemetria.config import AppConfig
from telemetria.sensors import HardwareMonitor, _detect_cpu_tdp_and_profile


def test_cpu_tdp_detection_x3d() -> None:
    tdp, ppt, idle = _detect_cpu_tdp_and_profile(
        "AMD Ryzen 7 7800X3D 8-Core Processor", 16
    )
    assert tdp == 120.0
    assert ppt == 88.0
    assert idle > 15.0


def test_hardware_monitor_sample_and_session(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("TELEMETRIA_HOME", str(tmp_path))
    cfg = AppConfig()
    monitor = HardwareMonitor(config=cfg)
    try:
        snap1 = monitor.sample(offline=True, include_processes=True)
        time.sleep(0.08)
        snap2 = monitor.sample(offline=True, include_processes=True)

        assert snap2.cpu.logical_cores >= 1
        assert snap2.cpu.power_watts > 0
        assert snap2.cpu.temperature_c > 20.0
        assert snap2.ram.total_gb > 0
        assert snap2.power.total_wall_watts > snap2.power.total_dc_watts
        assert snap2.session.samples_count == 2
        assert snap2.session.energy_wh >= snap1.session.energy_wh
        assert snap2.session.cost_uah >= 0.0
    finally:
        monitor.close()
