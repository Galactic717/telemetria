"""
Data models for hardware telemetry, power consumption, electricity tariffs, and mining/server rental economics.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field
from typing import Any, Dict, List, Optional


@dataclass
class CpuMetrics:
    name: str
    physical_cores: int
    logical_cores: int
    usage_percent: float
    per_core_percent: List[float]
    frequency_current_mhz: float
    frequency_max_mhz: float
    temperature_c: float
    temp_source: str
    power_watts: float
    tdp_watts: float
    power_source: str
    throttling: bool = False


@dataclass
class GpuProcessInfo:
    pid: int
    name: str
    vram_mb: float


@dataclass
class GpuMetrics:
    index: int
    name: str
    uuid: str
    vendor: str
    usage_percent: float
    memory_usage_percent: float
    vram_used_mb: float
    vram_total_mb: float
    temperature_c: float
    fan_speed_percent: float
    power_watts: float
    power_limit_watts: float
    energy_total_kwh: Optional[float]
    clock_core_mhz: float
    clock_max_core_mhz: float
    clock_mem_mhz: float
    pstate: str
    pcie_gen: int
    pcie_width: int
    throttling: bool = False
    throttle_reason: str = "None"
    source: str = "NVML"
    processes: List[GpuProcessInfo] = field(default_factory=list)


@dataclass
class RamMetrics:
    total_gb: float
    used_gb: float
    available_gb: float
    usage_percent: float
    swap_total_gb: float
    swap_used_gb: float
    swap_percent: float
    dimm_count: int
    speed_mts: int
    memory_type: str
    part_number: str
    power_watts: float


@dataclass
class DiskPartitionInfo:
    device: str
    mountpoint: str
    fstype: str
    total_gb: float
    used_gb: float
    free_gb: float
    usage_percent: float


@dataclass
class DiskMetrics:
    model_name: str
    media_type: str
    health_status: str
    read_speed_mbs: float
    write_speed_mbs: float
    total_read_gb: float
    total_written_gb: float
    power_watts: float
    partitions: List[DiskPartitionInfo] = field(default_factory=list)


@dataclass
class NetworkMetrics:
    upload_speed_mbs: float
    download_speed_mbs: float
    total_sent_gb: float
    total_recv_gb: float
    active_connections: int
    primary_interface: str


@dataclass
class ProcessPowerInfo:
    pid: int
    name: str
    cpu_percent: float
    memory_mb: float
    estimated_watts: float
    cost_uah_per_day: float


@dataclass
class PowerBreakdown:
    cpu_watts: float
    gpu_watts: float
    ram_watts: float
    disk_watts: float
    motherboard_fans_watts: float
    total_dc_watts: float
    psu_rating: str
    psu_efficiency: float
    psu_loss_watts: float
    total_wall_watts: float
    total_wall_kw: float


@dataclass
class ExchangeRates:
    usd_to_uah: float
    eur_to_uah: float
    pln_to_uah: float
    eur_to_usd: float
    updated_at: str
    source: str
    is_live: bool


@dataclass
class TariffProfile:
    profile_id: str
    name: str
    country: str
    currency: str
    current_rate_uah_kwh: float
    day_rate_uah_kwh: float
    night_rate_uah_kwh: float
    peak_rate_uah_kwh: float
    current_zone_label: str
    is_night_discount_active: bool
    schedule_description: str
    source: str
    updated_at: str
    is_live: bool

    def rate_in_usd(self, fx: ExchangeRates) -> float:
        return self.current_rate_uah_kwh / max(fx.usd_to_uah, 0.01)

    def rate_in_eur(self, fx: ExchangeRates) -> float:
        return self.current_rate_uah_kwh / max(fx.eur_to_uah, 0.01)


@dataclass
class CryptoMarketSnapshot:
    btc_usd: float
    eth_usd: float
    sol_usd: float
    render_usd: float
    updated_at: str
    source: str
    is_live: bool


@dataclass
class MiningRentalMetrics:
    workload_profile: str
    workload_description: str
    gross_revenue_usd_hr: float
    gross_revenue_uah_hr: float
    electricity_cost_uah_hr: float
    electricity_cost_usd_hr: float
    net_profit_uah_hr: float
    net_profit_usd_hr: float
    net_profit_uah_day_24h: float
    net_profit_usd_day_24h: float
    net_profit_uah_month_30d: float
    net_profit_usd_month_30d: float
    night_only_profit_uah_8h: float
    electricity_share_percent: float
    break_even_tariff_uah_kwh: float
    break_even_rental_usd_hr: float
    is_profitable_now: bool
    efficiency_label: str


@dataclass
class SessionAccumulator:
    session_id: str
    started_at: str
    elapsed_seconds: float
    energy_wh: float
    energy_kwh: float
    cost_uah: float
    cost_usd: float
    cost_eur: float
    gross_revenue_uah: float
    gross_revenue_usd: float
    net_profit_uah: float
    net_profit_usd: float
    peak_wall_watts: float
    avg_wall_watts: float
    peak_cpu_temp_c: float
    peak_gpu_temp_c: float
    samples_count: int


@dataclass
class SystemSnapshot:
    timestamp: str
    hostname: str
    os_name: str
    motherboard: str
    uptime_seconds: float
    cpu: CpuMetrics
    gpus: List[GpuMetrics]
    ram: RamMetrics
    disk: DiskMetrics
    network: NetworkMetrics
    power: PowerBreakdown
    tariff: TariffProfile
    fx: ExchangeRates
    crypto: CryptoMarketSnapshot
    mining: MiningRentalMetrics
    session: SessionAccumulator
    top_processes: List[ProcessPowerInfo] = field(default_factory=list)

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)
