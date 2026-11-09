use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub name: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub usage_percent: f64,
    pub per_core_percent: Vec<f64>,
    pub frequency_current_mhz: f64,
    pub frequency_max_mhz: f64,
    pub temperature_c: f64,
    pub temp_source: String,
    pub power_watts: f64,
    pub tdp_watts: f64,
    pub power_source: String,
    pub throttling: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuProcessInfo {
    pub pid: u32,
    pub name: String,
    pub vram_mb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuMetrics {
    pub index: u32,
    pub name: String,
    pub uuid: String,
    pub vendor: String,
    pub usage_percent: f64,
    pub memory_usage_percent: f64,
    pub vram_used_mb: f64,
    pub vram_total_mb: f64,
    pub temperature_c: f64,
    pub fan_speed_percent: f64,
    pub power_watts: f64,
    pub power_limit_watts: f64,
    pub energy_total_kwh: Option<f64>,
    pub clock_core_mhz: f64,
    pub clock_max_core_mhz: f64,
    pub clock_mem_mhz: f64,
    pub pstate: String,
    pub pcie_gen: u32,
    pub pcie_width: u32,
    pub pcie_tx_mbs: f64,
    pub pcie_rx_mbs: f64,
    pub throttling: bool,
    pub throttle_reason: String,
    pub source: String,
    pub processes: Vec<GpuProcessInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamMetrics {
    pub total_gb: f64,
    pub used_gb: f64,
    pub available_gb: f64,
    pub usage_percent: f64,
    pub swap_total_gb: f64,
    pub swap_used_gb: f64,
    pub swap_percent: f64,
    pub dimm_count: u32,
    pub speed_mts: u32,
    pub memory_type: String,
    pub part_number: String,
    pub power_watts: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskPartitionInfo {
    pub device: String,
    pub mountpoint: String,
    pub fstype: String,
    pub total_gb: f64,
    pub used_gb: f64,
    pub free_gb: f64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskMetrics {
    pub model_name: String,
    pub media_type: String,
    pub health_status: String,
    pub read_speed_mbs: f64,
    pub write_speed_mbs: f64,
    pub total_read_gb: f64,
    pub total_written_gb: f64,
    pub power_watts: f64,
    pub partitions: Vec<DiskPartitionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub upload_speed_mbs: f64,
    pub download_speed_mbs: f64,
    pub total_sent_gb: f64,
    pub total_recv_gb: f64,
    pub active_interfaces: usize,
    pub primary_interface: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessPowerInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub estimated_watts: f64,
    pub cost_uah_per_day: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerBreakdown {
    pub cpu_watts: f64,
    pub gpu_watts: f64,
    pub ram_watts: f64,
    pub disk_watts: f64,
    pub motherboard_fans_watts: f64,
    pub total_dc_watts: f64,
    pub psu_rating: String,
    pub psu_efficiency: f64,
    pub psu_loss_watts: f64,
    pub total_wall_watts: f64,
    pub total_wall_kw: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRates {
    pub usd_to_uah: f64,
    pub eur_to_uah: f64,
    pub pln_to_uah: f64,
    pub eur_to_usd: f64,
    pub updated_at: String,
    pub source: String,
    pub is_live: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TariffProfile {
    pub profile_id: String,
    pub name: String,
    pub country: String,
    pub currency: String,
    pub current_rate_uah_kwh: f64,
    pub day_rate_uah_kwh: f64,
    pub night_rate_uah_kwh: f64,
    pub peak_rate_uah_kwh: f64,
    pub current_zone_label: String,
    pub is_night_discount_active: bool,
    pub schedule_description: String,
    pub source: String,
    pub updated_at: String,
    pub is_live: bool,
}

impl TariffProfile {
    pub fn rate_in_usd(&self, fx: &ExchangeRates) -> f64 {
        self.current_rate_uah_kwh / fx.usd_to_uah.max(0.01)
    }

    pub fn rate_in_eur(&self, fx: &ExchangeRates) -> f64 {
        self.current_rate_uah_kwh / fx.eur_to_uah.max(0.01)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoMarketSnapshot {
    pub btc_usd: f64,
    pub eth_usd: f64,
    pub sol_usd: f64,
    pub render_usd: f64,
    pub updated_at: String,
    pub source: String,
    pub is_live: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningRentalMetrics {
    pub workload_id: String,
    pub workload_profile: String,
    pub workload_description: String,
    pub gross_revenue_usd_hr: f64,
    pub gross_revenue_uah_hr: f64,
    pub electricity_cost_uah_hr: f64,
    pub electricity_cost_usd_hr: f64,
    pub net_profit_uah_hr: f64,
    pub net_profit_usd_hr: f64,
    pub net_profit_uah_day_24h: f64,
    pub net_profit_usd_day_24h: f64,
    pub net_profit_uah_month_30d: f64,
    pub net_profit_usd_month_30d: f64,
    pub night_only_profit_uah_8h: f64,
    pub electricity_share_percent: f64,
    pub break_even_tariff_uah_kwh: f64,
    pub break_even_rental_usd_hr: f64,
    pub is_profitable_now: bool,
    pub efficiency_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadScenarioRow {
    pub scenario_name: String,
    pub wall_watts: f64,
    pub kwh_per_day: f64,
    pub cost_day_uah: f64,
    pub cost_month_uah: f64,
    pub gross_day_uah: f64,
    pub net_day_uah: f64,
    pub net_month_uah: f64,
    pub net_month_usd: f64,
    pub night_8h_net_month_uah: f64,
    pub roi_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAccumulator {
    pub session_id: String,
    pub started_at: String,
    pub updated_at: String,
    pub hostname: String,
    pub elapsed_seconds: f64,
    pub energy_wh: f64,
    pub energy_kwh: f64,
    pub cost_uah: f64,
    pub cost_usd: f64,
    pub cost_eur: f64,
    pub gross_revenue_uah: f64,
    pub gross_revenue_usd: f64,
    pub net_profit_uah: f64,
    pub net_profit_usd: f64,
    pub peak_wall_watts: f64,
    pub avg_wall_watts: f64,
    pub peak_cpu_temp_c: f64,
    pub peak_gpu_temp_c: f64,
    pub tariff_profile: String,
    pub workload_profile: String,
    pub samples_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub timestamp: String,
    pub hostname: String,
    pub os_name: String,
    pub motherboard: String,
    pub uptime_seconds: u64,
    pub sample_latency_us: u64,
    pub cpu: CpuMetrics,
    pub gpus: Vec<GpuMetrics>,
    pub ram: RamMetrics,
    pub disk: DiskMetrics,
    pub network: NetworkMetrics,
    pub power: PowerBreakdown,
    pub tariff: TariffProfile,
    pub fx: ExchangeRates,
    pub crypto: CryptoMarketSnapshot,
    pub mining: MiningRentalMetrics,
    pub session: SessionAccumulator,
    pub top_processes: Vec<ProcessPowerInfo>,
}
