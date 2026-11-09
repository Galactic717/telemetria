"""
Tests for SQLite TelemetryStorage ledger and CSV/JSON exports.
"""

from __future__ import annotations

import json
from pathlib import Path

from telemetria.config import AppConfig
from telemetria.sensors import HardwareMonitor
from telemetria.storage import TelemetryStorage


def test_sqlite_storage_and_exports(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("TELEMETRIA_HOME", str(tmp_path))
    storage = TelemetryStorage(db_path=tmp_path / "telemetry.db")
    monitor = HardwareMonitor(config=AppConfig())
    try:
        snap = monitor.sample(offline=True, include_processes=False)
        storage.record_snapshot(snap, record_tick=True)

        sessions = storage.list_sessions(limit=5)
        assert len(sessions) == 1
        assert sessions[0]["session_id"] == snap.session.session_id

        summary = storage.get_lifetime_summary()
        assert int(summary["total_sessions"]) == 1
        assert float(summary["mean_wall_watts"]) > 0

        csv_file = tmp_path / "out.csv"
        json_file = tmp_path / "out.json"
        storage.export_sessions_csv(csv_file)
        storage.export_sessions_json(json_file)

        assert csv_file.exists()
        assert snap.session.session_id in csv_file.read_text(encoding="utf-8")

        assert json_file.exists()
        parsed = json.loads(json_file.read_text(encoding="utf-8"))
        assert len(parsed["sessions"]) == 1
    finally:
        monitor.close()
