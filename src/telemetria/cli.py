"""
CLI entry point for Telemetria.
Provides `dashboard`, `status`, `tariffs`, `mining`, `history`, `export`, and `config` commands.
"""

from __future__ import annotations

import json
import shutil
import sys
import time
from pathlib import Path
from typing import Optional

import typer
from rich import box
from rich.console import Console
from rich.live import Live
from rich.panel import Panel
from rich.table import Table

from telemetria import __version__
from telemetria.config import (
    PSU_EFFICIENCY_MAP,
    WORKLOAD_PRESETS,
    load_config,
    save_config,
)
from telemetria.dashboard import (
    DashboardRenderer,
    format_duration,
    render_mining_scenarios_view,
    render_tariffs_view,
)
from telemetria.economics import simulate_workload_scenarios
from telemetria.sensors import HardwareMonitor
from telemetria.storage import TelemetryStorage
from telemetria.tariffs import LiveTariffService


def _ensure_utf8_console() -> None:
    for stream_name in ("stdout", "stderr"):
        stream = getattr(sys, stream_name, None)
        if stream is not None and hasattr(stream, "reconfigure"):
            try:
                stream.reconfigure(encoding="utf-8", errors="replace")
            except Exception:
                pass


_ensure_utf8_console()
_term_cols = shutil.get_terminal_size((136, 40)).columns
console = Console(width=max(136, _term_cols) if not sys.stdout.isatty() else None)

app = typer.Typer(
    name="telemetria",
    help=(
        "⚡ TELEMETRIA — CLI/TUI моніторинг комплектуючих ПК (CPU, GPU, RAM, NVMe, Мережа), "
        "енергоспоживання з розетки, автооновлення тарифів на електроенергію з інтернету "
        "та розрахунок прибутковості майнінгу і здачі ПК в оренду як сервера."
    ),
    add_completion=False,
    no_args_is_help=False,
)


def _run_live_dashboard(
    interval: float,
    duration: Optional[float],
    once: bool,
    offline: bool,
    refresh_tariffs: bool,
) -> None:
    cfg = load_config()
    tariff_svc = LiveTariffService(cfg)
    monitor = HardwareMonitor(config=cfg, tariff_service=tariff_svc)
    storage = TelemetryStorage()
    renderer = DashboardRenderer()

    try:
        # Warm up first sample
        snap = monitor.sample(
            force_tariff_refresh=refresh_tariffs,
            offline=offline,
            include_processes=True,
        )
        storage.record_snapshot(snap, record_tick=True)

        if once:
            console.print(renderer.render_full_dashboard(snap))
            return

        start_ts = time.monotonic()
        with Live(
            renderer.render_full_dashboard(snap),
            console=console,
            refresh_per_second=4,
            screen=False,
        ) as live:
            while True:
                if duration is not None and (time.monotonic() - start_ts) >= duration:
                    break
                time.sleep(max(0.2, interval))
                snap = monitor.sample(
                    force_tariff_refresh=False,
                    offline=offline,
                    include_processes=True,
                )
                storage.record_snapshot(snap, record_tick=True)
                live.update(renderer.render_full_dashboard(snap))
    except KeyboardInterrupt:
        pass
    finally:
        monitor.close()


@app.callback(invoke_without_command=True)
def main_callback(
    ctx: typer.Context,
    version: bool = typer.Option(
        False, "--version", "-v", help="Показати версію Telemetria та вийти."
    ),
    once: bool = typer.Option(
        False, "--once", help="Вивести один знімок дашборду та завершити роботу."
    ),
    offline: bool = typer.Option(
        False, "--offline", help="Працювати без звернення до інтернет-API (кеш)."
    ),
) -> None:
    if version:
        console.print(f"[bold cyan]Telemetria[/] v{__version__}")
        raise typer.Exit(0)
    if ctx.invoked_subcommand is None:
        _run_live_dashboard(
            interval=1.0,
            duration=None,
            once=once,
            offline=offline,
            refresh_tariffs=False,
        )


@app.command("dashboard")
def cmd_dashboard(
    interval: float = typer.Option(
        1.0, "--interval", "-i", help="Інтервал оновлення телеметрії в секундах."
    ),
    duration: Optional[float] = typer.Option(
        None, "--duration", "-d", help="Тривалість роботи в секундах (за замовчуванням безперервно)."
    ),
    once: bool = typer.Option(
        False, "--once", help="Зробити один кадр дашборду та вийти."
    ),
    offline: bool = typer.Option(
        False, "--offline", help="Не виконувати мережеві запити (використовувати локальний кеш)."
    ),
    refresh_tariffs: bool = typer.Option(
        False, "--refresh", "-r", help="Примусово оновити тарифи та курси валют з інтернету."
    ),
) -> None:
    """
    Запустити інтерактивний Live TUI дашборд моніторингу ПК, витрат електрики та рентабельності.
    """
    _run_live_dashboard(
        interval=interval,
        duration=duration,
        once=once,
        offline=offline,
        refresh_tariffs=refresh_tariffs,
    )


@app.command("status")
def cmd_status(
    as_json: bool = typer.Option(
        False, "--json", "-j", help="Вивести повну телеметрію у форматі JSON."
    ),
    offline: bool = typer.Option(
        False, "--offline", help="Використовувати кешовані тарифи без інтернет-запитів."
    ),
    refresh: bool = typer.Option(
        False, "--refresh", "-r", help="Примусово оновити ціни на електроенергію з інтернету."
    ),
) -> None:
    """
    Миттєвий знімок усіх комплектуючих ПК, споживання (W), вартості електрики та прибутку.
    """
    cfg = load_config()
    tariff_svc = LiveTariffService(cfg)
    monitor = HardwareMonitor(config=cfg, tariff_service=tariff_svc)
    storage = TelemetryStorage()
    try:
        time.sleep(0.12)
        snap = monitor.sample(
            force_tariff_refresh=refresh,
            offline=offline,
            include_processes=True,
        )
        storage.record_snapshot(snap, record_tick=True)
        if as_json:
            sys.stdout.write(
                json.dumps(snap.to_dict(), indent=2, ensure_ascii=False) + "\n"
            )
        else:
            renderer = DashboardRenderer()
            console.print(renderer.render_full_dashboard(snap))
    finally:
        monitor.close()


@app.command("tariffs")
def cmd_tariffs(
    set_profile: Optional[str] = typer.Option(
        None,
        "--set",
        "-s",
        help="Встановити активний тарифний профіль (ua_two_zone, ua_single, ua_three_zone, ua_heating, ua_commercial, eu_spot_pl, eu_spot_de, custom).",
    ),
    custom_rate: Optional[float] = typer.Option(
        None,
        "--custom-rate",
        help="Встановити власну ціну за 1 кВт·год у грн (автоматично вмикає профіль custom).",
    ),
    refresh: bool = typer.Option(
        True,
        "--refresh/--no-refresh",
        help="Автоматично підключитися до інтернету та оновити тарифи і курси валют.",
    ),
    offline: bool = typer.Option(
        False, "--offline", help="Працювати офлайн з локальним кешем."
    ),
    as_json: bool = typer.Option(
        False, "--json", help="Вивести тарифи та курси у форматі JSON."
    ),
) -> None:
    """
    Переглянути та оновити з інтернету поточні ціни на електроенергію, нічний тариф та курси НБУ.
    """
    cfg = load_config()
    updated_cfg = False

    if custom_rate is not None and custom_rate > 0:
        cfg.custom_tariff_uah_kwh = round(custom_rate, 3)
        cfg.tariff_profile = "custom"
        updated_cfg = True

    if set_profile:
        valid_ids = {
            "ua_two_zone",
            "ua_single",
            "ua_three_zone",
            "ua_heating",
            "ua_commercial",
            "eu_spot_pl",
            "eu_spot_de",
            "custom",
        }
        if set_profile not in valid_ids:
            console.print(
                f"[bold red]Помилка:[/] Невідомий тариф '{set_profile}'. Доступні: {', '.join(sorted(valid_ids))}"
            )
            raise typer.Exit(1)
        cfg.tariff_profile = set_profile
        updated_cfg = True

    if updated_cfg:
        save_config(cfg)

    svc = LiveTariffService(cfg)
    profiles = svc.list_all_profiles(force=refresh and not offline, offline=offline)
    fx = svc.get_exchange_rates(force=False, offline=offline)
    crypto = svc.get_crypto_snapshot(force=False, offline=offline)

    if as_json:
        payload = {
            "active_profile": cfg.tariff_profile,
            "exchange_rates": fx.__dict__,
            "crypto_market": crypto.__dict__,
            "profiles": [p.__dict__ for p in profiles],
        }
        sys.stdout.write(json.dumps(payload, indent=2, ensure_ascii=False) + "\n")
        return

    if updated_cfg:
        console.print(
            f"[bold bright_green]✓ Тарифний профіль успішно змінено на:[/] [bold yellow]{cfg.tariff_profile}[/]"
        )
    render_tariffs_view(
        console=console,
        profiles=profiles,
        active_profile_id=cfg.tariff_profile,
        fx=fx,
        crypto=crypto,
    )


@app.command("mining")
def cmd_mining(
    rate_usd: Optional[float] = typer.Option(
        None,
        "--rate-usd",
        help="Встановити очікуваний дохід від оренди сервера або майнінгу в USD за годину (напр. 0.42).",
    ),
    rate_uah: Optional[float] = typer.Option(
        None,
        "--rate-uah",
        help="Встановити очікуваний дохід у грн за годину (автоматично конвертується у USD за курсом НБУ).",
    ),
    profile: Optional[str] = typer.Option(
        None,
        "--profile",
        "-p",
        help=f"Обрати профіль роботи ({', '.join(WORKLOAD_PRESETS.keys())}).",
    ),
    offline: bool = typer.Option(
        False, "--offline", help="Працювати офлайн без інтернет-запитів."
    ),
    as_json: bool = typer.Option(
        False, "--json", help="Вивести розрахунок рентабельності у форматі JSON."
    ),
) -> None:
    """
    Режим майнінгу та здачі ПК в оренду як сервера: розрахунок витрат електрики, ROI та чистого прибутку.
    """
    cfg = load_config()
    svc = LiveTariffService(cfg)
    fx = svc.get_exchange_rates(force=False, offline=offline)
    changed = False

    if profile:
        if profile not in WORKLOAD_PRESETS:
            console.print(
                f"[bold red]Невідомий профіль '{profile}'.[/] Доступні: {', '.join(WORKLOAD_PRESETS.keys())}"
            )
            raise typer.Exit(1)
        cfg.workload_profile = profile
        if rate_usd is None and rate_uah is None:
            cfg.rental_revenue_usd_hr = float(
                WORKLOAD_PRESETS[profile]["default_revenue_usd_hr"]
            )
        changed = True

    if rate_usd is not None and rate_usd >= 0:
        cfg.rental_revenue_usd_hr = round(rate_usd, 4)
        changed = True
    elif rate_uah is not None and rate_uah >= 0:
        cfg.rental_revenue_usd_hr = round(rate_uah / max(fx.usd_to_uah, 0.01), 4)
        changed = True

    if changed:
        save_config(cfg)

    monitor = HardwareMonitor(config=cfg, tariff_service=svc)
    try:
        time.sleep(0.1)
        snap = monitor.sample(
            force_tariff_refresh=False,
            offline=offline,
            include_processes=False,
        )
        gpu_tdp = snap.gpus[0].power_limit_watts if snap.gpus else 250.0
        scenarios = simulate_workload_scenarios(
            current_wall_watts=snap.power.total_wall_watts,
            cpu_tdp_watts=snap.cpu.tdp_watts,
            gpu_tdp_watts=gpu_tdp,
            psu_efficiency=snap.power.psu_efficiency,
            tariff=snap.tariff,
            fx=snap.fx,
            rental_usd_hr=cfg.rental_revenue_usd_hr,
        )
        if as_json:
            payload = {
                "mining_metrics": snap.mining.__dict__,
                "power": snap.power.__dict__,
                "tariff": snap.tariff.__dict__,
                "scenarios": [s.__dict__ for s in scenarios],
            }
            sys.stdout.write(json.dumps(payload, indent=2, ensure_ascii=False) + "\n")
            return

        if changed:
            console.print(
                f"[bold bright_green]✓ Налаштування оренди/майнінгу збережено:[/] "
                f"профіль=[bold cyan]{cfg.workload_profile}[/], ставка=[bold yellow]${cfg.rental_revenue_usd_hr:.3f}/год ({cfg.rental_revenue_usd_hr * fx.usd_to_uah:.2f} ₴/год)[/]"
            )
        render_mining_scenarios_view(console=console, snap=snap, scenarios=scenarios)
    finally:
        monitor.close()


@app.command("history")
def cmd_history(
    limit: int = typer.Option(
        15, "--limit", "-n", help="Кількість останніх сесій для відображення."
    ),
    as_json: bool = typer.Option(
        False, "--json", help="Вивести історію споживання у форматі JSON."
    ),
) -> None:
    """
    Історія роботи ПК: скільки кВт·год та гривень з'їв ПК за всі сесії та скільки заробив.
    """
    storage = TelemetryStorage()
    summary = storage.get_lifetime_summary()
    sessions = storage.list_sessions(limit=limit)

    if as_json:
        sys.stdout.write(
            json.dumps(
                {"summary": summary, "sessions": sessions},
                indent=2,
                ensure_ascii=False,
            )
            + "\n"
        )
        return

    tot_sec = float(summary.get("total_seconds", 0.0))
    tot_wh = float(summary.get("total_energy_wh", 0.0))
    tot_kwh = float(summary.get("total_energy_kwh", 0.0))
    tot_cost_uah = float(summary.get("total_cost_uah", 0.0))
    tot_cost_usd = float(summary.get("total_cost_usd", 0.0))
    tot_gross_uah = float(summary.get("total_gross_uah", 0.0))
    tot_net_uah = float(summary.get("total_net_uah", 0.0))
    max_w = float(summary.get("max_wall_watts", 0.0))
    mean_w = float(summary.get("mean_wall_watts", 0.0))

    summary_panel = Panel(
        f"[bold white]Всього сесій:[/] [cyan]{summary.get('total_sessions', 0)}[/]  |  "
        f"[bold white]Сумарний час моніторингу:[/] [cyan]{format_duration(tot_sec)}[/]\n"
        f"[bold white]Спожито електроенергії:[/] [bold yellow]{tot_wh:.2f} Вт·год ({tot_kwh:.5f} кВт·год)[/]  |  "
        f"[bold white]Середня / Пікова потужність:[/] [yellow]{mean_w:.1f} W / {max_w:.1f} W[/]\n"
        f"[bold white]Витрачено на електроенергію:[/] [bold bright_red]{tot_cost_uah:.4f} ₴ (${tot_cost_usd:.5f})[/]  |  "
        f"[bold white]Валовий дохід:[/] [cyan]+{tot_gross_uah:.4f} ₴[/]  |  "
        f"[bold white]Чистий баланс:[/] [bold bright_green]{tot_net_uah:+.4f} ₴[/]",
        title="[bold bright_cyan]📚 СУМАРНА СТАТИСТИКА ЕНЕРГОСПОЖИВАННЯ ТА ВИТРАТ (SQLite Ledger)[/]",
        border_style="bright_cyan",
        box=box.ROUNDED,
    )
    console.print(summary_panel)

    tbl = Table(
        title=f"Останні сесії моніторингу (показано {len(sessions)})",
        box=box.ROUNDED,
        header_style="bold cyan",
        expand=True,
    )
    tbl.add_column("ID сесії", style="dim")
    tbl.add_column("Початок", style="white")
    tbl.add_column("Тривалість", justify="right")
    tbl.add_column("Сер. / Пік (W)", justify="right", style="yellow")
    tbl.add_column("Енергія (Вт·год)", justify="right", style="bold yellow")
    tbl.add_column("З'їдено (₴)", justify="right", style="bold red")
    tbl.add_column("Чистий прибуток (₴)", justify="right", style="bold green")
    tbl.add_column("Тариф", style="cyan")

    for s in sessions:
        tbl.add_row(
            str(s["session_id"]),
            str(s["started_at"]),
            format_duration(float(s["elapsed_seconds"])),
            f"{float(s['avg_wall_watts']):.1f} / {float(s['peak_wall_watts']):.1f} W",
            f"{float(s['energy_wh']):.3f} Wh",
            f"{float(s['cost_uah']):.4f} ₴",
            f"{float(s['net_profit_uah']):+.4f} ₴",
            str(s["tariff_profile"]),
        )
    console.print(tbl)


@app.command("export")
def cmd_export(
    fmt: str = typer.Option(
        "csv", "--format", "-f", help="Формат експорту: csv або json."
    ),
    output: Path = typer.Option(
        Path("telemetria_report.csv"),
        "--output",
        "-o",
        help="Шлях до файлу звіту.",
    ),
) -> None:
    """
    Експортувати фінансовий та енергетичний звіт у CSV або JSON.
    """
    storage = TelemetryStorage()
    fmt_lower = fmt.lower().strip()
    if fmt_lower == "json":
        if output.suffix.lower() != ".json":
            output = output.with_suffix(".json")
        saved = storage.export_sessions_json(output)
    else:
        saved = storage.export_sessions_csv(output)
    console.print(
        f"[bold bright_green]✓ Звіт успішно експортовано у файл:[/] [bold yellow]{saved.resolve()}[/]"
    )


@app.command("config")
def cmd_config(
    psu: Optional[str] = typer.Option(
        None,
        "--psu",
        help=f"Сертифікат ККД блоку живлення ({', '.join(PSU_EFFICIENCY_MAP.keys())}).",
    ),
    mobo_watts: Optional[float] = typer.Option(
        None,
        "--mobo-watts",
        help="Базове споживання материнської плати та вентиляторів (W).",
    ),
    reset: bool = typer.Option(
        False, "--reset", help="Скинути налаштування до стандартних."
    ),
) -> None:
    """
    Переглянути або змінити конфігурацію системи (БЖ 80 PLUS, базове споживання, тарифи).
    """
    from telemetria.config import AppConfig, get_config_path

    if reset:
        cfg = AppConfig()
        save_config(cfg)
        console.print("[bold green]✓ Конфігурацію скинуто до заводських налаштувань.[/]")
    else:
        cfg = load_config()
        changed = False
        if psu:
            matched = None
            for k in PSU_EFFICIENCY_MAP:
                if k.lower() == psu.lower().strip():
                    matched = k
                    break
            if not matched:
                console.print(
                    f"[bold red]Невідомий сертифікат БЖ '{psu}'.[/] Доступні: {', '.join(PSU_EFFICIENCY_MAP.keys())}"
                )
                raise typer.Exit(1)
            cfg.psu_rating = matched
            cfg.psu_efficiency = PSU_EFFICIENCY_MAP[matched]
            changed = True
        if mobo_watts is not None and mobo_watts >= 5.0:
            cfg.motherboard_base_watts = round(mobo_watts, 1)
            changed = True
        if changed:
            save_config(cfg)
            console.print("[bold bright_green]✓ Налаштування успішно оновлено![/]")

    tbl = Table(
        title=f"⚙️ Конфігурація Telemetria ({get_config_path()})",
        box=box.ROUNDED,
        header_style="bold cyan",
    )
    tbl.add_column("Параметр", style="bold white")
    tbl.add_column("Значення", style="bold yellow")
    for k, v in cfg.to_dict().items():
        tbl.add_row(k, str(v))
    console.print(tbl)


if __name__ == "__main__":
    app()
