use crate::config::{get_app_dir, AppConfig, TARIFF_PROFILE_IDS};
use crate::economics::{round2, round3, round4};
use crate::models::{CryptoMarketSnapshot, ExchangeRates, TariffProfile};
use chrono::{Local, Timelike};
use encoding_rs::WINDOWS_1251;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_UA_BASE_TARIFF_UAH: f64 = 4.32;
pub const DEFAULT_UA_HEATING_TARIFF_UAH: f64 = 2.64;
pub const DEFAULT_UA_COMMERCIAL_DAY_UAH: f64 = 8.80;
pub const DEFAULT_UA_COMMERCIAL_NIGHT_UAH: f64 = 6.20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UaTariffData {
    pub base_uah_kwh: f64,
    pub heating_uah_kwh: f64,
    pub source: String,
    pub updated_at: String,
    pub is_live: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EuSpotZoneData {
    pub latest_eur_mwh: f64,
    pub avg_24h_eur_mwh: f64,
    pub latest_eur_kwh: f64,
    pub updated_at: String,
    pub source: String,
    pub is_live: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketCache {
    pub timestamp: u64,
    pub fx: ExchangeRates,
    pub ua_tariff: UaTariffData,
    pub eu_spots: HashMap<String, EuSpotZoneData>,
    pub crypto: CryptoMarketSnapshot,
}

impl Default for MarketCache {
    fn default() -> Self {
        let now_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut eu_spots = HashMap::new();
        eu_spots.insert(
            "PL".to_string(),
            EuSpotZoneData {
                latest_eur_mwh: 157.38,
                avg_24h_eur_mwh: 142.50,
                latest_eur_kwh: 0.15738,
                updated_at: now_str.clone(),
                source: "ENTSO-E PL (Cached)".to_string(),
                is_live: false,
            },
        );
        eu_spots.insert(
            "DE-LU".to_string(),
            EuSpotZoneData {
                latest_eur_mwh: 138.20,
                avg_24h_eur_mwh: 125.40,
                latest_eur_kwh: 0.1382,
                updated_at: now_str.clone(),
                source: "ENTSO-E DE-LU (Cached)".to_string(),
                is_live: false,
            },
        );

        Self {
            timestamp: 0,
            fx: ExchangeRates {
                usd_to_uah: 45.32,
                eur_to_uah: 50.65,
                pln_to_uah: 11.53,
                eur_to_usd: 1.1176,
                updated_at: now_str.clone(),
                source: "NBU Cached".to_string(),
                is_live: false,
            },
            ua_tariff: UaTariffData {
                base_uah_kwh: DEFAULT_UA_BASE_TARIFF_UAH,
                heating_uah_kwh: DEFAULT_UA_HEATING_TARIFF_UAH,
                source: "КМУ Постанова №632 / YASNO".to_string(),
                updated_at: now_str.clone(),
                is_live: false,
            },
            eu_spots,
            crypto: CryptoMarketSnapshot {
                btc_usd: 74150.0,
                eth_usd: 2110.0,
                sol_usd: 86.4,
                render_usd: 1.55,
                updated_at: now_str,
                source: "Market Cached".to_string(),
                is_live: false,
            },
        }
    }
}

pub fn get_market_cache_path() -> PathBuf {
    get_app_dir().join("market_cache.json")
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn make_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(2000))
        .timeout_read(Duration::from_millis(2500))
        .timeout(Duration::from_millis(3500))
        .user_agent("Telemetria-Rust/2.0")
        .build()
}

fn fetch_exchange_rates_live() -> Option<ExchangeRates> {
    let agent = make_agent();
    let mut usd_uah: Option<f64> = None;
    let mut eur_uah: Option<f64> = None;
    let mut pln_uah: Option<f64> = None;
    let mut sources: Vec<&str> = Vec::new();

    if let Ok(resp) = agent
        .get("https://bank.gov.ua/NBUStatService/v1/statdirectory/exchange?json")
        .call()
    {
        if let Ok(json) = resp.into_json::<serde_json::Value>() {
            if let Some(arr) = json.as_array() {
                for item in arr {
                    let cc = item.get("cc").and_then(|v| v.as_str()).unwrap_or("");
                    let rate = item.get("rate").and_then(|v| v.as_f64());
                    if let Some(r) = rate {
                        match cc {
                            "USD" => usd_uah = Some(r),
                            "EUR" => eur_uah = Some(r),
                            "PLN" => pln_uah = Some(r),
                            _ => {}
                        }
                    }
                }
                if usd_uah.is_some() {
                    sources.push("NBU Live");
                }
            }
        }
    }

    if usd_uah.is_none() || eur_uah.is_none() || pln_uah.is_none() {
        if let Ok(resp) = agent.get("https://open.er-api.com/v6/latest/USD").call() {
            if let Ok(json) = resp.into_json::<serde_json::Value>() {
                if let Some(rates) = json.get("rates").and_then(|v| v.as_object()) {
                    let uah = rates.get("UAH").and_then(|v| v.as_f64()).unwrap_or(45.32);
                    let eur = rates.get("EUR").and_then(|v| v.as_f64()).unwrap_or(0.90);
                    let pln = rates.get("PLN").and_then(|v| v.as_f64()).unwrap_or(3.94);
                    if usd_uah.is_none() {
                        usd_uah = Some(uah);
                    }
                    let u = usd_uah.unwrap_or(uah);
                    if eur_uah.is_none() && eur > 0.0 {
                        eur_uah = Some(u / eur);
                    }
                    if pln_uah.is_none() && pln > 0.0 {
                        pln_uah = Some(u / pln);
                    }
                    sources.push("Open-ER Live");
                }
            }
        }
    }

    let usd = usd_uah?;
    let eur = eur_uah.unwrap_or(usd * 1.10);
    let pln = pln_uah.unwrap_or(usd * 0.25);

    Some(ExchangeRates {
        usd_to_uah: round4(usd),
        eur_to_uah: round4(eur),
        pln_to_uah: round4(pln),
        eur_to_usd: round4(eur / usd.max(0.01)),
        updated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        source: if sources.is_empty() {
            "NBU Live".to_string()
        } else {
            sources.join(" + ")
        },
        is_live: true,
    })
}

fn fetch_ua_tariff_live() -> Option<UaTariffData> {
    let agent = make_agent();
    if let Ok(resp) = agent
        .get("https://index.minfin.com.ua/ua/tariff/electric/")
        .call()
    {
        let mut bytes = Vec::new();
        if resp.into_reader().take(256 * 1024).read_to_end(&mut bytes).is_ok() {
            let (cow, _, _) = WINDOWS_1251.decode(&bytes);
            let re = Regex::new(r"<big>\s*(\d+[,\.]\d{2})\s*</big>").ok()?;
            if let Some(caps) = re.captures(&cow) {
                if let Some(m) = caps.get(1) {
                    let normalized = m.as_str().replace(',', ".");
                    if let Ok(val) = normalized.parse::<f64>() {
                        if (1.0..=25.0).contains(&val) {
                            return Some(UaTariffData {
                                base_uah_kwh: val,
                                heating_uah_kwh: DEFAULT_UA_HEATING_TARIFF_UAH,
                                source: "Minfin / НКРЕКП (Live Web)".to_string(),
                                updated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                                is_live: true,
                            });
                        }
                    }
                }
            }
        }
    }

    if let Ok(resp) = agent.get("https://yasno.com.ua/b2c-tariffs").call() {
        if let Ok(text) = resp.into_string() {
            if text.contains("4.32") || text.contains("4,32") || text.contains("2.16") {
                return Some(UaTariffData {
                    base_uah_kwh: 4.32,
                    heating_uah_kwh: 2.64,
                    source: "YASNO Ukraine (Live Web)".to_string(),
                    updated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    is_live: true,
                });
            }
        }
    }
    None
}

fn fetch_eu_spot_zone(zone: &str) -> Option<(String, EuSpotZoneData)> {
    let agent = make_agent();
    let url = format!("https://api.energy-charts.info/price?bzn={}", zone);
    let resp = agent.get(&url).call().ok()?;
    let json: serde_json::Value = resp.into_json().ok()?;
    let arr = json.get("price")?.as_array()?;
    let prices: Vec<f64> = arr.iter().filter_map(|v| v.as_f64()).collect();
    if prices.is_empty() {
        return None;
    }
    let latest_eur_mwh = *prices.last()?;
    let tail_len = prices.len().min(24);
    let tail = &prices[prices.len() - tail_len..];
    let avg_24h = tail.iter().sum::<f64>() / (tail_len as f64);

    Some((
        zone.to_string(),
        EuSpotZoneData {
            latest_eur_mwh: round2(latest_eur_mwh),
            avg_24h_eur_mwh: round2(avg_24h),
            latest_eur_kwh: round4(latest_eur_mwh / 1000.0),
            updated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            source: format!("Energy-Charts ENTSO-E ({})", zone),
            is_live: true,
        },
    ))
}

fn fetch_crypto_live() -> Option<CryptoMarketSnapshot> {
    let agent = make_agent();
    let url = "https://api.binance.com/api/v3/ticker/price?symbols=%5B%22BTCUSDT%22,%22ETHUSDT%22,%22SOLUSDT%22,%22RENDERUSDT%22%5D";
    let resp = agent.get(url).call().ok()?;
    let arr: Vec<serde_json::Value> = resp.into_json().ok()?;
    let mut map = HashMap::new();
    for item in arr {
        if let (Some(sym), Some(p_str)) = (
            item.get("symbol").and_then(|v| v.as_str()),
            item.get("price").and_then(|v| v.as_str()),
        ) {
            if let Ok(p) = p_str.parse::<f64>() {
                map.insert(sym.to_string(), p);
            }
        }
    }
    if map.is_empty() {
        return None;
    }
    Some(CryptoMarketSnapshot {
        btc_usd: round2(*map.get("BTCUSDT").unwrap_or(&74150.0)),
        eth_usd: round2(*map.get("ETHUSDT").unwrap_or(&2110.0)),
        sol_usd: round2(*map.get("SOLUSDT").unwrap_or(&86.4)),
        render_usd: round4(*map.get("RENDERUSDT").unwrap_or(&1.55)),
        updated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        source: "Binance Spot API (Live)".to_string(),
        is_live: true,
    })
}

pub struct LiveTariffService {
    cache: Arc<RwLock<MarketCache>>,
}

impl LiveTariffService {
    pub fn new() -> Self {
        let initial = Self::load_disk_cache().unwrap_or_default();
        Self {
            cache: Arc::new(RwLock::new(initial)),
        }
    }

    pub fn shared_cache(&self) -> Arc<RwLock<MarketCache>> {
        Arc::clone(&self.cache)
    }

    pub fn load_disk_cache() -> Option<MarketCache> {
        let path = get_market_cache_path();
        let raw = fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn save_disk_cache(data: &MarketCache) {
        let path = get_market_cache_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(data) {
            let _ = fs::write(path, raw);
        }
    }

    /// Parallel live fetch across all internet APIs using scoped threads.
    pub fn refresh_now(&self, force: bool, offline: bool, ttl_sec: u64) -> MarketCache {
        let now_ts = current_unix_secs();
        {
            if let Ok(guard) = self.cache.read() {
                if offline || (!force && guard.timestamp > 0 && (now_ts - guard.timestamp) < ttl_sec)
                {
                    return guard.clone();
                }
            }
        }

        if offline {
            return self.cache.read().map(|g| g.clone()).unwrap_or_default();
        }

        let (fx_opt, ua_opt, pl_opt, de_opt, crypto_opt) = thread::scope(|s| {
            let h_fx = s.spawn(fetch_exchange_rates_live);
            let h_ua = s.spawn(fetch_ua_tariff_live);
            let h_pl = s.spawn(|| fetch_eu_spot_zone("PL"));
            let h_de = s.spawn(|| fetch_eu_spot_zone("DE-LU"));
            let h_cr = s.spawn(fetch_crypto_live);

            (
                h_fx.join().ok().flatten(),
                h_ua.join().ok().flatten(),
                h_pl.join().ok().flatten(),
                h_de.join().ok().flatten(),
                h_cr.join().ok().flatten(),
            )
        });

        let mut updated = self.cache.read().map(|g| g.clone()).unwrap_or_default();
        let mut any_live = false;

        if let Some(fx) = fx_opt {
            updated.fx = fx;
            any_live = true;
        }
        if let Some(ua) = ua_opt {
            updated.ua_tariff = ua;
            any_live = true;
        }
        if let Some((k, v)) = pl_opt {
            updated.eu_spots.insert(k, v);
            any_live = true;
        }
        if let Some((k, v)) = de_opt {
            updated.eu_spots.insert(k, v);
            any_live = true;
        }
        if let Some(cr) = crypto_opt {
            updated.crypto = cr;
            any_live = true;
        }

        if any_live {
            updated.timestamp = now_ts;
            Self::save_disk_cache(&updated);
        }

        if let Ok(mut guard) = self.cache.write() {
            *guard = updated.clone();
        }
        updated
    }

    pub fn spawn_background_updater(&self, interval_sec: u64) -> Sender<()> {
        let (tx, rx) = mpsc::channel::<()>();
        let svc = Self {
            cache: Arc::clone(&self.cache),
        };
        thread::spawn(move || {
            // Initial immediate live update on background startup
            svc.refresh_now(true, false, interval_sec);
            loop {
                match rx.recv_timeout(Duration::from_secs(interval_sec.max(15))) {
                    Ok(()) => {
                        svc.refresh_now(true, false, 0);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        svc.refresh_now(true, false, 0);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        tx
    }

    pub fn current_snapshot_data(&self) -> MarketCache {
        self.cache.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn build_tariff_profile(
        market: &MarketCache,
        profile_id: &str,
        custom_rate_uah: f64,
        hour_override: Option<u32>,
    ) -> TariffProfile {
        let hour = hour_override.unwrap_or_else(|| Local::now().hour());
        let is_night = hour >= 23 || hour < 7;
        let is_peak = (8..11).contains(&hour) || (20..22).contains(&hour);

        let base_uah = market.ua_tariff.base_uah_kwh;
        let heating_uah = market.ua_tariff.heating_uah_kwh;
        let ua_source = market.ua_tariff.source.clone();
        let ua_updated = market.ua_tariff.updated_at.clone();
        let ua_live = market.ua_tariff.is_live;

        match profile_id {
            "ua_single" => TariffProfile {
                profile_id: "ua_single".to_string(),
                name: "Україна — Однозонний побутовий (Фіксований)".to_string(),
                country: "UA".to_string(),
                currency: "UAH".to_string(),
                current_rate_uah_kwh: round3(base_uah),
                day_rate_uah_kwh: round3(base_uah),
                night_rate_uah_kwh: round3(base_uah),
                peak_rate_uah_kwh: round3(base_uah),
                current_zone_label: "24/7 ФІКСОВАНИЙ (100%)".to_string(),
                is_night_discount_active: false,
                schedule_description: format!("Цілодобово єдиний тариф {:.2} ₴/кВт·год", base_uah),
                source: ua_source,
                updated_at: ua_updated,
                is_live: ua_live,
            },
            "ua_three_zone" => {
                let day_rate = round3(base_uah);
                let night_rate = round3(base_uah * 0.4);
                let peak_rate = round3(base_uah * 1.5);
                let (current, zone_label) = if is_night {
                    (night_rate, "НІЧНИЙ ТАРИФ 0.4x (-60%, 23:00–07:00)")
                } else if is_peak {
                    (peak_rate, "ПІКОВИЙ ТАРИФ 1.5x (+50%, 08–11 / 20–22)")
                } else {
                    (day_rate, "НАПІВПІКОВИЙ ТАРИФ 1.0x (Денний)")
                };
                TariffProfile {
                    profile_id: "ua_three_zone".to_string(),
                    name: "Україна — Тризонний лічильник (Пік / Напівпік / Ніч)".to_string(),
                    country: "UA".to_string(),
                    currency: "UAH".to_string(),
                    current_rate_uah_kwh: current,
                    day_rate_uah_kwh: day_rate,
                    night_rate_uah_kwh: night_rate,
                    peak_rate_uah_kwh: peak_rate,
                    current_zone_label: zone_label.to_string(),
                    is_night_discount_active: is_night,
                    schedule_description: format!(
                        "Пік 1.5x ({:.2} ₴) | Напівпік 1.0x ({:.2} ₴) | Ніч 0.4x ({:.3} ₴)",
                        peak_rate, day_rate, night_rate
                    ),
                    source: ua_source,
                    updated_at: ua_updated,
                    is_live: ua_live,
                }
            }
            "ua_heating" => {
                let day_rate = round3(heating_uah);
                let night_rate = round3(heating_uah * 0.5);
                let current = if is_night { night_rate } else { day_rate };
                let zone_label = if is_night {
                    "НІЧ ОПАЛЕННЯ -50% (23:00–07:00)"
                } else {
                    "ДЕНЬ ОПАЛЕННЯ (07:00–23:00)"
                };
                TariffProfile {
                    profile_id: "ua_heating".to_string(),
                    name: "Україна — Електроопалення (<2000 кВт·год/міс)".to_string(),
                    country: "UA".to_string(),
                    currency: "UAH".to_string(),
                    current_rate_uah_kwh: current,
                    day_rate_uah_kwh: day_rate,
                    night_rate_uah_kwh: night_rate,
                    peak_rate_uah_kwh: day_rate,
                    current_zone_label: zone_label.to_string(),
                    is_night_discount_active: is_night,
                    schedule_description: format!(
                        "День (07:00-23:00): {:.2} ₴ | Ніч (23:00-07:00): {:.2} ₴",
                        day_rate, night_rate
                    ),
                    source: ua_source,
                    updated_at: ua_updated,
                    is_live: ua_live,
                }
            }
            "ua_commercial" => {
                let day_rate = DEFAULT_UA_COMMERCIAL_DAY_UAH;
                let night_rate = DEFAULT_UA_COMMERCIAL_NIGHT_UAH;
                let current = if is_night { night_rate } else { day_rate };
                TariffProfile {
                    profile_id: "ua_commercial".to_string(),
                    name: "Україна — Комерційний тариф (Дата-центри / Серверні)".to_string(),
                    country: "UA".to_string(),
                    currency: "UAH".to_string(),
                    current_rate_uah_kwh: current,
                    day_rate_uah_kwh: day_rate,
                    night_rate_uah_kwh: night_rate,
                    peak_rate_uah_kwh: day_rate,
                    current_zone_label: if is_night {
                        "НІЧ КОМЕРЦІЯ".to_string()
                    } else {
                        "ДЕНЬ КОМЕРЦІЯ".to_string()
                    },
                    is_night_discount_active: is_night,
                    schedule_description: format!(
                        "День: {:.2} ₴/кВт·год | Ніч: {:.2} ₴/кВт·год",
                        day_rate, night_rate
                    ),
                    source: "Укренерго РДН + розподіл".to_string(),
                    updated_at: ua_updated,
                    is_live: ua_live,
                }
            }
            "eu_spot_pl" | "eu_spot_de" => {
                let zone_code = if profile_id == "eu_spot_pl" {
                    "PL"
                } else {
                    "DE-LU"
                };
                let spot = market.eu_spots.get(zone_code);
                let eur_kwh = spot.map(|s| s.latest_eur_kwh).unwrap_or(0.145);
                let eur_mwh = spot.map(|s| s.latest_eur_mwh).unwrap_or(145.0);
                let spot_uah = round3(eur_kwh * market.fx.eur_to_uah);
                let night_uah = round3(spot_uah * 0.78);
                TariffProfile {
                    profile_id: profile_id.to_string(),
                    name: format!("ЄС ENTSO-E Spot ({}) — Погодинна спотова ціна", zone_code),
                    country: zone_code.to_string(),
                    currency: "EUR/UAH".to_string(),
                    current_rate_uah_kwh: spot_uah,
                    day_rate_uah_kwh: spot_uah,
                    night_rate_uah_kwh: night_uah,
                    peak_rate_uah_kwh: round3(spot_uah * 1.2),
                    current_zone_label: format!("SPOT LIVE ({:.1} EUR/MWh)", eur_mwh),
                    is_night_discount_active: is_night,
                    schedule_description: format!(
                        "Погодинна біржова ціна ENTSO-E ({}): {:.4} €/кВт·год",
                        zone_code, eur_kwh
                    ),
                    source: spot
                        .map(|s| s.source.clone())
                        .unwrap_or_else(|| format!("Energy-Charts ({})", zone_code)),
                    updated_at: spot
                        .map(|s| s.updated_at.clone())
                        .unwrap_or(ua_updated),
                    is_live: spot.map(|s| s.is_live).unwrap_or(false),
                }
            }
            "custom" => {
                let c_rate = round3(custom_rate_uah.max(0.01));
                TariffProfile {
                    profile_id: "custom".to_string(),
                    name: "Користувацький тариф (Custom Rate)".to_string(),
                    country: "CUSTOM".to_string(),
                    currency: "UAH".to_string(),
                    current_rate_uah_kwh: c_rate,
                    day_rate_uah_kwh: c_rate,
                    night_rate_uah_kwh: round3(c_rate * 0.5),
                    peak_rate_uah_kwh: c_rate,
                    current_zone_label: "CUSTOM FIXED".to_string(),
                    is_night_discount_active: false,
                    schedule_description: format!(
                        "Встановлений користувачем тариф: {:.2} ₴/кВт·год",
                        c_rate
                    ),
                    source: "User Config".to_string(),
                    updated_at: ua_updated,
                    is_live: true,
                }
            }
            _ => {
                // Default: ua_two_zone (Двозонний лічильник День/Ніч -50%)
                let day_rate = round3(base_uah);
                let night_rate = round3(base_uah * 0.5);
                let current = if is_night { night_rate } else { day_rate };
                let zone_label = if is_night {
                    format!("НІЧНИЙ ТАРИФ -50% ({:.2} ₴, 23:00–07:00)", night_rate)
                } else {
                    format!("ДЕННИЙ ТАРИФ ({:.2} ₴, 07:00–23:00)", day_rate)
                };
                TariffProfile {
                    profile_id: "ua_two_zone".to_string(),
                    name: "Україна — Двозонний лічильник (День / Ніч -50%)".to_string(),
                    country: "UA".to_string(),
                    currency: "UAH".to_string(),
                    current_rate_uah_kwh: current,
                    day_rate_uah_kwh: day_rate,
                    night_rate_uah_kwh: night_rate,
                    peak_rate_uah_kwh: day_rate,
                    current_zone_label: zone_label,
                    is_night_discount_active: is_night,
                    schedule_description: format!(
                        "День (07:00-23:00): {:.2} ₴/кВт·год | Ніч (23:00-07:00): {:.2} ₴/кВт·год",
                        day_rate, night_rate
                    ),
                    source: ua_source,
                    updated_at: ua_updated,
                    is_live: ua_live,
                }
            }
        }
    }

    pub fn list_all_profiles(
        &self,
        cfg: &AppConfig,
        hour_override: Option<u32>,
    ) -> Vec<TariffProfile> {
        let market = self.current_snapshot_data();
        TARIFF_PROFILE_IDS
            .iter()
            .map(|pid| {
                Self::build_tariff_profile(
                    &market,
                    pid,
                    cfg.custom_tariff_uah_kwh,
                    hour_override,
                )
            })
            .collect()
    }
}
