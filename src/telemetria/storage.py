"""
SQLite persistence engine for Telemetria (~/.telemetria/telemetry.db).
Records sessions, historical electricity consumption (Wh/kWh), money spent (UAH/USD),
and server rental / mining revenue.
"""

from __future__ import annotations

import csv
import json
import sqlite3
from pathlib import Path
from typing import Any, Dict, List, Optional

from telemetria.config import get_app_dir
from telemetria.models import SystemSnapshot


def get_db_path() -> Path:
    return get_app_dir() / "telemetry.db"


class TelemetryStorage:
    def __init__(self, db_path: Optional[Path] = None) -> None:
        self.db_path = db_path or get_db_path()
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._init_schema()

    def _connect(self) -> sqlite3.Connection:
        conn = sqlite3.connect(str(self.db_path))
        conn.row_factory = sqlite3.Row
        return conn

    def _init_schema(self) -> None:
        with self._connect() as conn:
            conn.execute(
                """
                CREATE TABLE IF NOT EXISTS sessions (
                    session_id TEXT PRIMARY KEY,
                    started_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    hostname TEXT NOT NULL,
                    elapsed_seconds REAL NOT NULL,
                    energy_wh REAL NOT NULL,
                    energy_kwh REAL NOT NULL,
                    cost_uah REAL NOT NULL,
                    cost_usd REAL NOT NULL,
                    gross_revenue_uah REAL NOT NULL,
                    net_profit_uah REAL NOT NULL,
                    avg_wall_watts REAL NOT NULL,
                    peak_wall_watts REAL NOT NULL,
                    peak_cpu_temp_c REAL NOT NULL,
                    peak_gpu_temp_c REAL NOT NULL,
                    tariff_profile TEXT NOT NULL,
                    workload_profile TEXT NOT NULL,
                    samples_count INTEGER NOT NULL
                )
                """
            )
            conn.execute(
                """
                CREATE TABLE IF NOT EXISTS samples (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    timestamp TEXT NOT NULL,
                    cpu_usage_pct REAL NOT NULL,
                    cpu_temp_c REAL NOT NULL,
                    cpu_watts REAL NOT NULL,
                    gpu_usage_pct REAL NOT NULL,
                    gpu_temp_c REAL NOT NULL,
                    gpu_watts REAL NOT NULL,
                    ram_used_gb REAL NOT NULL,
                    wall_watts REAL NOT NULL,
                    tariff_uah_kwh REAL NOT NULL,
                    cost_uah_hr REAL NOT NULL,
                    net_profit_uah_hr REAL NOT NULL
                )
                """
            )
            conn.commit()

    def record_snapshot(
        self, snap: SystemSnapshot, record_tick: bool = True
    ) -> None:
        s = snap.session
        gpu_usage = snap.gpus[0].usage_percent if snap.gpus else 0.0
        gpu_temp = snap.gpus[0].temperature_c if snap.gpus else 0.0
        gpu_watts = sum(g.power_watts for g in snap.gpus)

        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO sessions (
                    session_id, started_at, updated_at, hostname,
                    elapsed_seconds, energy_wh, energy_kwh,
                    cost_uah, cost_usd, gross_revenue_uah, net_profit_uah,
                    avg_wall_watts, peak_wall_watts,
                    peak_cpu_temp_c, peak_gpu_temp_c,
                    tariff_profile, workload_profile, samples_count
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(session_id) DO UPDATE SET
                    updated_at = excluded.updated_at,
                    elapsed_seconds = excluded.elapsed_seconds,
                    energy_wh = excluded.energy_wh,
                    energy_kwh = excluded.energy_kwh,
                    cost_uah = excluded.cost_uah,
                    cost_usd = excluded.cost_usd,
                    gross_revenue_uah = excluded.gross_revenue_uah,
                    net_profit_uah = excluded.net_profit_uah,
                    avg_wall_watts = excluded.avg_wall_watts,
                    peak_wall_watts = excluded.peak_wall_watts,
                    peak_cpu_temp_c = excluded.peak_cpu_temp_c,
                    peak_gpu_temp_c = excluded.peak_gpu_temp_c,
                    tariff_profile = excluded.tariff_profile,
                    workload_profile = excluded.workload_profile,
                    samples_count = excluded.samples_count
                """,
                (
                    s.session_id,
                    s.started_at,
                    snap.timestamp,
                    snap.hostname,
                    s.elapsed_seconds,
                    s.energy_wh,
                    s.energy_kwh,
                    s.cost_uah,
                    s.cost_usd,
                    s.gross_revenue_uah,
                    s.net_profit_uah,
                    s.avg_wall_watts,
                    s.peak_wall_watts,
                    s.peak_cpu_temp_c,
                    s.peak_gpu_temp_c,
                    snap.tariff.profile_id,
                    snap.mining.workload_profile,
                    s.samples_count,
                ),
            )
            if record_tick:
                conn.execute(
                    """
                    INSERT INTO samples (
                        session_id, timestamp, cpu_usage_pct, cpu_temp_c, cpu_watts,
                        gpu_usage_pct, gpu_temp_c, gpu_watts, ram_used_gb,
                        wall_watts, tariff_uah_kwh, cost_uah_hr, net_profit_uah_hr
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    (
                        s.session_id,
                        snap.timestamp,
                        snap.cpu.usage_percent,
                        snap.cpu.temperature_c,
                        snap.cpu.power_watts,
                        gpu_usage,
                        gpu_temp,
                        gpu_watts,
                        snap.ram.used_gb,
                        snap.power.total_wall_watts,
                        snap.tariff.current_rate_uah_kwh,
                        snap.mining.electricity_cost_uah_hr,
                        snap.mining.net_profit_uah_hr,
                    ),
                )
            conn.commit()

    def list_sessions(self, limit: int = 20) -> List[Dict[str, Any]]:
        with self._connect() as conn:
            rows = conn.execute(
                """
                SELECT * FROM sessions
                ORDER BY updated_at DESC
                LIMIT ?
                """,
                (limit,),
            ).fetchall()
            return [dict(r) for r in rows]

    def get_lifetime_summary(self) -> Dict[str, Any]:
        with self._connect() as conn:
            row = conn.execute(
                """
                SELECT
                    COUNT(*) AS total_sessions,
                    COALESCE(SUM(elapsed_seconds), 0.0) AS total_seconds,
                    COALESCE(SUM(energy_wh), 0.0) AS total_energy_wh,
                    COALESCE(SUM(energy_kwh), 0.0) AS total_energy_kwh,
                    COALESCE(SUM(cost_uah), 0.0) AS total_cost_uah,
                    COALESCE(SUM(cost_usd), 0.0) AS total_cost_usd,
                    COALESCE(SUM(gross_revenue_uah), 0.0) AS total_gross_uah,
                    COALESCE(SUM(net_profit_uah), 0.0) AS total_net_uah,
                    COALESCE(AVG(avg_wall_watts), 0.0) AS mean_wall_watts,
                    COALESCE(MAX(peak_wall_watts), 0.0) AS max_wall_watts,
                    COALESCE(MAX(peak_cpu_temp_c), 0.0) AS max_cpu_temp_c,
                    COALESCE(MAX(peak_gpu_temp_c), 0.0) AS max_gpu_temp_c
                FROM sessions
                """
            ).fetchone()
            return dict(row) if row else {}

    def export_sessions_csv(self, output_path: Path) -> Path:
        sessions = self.list_sessions(limit=1000)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        fieldnames = [
            "session_id",
            "started_at",
            "updated_at",
            "hostname",
            "elapsed_seconds",
            "energy_wh",
            "energy_kwh",
            "cost_uah",
            "cost_usd",
            "gross_revenue_uah",
            "net_profit_uah",
            "avg_wall_watts",
            "peak_wall_watts",
            "peak_cpu_temp_c",
            "peak_gpu_temp_c",
            "tariff_profile",
            "workload_profile",
            "samples_count",
        ]
        with output_path.open("w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=fieldnames)
            writer.writeheader()
            for s in sessions:
                writer.writerow({k: s.get(k) for k in fieldnames})
        return output_path

    def export_sessions_json(self, output_path: Path) -> Path:
        payload = {
            "summary": self.get_lifetime_summary(),
            "sessions": self.list_sessions(limit=1000),
        }
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(
            json.dumps(payload, indent=2, ensure_ascii=False), encoding="utf-8"
        )
        return output_path
