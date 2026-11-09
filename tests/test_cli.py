"""
End-to-end CLI tests for Telemetria.
"""

from __future__ import annotations

import json
from pathlib import Path
from typer.testing import CliRunner

from telemetria.cli import app

runner = CliRunner()


def test_cli_version() -> None:
    res = runner.invoke(app, ["--version"])
    assert res.exit_code == 0
    assert "Telemetria" in res.stdout


def test_cli_status_json(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("TELEMETRIA_HOME", str(tmp_path))
    res = runner.invoke(app, ["status", "--offline", "--json"])
    assert res.exit_code == 0
    payload = json.loads(res.stdout)
    assert "cpu" in payload
    assert "power" in payload
    assert "tariff" in payload
    assert "mining" in payload


def test_cli_tariffs_and_mining(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("TELEMETRIA_HOME", str(tmp_path))
    res_t = runner.invoke(
        app, ["tariffs", "--set", "ua_two_zone", "--offline", "--json"]
    )
    assert res_t.exit_code == 0
    t_data = json.loads(res_t.stdout)
    assert t_data["active_profile"] == "ua_two_zone"
    assert len(t_data["profiles"]) == 8

    res_m = runner.invoke(
        app,
        [
            "mining",
            "--profile",
            "vastai_rtx5080",
            "--rate-usd",
            "0.50",
            "--offline",
            "--json",
        ],
    )
    assert res_m.exit_code == 0
    m_data = json.loads(res_m.stdout)
    assert abs(m_data["mining_metrics"]["gross_revenue_usd_hr"] - 0.50) < 1e-3
    assert len(m_data["scenarios"]) == 5


def test_cli_history_and_export(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("TELEMETRIA_HOME", str(tmp_path))
    runner.invoke(app, ["status", "--offline"])

    res_h = runner.invoke(app, ["history", "--json"])
    assert res_h.exit_code == 0
    h_data = json.loads(res_h.stdout)
    assert int(h_data["summary"]["total_sessions"]) >= 1

    csv_out = tmp_path / "report.csv"
    res_e = runner.invoke(
        app, ["export", "--format", "csv", "--output", str(csv_out)]
    )
    assert res_e.exit_code == 0
    assert csv_out.exists()
