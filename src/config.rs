use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

pub const PSU_PROFILES: &[(&str, f64)] = &[
    ("80+ Titanium", 0.94),
    ("80+ Platinum", 0.92),
    ("80+ Gold", 0.90),
    ("80+ Silver", 0.88),
    ("80+ Bronze", 0.85),
    ("80+ White", 0.80),
];

pub const TARIFF_PROFILE_IDS: &[&str] = &[
    "ua_two_zone",
    "ua_single",
    "ua_three_zone",
    "ua_heating",
    "ua_commercial",
    "eu_spot_pl",
    "eu_spot_de",
    "custom",
];

#[derive(Debug, Clone)]
pub struct WorkloadPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub default_revenue_usd_hr: f64,
}

pub const WORKLOAD_PRESETS: &[WorkloadPreset] = &[
    WorkloadPreset {
        id: "vastai_rtx5080",
        name: "Vast.ai / RunPod GPU Server (RTX 5080 / High-End AI)",
        description: "Здача ПК в оренду під AI-інференс та навчання (LLM, Stable Diffusion, ComfyUI)",
        default_revenue_usd_hr: 0.42,
    },
    WorkloadPreset {
        id: "render_network",
        name: "Render Network / Octane / Blender 3D Farm",
        description: "Розподілений GPU-рендеринг сцен та відео коли ПК простоює",
        default_revenue_usd_hr: 0.36,
    },
    WorkloadPreset {
        id: "salad_container",
        name: "Salad.com / IO.net Idle Compute Container",
        description: "Автоматична здача потужностей GPU/CPU/RAM у хмарні контейнери під час простою",
        default_revenue_usd_hr: 0.28,
    },
    WorkloadPreset {
        id: "nicehash_mining",
        name: "NiceHash / Crypto PoW GPU+CPU Mining",
        description: "Класичний майнінг алгоритмів (KawPow / KHeavyHash / RandomX)",
        default_revenue_usd_hr: 0.19,
    },
    WorkloadPreset {
        id: "vps_dedicated",
        name: "Dedicated Game / Web / Docker Host Server",
        description: "Оренда як виділеного сервера (Ryzen 7 7800X3D + 64GB DDR5 + NVMe)",
        default_revenue_usd_hr: 0.24,
    },
    WorkloadPreset {
        id: "custom",
        name: "Custom Server Rental / Mining Profile",
        description: "Власний тариф доходу від оренди сервера або майнінгу",
        default_revenue_usd_hr: 0.35,
    },
];

pub fn get_workload_preset(id: &str) -> &'static WorkloadPreset {
    WORKLOAD_PRESETS
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(id))
        .unwrap_or(&WORKLOAD_PRESETS[0])
}

pub fn lookup_psu_efficiency(rating: &str) -> Option<(&'static str, f64)> {
    PSU_PROFILES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(rating.trim()))
        .copied()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub tariff_profile: String,
    pub custom_tariff_uah_kwh: f64,
    pub psu_rating: String,
    pub psu_efficiency: f64,
    pub motherboard_base_watts: f64,
    pub workload_profile: String,
    pub rental_revenue_usd_hr: f64,
    pub refresh_interval_sec: f64,
    pub tariff_cache_ttl_sec: u64,
    pub cpu_temp_warn_c: f64,
    pub gpu_temp_warn_c: f64,
    pub preferred_currency: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tariff_profile: "ua_two_zone".to_string(),
            custom_tariff_uah_kwh: 4.32,
            psu_rating: "80+ Gold".to_string(),
            psu_efficiency: 0.90,
            motherboard_base_watts: 22.0,
            workload_profile: "vastai_rtx5080".to_string(),
            rental_revenue_usd_hr: 0.42,
            refresh_interval_sec: 1.0,
            tariff_cache_ttl_sec: 60,
            cpu_temp_warn_c: 80.0,
            gpu_temp_warn_c: 78.0,
            preferred_currency: "UAH".to_string(),
        }
    }
}

impl AppConfig {
    pub fn cycle_tariff_profile(&mut self) {
        let idx = TARIFF_PROFILE_IDS
            .iter()
            .position(|&p| p == self.tariff_profile)
            .unwrap_or(0);
        let next_idx = (idx + 1) % TARIFF_PROFILE_IDS.len();
        self.tariff_profile = TARIFF_PROFILE_IDS[next_idx].to_string();
    }

    pub fn cycle_workload_profile(&mut self) {
        let idx = WORKLOAD_PRESETS
            .iter()
            .position(|p| p.id == self.workload_profile)
            .unwrap_or(0);
        let next_idx = (idx + 1) % WORKLOAD_PRESETS.len();
        let preset = &WORKLOAD_PRESETS[next_idx];
        self.workload_profile = preset.id.to_string();
        self.rental_revenue_usd_hr = preset.default_revenue_usd_hr;
    }

    pub fn cycle_psu_rating(&mut self) {
        let idx = PSU_PROFILES
            .iter()
            .position(|(name, _)| *name == self.psu_rating)
            .unwrap_or(2);
        let next_idx = (idx + 1) % PSU_PROFILES.len();
        let (name, eff) = PSU_PROFILES[next_idx];
        self.psu_rating = name.to_string();
        self.psu_efficiency = eff;
    }
}

pub fn get_app_dir() -> PathBuf {
    if let Ok(custom) = env::var("TELEMETRIA_HOME") {
        let p = PathBuf::from(custom);
        let _ = fs::create_dir_all(&p);
        return p;
    }
    let base = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".telemetria");
    let _ = fs::create_dir_all(&base);
    base
}

pub fn get_config_path() -> PathBuf {
    get_app_dir().join("config.json")
}

pub fn load_config() -> AppConfig {
    let path = get_config_path();
    if !path.exists() {
        let cfg = AppConfig::default();
        let _ = save_config(&cfg);
        return cfg;
    }
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let mut cfg: AppConfig = serde_json::from_str(&raw).unwrap_or_default();
            if let Some((canonical, eff)) = lookup_psu_efficiency(&cfg.psu_rating) {
                cfg.psu_rating = canonical.to_string();
                cfg.psu_efficiency = eff;
            }
            cfg
        }
        Err(_) => AppConfig::default(),
    }
}

pub fn save_config(cfg: &AppConfig) -> PathBuf {
    let path = get_config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = fs::write(&path, json);
    }
    path
}
