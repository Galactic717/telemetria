"""
Configuration management for Telemetria (~/.telemetria/config.json).
"""

from __future__ import annotations

import json
import os
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Dict, Optional


PSU_EFFICIENCY_MAP: Dict[str, float] = {
    "80+ Titanium": 0.94,
    "80+ Platinum": 0.92,
    "80+ Gold": 0.90,
    "80+ Silver": 0.88,
    "80+ Bronze": 0.85,
    "80+ White": 0.80,
}

WORKLOAD_PRESETS: Dict[str, Dict[str, Any]] = {
    "vastai_rtx5080": {
        "name": "Vast.ai / RunPod GPU Server (RTX 5080 / High-End AI)",
        "description": "Здача ПК в оренду під AI-інференс та навчання (LLM, Stable Diffusion, ComfyUI)",
        "default_revenue_usd_hr": 0.42,
    },
    "render_network": {
        "name": "Render Network / Octane / Blender 3D Farm",
        "description": "Розподілений GPU-рендеринг сцен та відео коли ПК простоює",
        "default_revenue_usd_hr": 0.36,
    },
    "salad_container": {
        "name": "Salad.com / IO.net Idle Compute Container",
        "description": "Автоматична здача потужностей GPU/CPU/RAM у хмарні контейнери під час простою",
        "default_revenue_usd_hr": 0.28,
    },
    "nicehash_mining": {
        "name": "NiceHash / Crypto PoW GPU+CPU Mining",
        "description": "Класичний майнінг алгоритмів (KawPow / KHeavyHash / RandomX)",
        "default_revenue_usd_hr": 0.19,
    },
    "vps_dedicated": {
        "name": "Dedicated Game / Web / Docker Host Server",
        "description": "Оренда як виділеного сервера (Ryzen 7 7800X3D + 64GB DDR5 + NVMe)",
        "default_revenue_usd_hr": 0.24,
    },
    "custom": {
        "name": "Custom Server Rental / Mining Profile",
        "description": "Власний тариф доходу від оренди сервера або майнінгу",
        "default_revenue_usd_hr": 0.35,
    },
}


@dataclass
class AppConfig:
    tariff_profile: str = "ua_two_zone"
    custom_tariff_uah_kwh: float = 4.32
    psu_rating: str = "80+ Gold"
    psu_efficiency: float = 0.90
    motherboard_base_watts: float = 22.0
    workload_profile: str = "vastai_rtx5080"
    rental_revenue_usd_hr: float = 0.42
    auto_scale_revenue_by_load: bool = False
    refresh_interval_sec: float = 1.0
    tariff_cache_ttl_sec: int = 1800
    cpu_temp_warn_c: float = 80.0
    gpu_temp_warn_c: float = 78.0
    preferred_currency: str = "UAH"

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


def get_app_dir() -> Path:
    env_dir = os.environ.get("TELEMETRIA_HOME")
    if env_dir:
        base = Path(env_dir)
    else:
        base = Path.home() / ".telemetria"
    base.mkdir(parents=True, exist_ok=True)
    return base


def get_config_path() -> Path:
    return get_app_dir() / "config.json"


def load_config() -> AppConfig:
    cfg_path = get_config_path()
    if not cfg_path.exists():
        cfg = AppConfig()
        save_config(cfg)
        return cfg
    try:
        raw = json.loads(cfg_path.read_text(encoding="utf-8"))
        valid_keys = {f.name for f in AppConfig.__dataclass_fields__.values()}
        filtered = {k: v for k, v in raw.items() if k in valid_keys}
        cfg = AppConfig(**filtered)
        if cfg.psu_rating in PSU_EFFICIENCY_MAP:
            cfg.psu_efficiency = PSU_EFFICIENCY_MAP[cfg.psu_rating]
        return cfg
    except Exception:
        return AppConfig()


def save_config(cfg: AppConfig) -> Path:
    if cfg.psu_rating in PSU_EFFICIENCY_MAP:
        cfg.psu_efficiency = PSU_EFFICIENCY_MAP[cfg.psu_rating]
    cfg_path = get_config_path()
    cfg_path.parent.mkdir(parents=True, exist_ok=True)
    cfg_path.write_text(json.dumps(cfg.to_dict(), indent=2, ensure_ascii=False), encoding="utf-8")
    return cfg_path
