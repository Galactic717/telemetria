use crate::config::get_app_dir;
use crate::economics::{round2, round4, round5};
use crate::models::{SessionAccumulator, SystemSnapshot};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LifetimeSummary {
    pub total_sessions: usize,
    pub total_seconds: f64,
    pub total_energy_wh: f64,
    pub total_energy_kwh: f64,
    pub total_cost_uah: f64,
    pub total_cost_usd: f64,
    pub total_gross_uah: f64,
    pub total_net_uah: f64,
    pub mean_wall_watts: f64,
    pub max_wall_watts: f64,
    pub max_cpu_temp_c: f64,
    pub max_gpu_temp_c: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LedgerData {
    sessions: Vec<SessionAccumulator>,
}

pub fn get_ledger_path() -> PathBuf {
    get_app_dir().join("ledger.json")
}

pub struct TelemetryStorage {
    path: PathBuf,
}

impl Default for TelemetryStorage {
    fn default() -> Self {
        Self::new(None)
    }
}

impl TelemetryStorage {
    pub fn new(custom_path: Option<PathBuf>) -> Self {
        let path = custom_path.unwrap_or_else(get_ledger_path);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        Self { path }
    }

    fn load_ledger(&self) -> LedgerData {
        if !self.path.exists() {
            return LedgerData::default();
        }
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save_ledger(&self, data: &LedgerData) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(data) {
            let _ = fs::write(&self.path, json);
        }
    }

    pub fn record_snapshot(&self, snap: &SystemSnapshot) {
        let mut ledger = self.load_ledger();
        if let Some(existing) = ledger
            .sessions
            .iter_mut()
            .find(|s| s.session_id == snap.session.session_id)
        {
            *existing = snap.session.clone();
        } else {
            ledger.sessions.insert(0, snap.session.clone());
            if ledger.sessions.len() > 500 {
                ledger.sessions.truncate(500);
            }
        }
        self.save_ledger(&ledger);
    }

    pub fn list_sessions(&self, limit: usize) -> Vec<SessionAccumulator> {
        let mut sessions = self.load_ledger().sessions;
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions.truncate(limit);
        sessions
    }

    pub fn get_lifetime_summary(&self) -> LifetimeSummary {
        let sessions = self.load_ledger().sessions;
        if sessions.is_empty() {
            return LifetimeSummary::default();
        }
        let total_sessions = sessions.len();
        let mut total_seconds = 0.0;
        let mut total_energy_wh = 0.0;
        let mut total_energy_kwh = 0.0;
        let mut total_cost_uah = 0.0;
        let mut total_cost_usd = 0.0;
        let mut total_gross_uah = 0.0;
        let mut total_net_uah = 0.0;
        let mut sum_avg_w = 0.0;
        let mut max_wall_watts = 0.0f64;
        let mut max_cpu_temp_c = 0.0f64;
        let mut max_gpu_temp_c = 0.0f64;

        for s in &sessions {
            total_seconds += s.elapsed_seconds;
            total_energy_wh += s.energy_wh;
            total_energy_kwh += s.energy_kwh;
            total_cost_uah += s.cost_uah;
            total_cost_usd += s.cost_usd;
            total_gross_uah += s.gross_revenue_uah;
            total_net_uah += s.net_profit_uah;
            sum_avg_w += s.avg_wall_watts;
            if s.peak_wall_watts > max_wall_watts {
                max_wall_watts = s.peak_wall_watts;
            }
            if s.peak_cpu_temp_c > max_cpu_temp_c {
                max_cpu_temp_c = s.peak_cpu_temp_c;
            }
            if s.peak_gpu_temp_c > max_gpu_temp_c {
                max_gpu_temp_c = s.peak_gpu_temp_c;
            }
        }

        LifetimeSummary {
            total_sessions,
            total_seconds: round2(total_seconds),
            total_energy_wh: round4(total_energy_wh),
            total_energy_kwh: round5(total_energy_kwh),
            total_cost_uah: round4(total_cost_uah),
            total_cost_usd: round5(total_cost_usd),
            total_gross_uah: round4(total_gross_uah),
            total_net_uah: round4(total_net_uah),
            mean_wall_watts: round2(sum_avg_w / (total_sessions as f64)),
            max_wall_watts: round2(max_wall_watts),
            max_cpu_temp_c: round2(max_cpu_temp_c),
            max_gpu_temp_c: round2(max_gpu_temp_c),
        }
    }

    pub fn export_sessions_csv(&self, output_path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }
        let sessions = self.list_sessions(1000);
        let mut wtr = csv::Writer::from_path(output_path).map_err(|e| e.to_string())?;
        for s in sessions {
            wtr.serialize(s).map_err(|e| e.to_string())?;
        }
        wtr.flush().map_err(|e| e.to_string())?;
        Ok(output_path.to_path_buf())
    }

    pub fn export_sessions_json(&self, output_path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }
        let payload = serde_json::json!({
            "summary": self.get_lifetime_summary(),
            "sessions": self.list_sessions(1000),
        });
        let raw = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;
        fs::write(output_path, raw).map_err(|e| e.to_string())?;
        Ok(output_path.to_path_buf())
    }
}
