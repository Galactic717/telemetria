"""
Rich terminal rendering and Live TUI dashboard for Telemetria.
"""

from __future__ import annotations

from collections import deque
from typing import Any, Deque, Dict, List, Optional

from rich import box
from rich.columns import Columns
from rich.console import Console, Group, RenderableType
from rich.layout import Layout
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from telemetria.economics import WorkloadScenarioRow, compute_weighted_24h_kwh_rate_uah
from telemetria.models import (
    CryptoMarketSnapshot,
    ExchangeRates,
    SystemSnapshot,
    TariffProfile,
)

SPARK_CHARS = "▁▂▃▄▅▆▇█"


def make_sparkline(values: List[float], width: int = 28) -> str:
    if not values:
        return "▁" * width
    recent = values[-width:]
    lo = min(recent)
    hi = max(recent)
    span = max(hi - lo, 1.0)
    chars = []
    for v in recent:
        idx = int(((v - lo) / span) * (len(SPARK_CHARS) - 1))
        idx = max(0, min(len(SPARK_CHARS) - 1, idx))
        chars.append(SPARK_CHARS[idx])
    padded = ("▁" * max(0, width - len(chars))) + "".join(chars)
    return padded


def make_bar(percent: float, width: int = 18) -> Text:
    pct = max(0.0, min(100.0, percent))
    filled = int(round((pct / 100.0) * width))
    empty = max(0, width - filled)
    if pct >= 85.0:
        color = "bold red"
    elif pct >= 60.0:
        color = "bold yellow"
    else:
        color = "bold green"
    txt = Text()
    txt.append("█" * filled, style=color)
    txt.append("░" * empty, style="dim white")
    txt.append(f" {pct:5.1f}%", style=color)
    return txt


def temp_style(temp_c: float, warn_c: float = 78.0) -> str:
    if temp_c >= warn_c + 8:
        return "bold red"
    if temp_c >= warn_c:
        return "bold yellow"
    if temp_c >= 60.0:
        return "cyan"
    return "bold green"


def format_duration(seconds: float) -> str:
    sec = int(max(0, seconds))
    days, rem = divmod(sec, 86400)
    hours, rem = divmod(rem, 3600)
    mins, s = divmod(rem, 60)
    if days > 0:
        return f"{days}д {hours:02d}г {mins:02d}хв"
    return f"{hours:02d}:{mins:02d}:{s:02d}"


class DashboardRenderer:
    """
    Maintains rolling history buffers for sparklines and builds Rich TUI frames.
    """

    def __init__(self, history_len: int = 40) -> None:
        self.wall_watts_history: Deque[float] = deque(maxlen=history_len)
        self.cpu_temp_history: Deque[float] = deque(maxlen=history_len)
        self.gpu_temp_history: Deque[float] = deque(maxlen=history_len)

    def push_snapshot(self, snap: SystemSnapshot) -> None:
        self.wall_watts_history.append(snap.power.total_wall_watts)
        self.cpu_temp_history.append(snap.cpu.temperature_c)
        if snap.gpus:
            self.gpu_temp_history.append(snap.gpus[0].temperature_c)

    def build_header_panel(self, snap: SystemSnapshot) -> Panel:
        grid = Table.grid(expand=True)
        grid.add_column(justify="left", ratio=3)
        grid.add_column(justify="center", ratio=3)
        grid.add_column(justify="right", ratio=3)

        left = Text()
        left.append("⚡ TELEMETRIA v1.0 ", style="bold bright_cyan")
        left.append(f"| {snap.hostname} ({snap.os_name})\n", style="white")
        left.append(f"Материнська плата: ", style="dim")
        left.append(f"{snap.motherboard} ", style="bold white")
        left.append(f"| Аптайм: {format_duration(snap.uptime_seconds)}", style="dim cyan")

        center = Text()
        zone_style = (
            "bold black on bright_green"
            if snap.tariff.is_night_discount_active
            else "bold black on bright_yellow"
        )
        center.append(f" {snap.tariff.current_zone_label} ", style=zone_style)
        center.append(
            f"\nТариф: {snap.tariff.current_rate_uah_kwh:.2f} ₴/кВт·год ({snap.tariff.rate_in_usd(snap.fx):.3f} $)",
            style="bold yellow",
        )

        right = Text()
        live_badge = "● LIVE NET" if snap.fx.is_live else "○ CACHED"
        badge_style = "bold bright_green" if snap.fx.is_live else "bold yellow"
        right.append(f"{live_badge} ", style=badge_style)
        right.append(
            f"USD: {snap.fx.usd_to_uah:.2f} ₴ | EUR: {snap.fx.eur_to_uah:.2f} ₴\n",
            style="white",
        )
        right.append(
            f"BTC: ${snap.crypto.btc_usd:,.0f} | ETH: ${snap.crypto.eth_usd:,.0f} | RENDER: ${snap.crypto.render_usd:.2f}",
            style="dim bright_magenta",
        )

        grid.add_row(left, center, right)
        return Panel(grid, style="cyan", box=box.ROUNDED, padding=(0, 1))

    def build_cpu_panel(self, snap: SystemSnapshot) -> Panel:
        c = snap.cpu
        tbl = Table.grid(expand=True, padding=(0, 1))
        tbl.add_column(ratio=2)
        tbl.add_column(ratio=3)

        t_style = temp_style(c.temperature_c)
        tbl.add_row(
            Text("Процесор:", style="dim"),
            Text(c.name, style="bold bright_white"),
        )
        tbl.add_row(
            Text("Навантаження CPU:", style="dim"),
            make_bar(c.usage_percent, width=18),
        )
        tbl.add_row(
            Text("Температура:", style="dim"),
            Text(
                f"{c.temperature_c:.1f} °C  [{c.temp_source}]",
                style=t_style,
            ),
        )
        tbl.add_row(
            Text("Споживання CPU:", style="dim"),
            Text(
                f"{c.power_watts:.1f} W / {c.tdp_watts:.0f} W TDP  ({(c.power_watts / max(c.tdp_watts, 1.0)) * 100:.0f}%)",
                style="bold bright_yellow",
            ),
        )
        tbl.add_row(
            Text("Частота / Ядра:", style="dim"),
            Text(
                f"{c.frequency_current_mhz:.0f} / {c.frequency_max_mhz:.0f} MHz  ({c.physical_cores}C / {c.logical_cores}T)",
                style="cyan",
            ),
        )

        # Compact per-core mini bars (up to 8 shown inline)
        core_preview = " ".join(
            f"C{i}:{v:2.0f}%" for i, v in enumerate(c.per_core_percent[:8])
        )
        if len(c.per_core_percent) > 8:
            core_preview += f" (+{len(c.per_core_percent) - 8}T)"
        tbl.add_row(
            Text("Потоки:", style="dim"),
            Text(core_preview, style="dim white"),
        )

        return Panel(
            tbl,
            title="[bold bright_cyan]🖥  ПРОЦЕСОР (CPU TELEMETRY)[/]",
            border_style="bright_cyan",
            box=box.ROUNDED,
        )

    def build_gpu_panel(self, snap: SystemSnapshot) -> Panel:
        if not snap.gpus:
            return Panel(
                Text("Дискретна відеокарта не виявлена (Integrated Graphics)", style="dim"),
                title="[bold bright_magenta]🎮 ВІДЕОКАРТА (GPU TELEMETRY)[/]",
                border_style="magenta",
                box=box.ROUNDED,
            )

        g = snap.gpus[0]
        tbl = Table.grid(expand=True, padding=(0, 1))
        tbl.add_column(ratio=2)
        tbl.add_column(ratio=3)

        t_style = temp_style(g.temperature_c)
        vram_pct = (g.vram_used_mb / max(g.vram_total_mb, 1.0)) * 100.0
        fan_label = (
            f"{g.fan_speed_percent:.0f}%"
            if g.fan_speed_percent > 0
            else "0% (Zero-RPM Fan Stop)"
        )
        energy_str = (
            f" | Лічильник GPU: {g.energy_total_kwh:.3f} кВт·год"
            if g.energy_total_kwh is not None
            else ""
        )

        tbl.add_row(
            Text("Відеокарта:", style="dim"),
            Text(
                f"{g.name} (PCIe Gen{g.pcie_gen} x{g.pcie_width}, {g.pstate})",
                style="bold bright_white",
            ),
        )
        tbl.add_row(
            Text("Ядро GPU:", style="dim"),
            make_bar(g.usage_percent, width=18),
        )
        tbl.add_row(
            Text("Відеопам'ять VRAM:", style="dim"),
            Text(
                f"{g.vram_used_mb:.0f} / {g.vram_total_mb:.0f} MiB ({vram_pct:.1f}%)",
                style="bold bright_magenta",
            ),
        )
        tbl.add_row(
            Text("Температура / Кулер:", style="dim"),
            Text(f"{g.temperature_c:.1f} °C  |  Fan: {fan_label}", style=t_style),
        )
        tbl.add_row(
            Text("Споживання GPU:", style="dim"),
            Text(
                f"{g.power_watts:.2f} W / {g.power_limit_watts:.0f} W Limit{energy_str}",
                style="bold bright_yellow",
            ),
        )
        tbl.add_row(
            Text("Частоти (Core/Mem):", style="dim"),
            Text(
                f"Core: {g.clock_core_mhz:.0f} MHz (Max {g.clock_max_core_mhz:.0f}) | Mem: {g.clock_mem_mhz:.0f} MHz",
                style="cyan",
            ),
        )

        return Panel(
            tbl,
            title=f"[bold bright_magenta]🎮 ВІДЕОКАРТА ({g.source})[/]",
            border_style="bright_magenta",
            box=box.ROUNDED,
        )

    def build_system_io_panel(self, snap: SystemSnapshot) -> Panel:
        r = snap.ram
        d = snap.disk
        n = snap.network

        tbl = Table.grid(expand=True, padding=(0, 1))
        tbl.add_column(ratio=2)
        tbl.add_column(ratio=3)

        tbl.add_row(
            Text(f"ОЗП ({r.dimm_count}x {r.memory_type}-{r.speed_mts}):", style="dim"),
            Text(
                f"{r.used_gb:.1f} / {r.total_gb:.1f} GB ({r.usage_percent:.1f}%) | {r.power_watts:.1f} W [{r.part_number}]",
                style="bold white",
            ),
        )
        part_summary = (
            ", ".join(
                f"{p.mountpoint} {p.used_gb:.0f}/{p.total_gb:.0f}GB ({p.usage_percent:.0f}%)"
                for p in d.partitions[:3]
            )
            or "N/A"
        )
        tbl.add_row(
            Text(f"Диск ({d.model_name}):", style="dim"),
            Text(
                f"{d.media_type} [{d.health_status}] | R: {d.read_speed_mbs:.2f} MB/s, W: {d.write_speed_mbs:.2f} MB/s | {d.power_watts:.1f} W",
                style="cyan",
            ),
        )
        tbl.add_row(
            Text("Розділи диска:", style="dim"),
            Text(part_summary, style="dim white"),
        )
        tbl.add_row(
            Text(f"Мережа ({n.primary_interface}):", style="dim"),
            Text(
                f"↓ {n.download_speed_mbs:.3f} MB/s | ↑ {n.upload_speed_mbs:.3f} MB/s (Всього: ↓{n.total_recv_gb:.2f}GB ↑{n.total_sent_gb:.2f}GB)",
                style="bright_green",
            ),
        )

        return Panel(
            tbl,
            title="[bold bright_blue]💾 ПАМ'ЯТЬ, НАКОПИЧУВАЧІ ТА МЕРЕЖА (RAM · NVMe · NET)[/]",
            border_style="bright_blue",
            box=box.ROUNDED,
        )

    def build_electricity_cost_panel(self, snap: SystemSnapshot) -> Panel:
        p = snap.power
        t = snap.tariff
        fx = snap.fx
        s = snap.session

        weighted_24h_uah = compute_weighted_24h_kwh_rate_uah(t)
        cost_hr_uah = p.total_wall_kw * t.current_rate_uah_kwh
        cost_hr_usd = cost_hr_uah / max(fx.usd_to_uah, 0.01)
        cost_day_uah = p.total_wall_kw * 24.0 * weighted_24h_uah
        cost_day_usd = cost_day_uah / max(fx.usd_to_uah, 0.01)
        cost_month_uah = cost_day_uah * 30.0
        cost_month_usd = cost_day_usd * 30.0

        spark = make_sparkline(list(self.wall_watts_history), width=26)

        tbl = Table.grid(expand=True, padding=(0, 1))
        tbl.add_column(ratio=2)
        tbl.add_column(ratio=3)

        tbl.add_row(
            Text("Споживання з розетки (AC):", style="dim"),
            Text(
                f"⚡ {p.total_wall_watts:.1f} W  ({p.total_wall_kw:.3f} кВт)  {spark}",
                style="bold bright_yellow",
            ),
        )
        tbl.add_row(
            Text("Баланс компонентів (DC):", style="dim"),
            Text(
                f"CPU {p.cpu_watts:.1f}W + GPU {p.gpu_watts:.1f}W + RAM {p.ram_watts:.1f}W + NVMe {p.disk_watts:.1f}W + Mobo {p.motherboard_fans_watts:.1f}W = {p.total_dc_watts:.1f}W",
                style="white",
            ),
        )
        tbl.add_row(
            Text(f"Блок живлення ({p.psu_rating}):", style="dim"),
            Text(
                f"ККД {p.psu_efficiency * 100:.0f}% | Втрати тепла БЖ: +{p.psu_loss_watts:.1f} W",
                style="dim cyan",
            ),
        )
        tbl.add_row(
            Text("Активний тариф зараз:", style="dim"),
            Text(
                f"{t.name} → {t.current_rate_uah_kwh:.2f} ₴/кВт·год (День {t.day_rate_uah_kwh:.2f} ₴ / Ніч {t.night_rate_uah_kwh:.2f} ₴)",
                style="bold green" if t.is_night_discount_active else "yellow",
            ),
        )
        tbl.add_row(
            Text("Витрати (Година / Доба / Міс):", style="dim"),
            Text(
                f"Година: {cost_hr_uah:.2f} ₴ (${cost_hr_usd:.3f}) | Доба: {cost_day_uah:.2f} ₴ (${cost_day_usd:.2f}) | Місяць: {cost_month_uah:.0f} ₴ (${cost_month_usd:.1f})",
                style="bold bright_red",
            ),
        )
        tbl.add_row(
            Text(f"З'їдено за поточну сесію ({format_duration(s.elapsed_seconds)}):", style="dim"),
            Text(
                f"{s.energy_wh:.3f} Вт·год ({s.energy_kwh:.5f} кВт·год)  →  {s.cost_uah:.4f} ₴  (${s.cost_usd:.5f} / €{s.cost_eur:.5f})",
                style="bold bright_white",
            ),
        )

        return Panel(
            tbl,
            title="[bold bright_yellow]🔌 ЕЛЕКТРОЕНЕРГІЯ ТА ЛІЧИЛЬНИК КОШТІВ (Скільки з'їв ПК)[/]",
            border_style="bright_yellow",
            box=box.ROUNDED,
        )

    def build_mining_rental_panel(self, snap: SystemSnapshot) -> Panel:
        m = snap.mining
        s = snap.session

        tbl = Table.grid(expand=True, padding=(0, 1))
        tbl.add_column(ratio=2)
        tbl.add_column(ratio=3)

        profit_style = "bold bright_green" if m.is_profitable_now else "bold bright_red"
        sign = "+" if m.net_profit_uah_hr >= 0 else ""

        tbl.add_row(
            Text("Профіль роботи сервера:", style="dim"),
            Text(m.workload_profile, style="bold bright_white"),
        )
        tbl.add_row(
            Text("Валовий дохід (Оренда/Майнінг):", style="dim"),
            Text(
                f"+{m.gross_revenue_uah_hr:.2f} ₴/год  (+${m.gross_revenue_usd_hr:.3f}/год)  |  +{m.gross_revenue_uah_hr * 24:.1f} ₴/доба",
                style="cyan",
            ),
        )
        tbl.add_row(
            Text("Чистий прибуток (Зараз / Доба):", style="dim"),
            Text(
                f"{sign}{m.net_profit_uah_hr:.2f} ₴/год (${m.net_profit_usd_hr:+.3f}/год)  |  Доба (Д+Н): {m.net_profit_uah_day_24h:+.1f} ₴ (${m.net_profit_usd_day_24h:+.2f})",
                style=profit_style,
            ),
        )
        tbl.add_row(
            Text("Прогноз за 30 днів / Нічний режим:", style="dim"),
            Text(
                f"Місяць 24/7: {m.net_profit_uah_month_30d:+.0f} ₴ (${m.net_profit_usd_month_30d:+.1f})  |  Тільки ніч (8 год): +{m.night_only_profit_uah_8h * 30:.0f} ₴/міс",
                style="bold bright_green",
            ),
        )
        tbl.add_row(
            Text("Частка електрики / Беззбитковість:", style="dim"),
            Text(
                f"Електрика забирає {m.electricity_share_percent:.1f}% доходу | Мін. оренда: ${m.break_even_rental_usd_hr:.3f}/год | Макс. тариф: {m.break_even_tariff_uah_kwh:.1f} ₴/кВт",
                style="yellow",
            ),
        )
        tbl.add_row(
            Text("Статус рентабельності / Сесія:", style="dim"),
            Text(
                f"[{m.efficiency_label}]  |  За сесію чистими: {s.net_profit_uah:+.4f} ₴ (${s.net_profit_usd:+.5f})",
                style=profit_style,
            ),
        )

        return Panel(
            tbl,
            title="[bold bright_green]💰 РЕЖИМ МАЙНІНГУ ТА ОРЕНДИ СЕРВЕРА (PROFITABILITY & ROI)[/]",
            border_style="bright_green",
            box=box.ROUNDED,
        )

    def build_processes_panel(self, snap: SystemSnapshot) -> Panel:
        tbl = Table(
            box=box.SIMPLE_HEAVY,
            expand=True,
            show_edge=False,
            header_style="bold cyan",
            padding=(0, 1),
        )
        tbl.add_column("PID", style="dim", width=8)
        tbl.add_column("Процес (Process)", style="bold white", min_width=24, ratio=3)
        tbl.add_column("CPU %", justify="right", width=9)
        tbl.add_column("RAM (MB)", justify="right", width=11)
        tbl.add_column("Споживання (W)", justify="right", style="bold yellow", width=15)
        tbl.add_column("Вартість (₴/доба)", justify="right", style="bright_red", width=18)

        for p in snap.top_processes[:5]:
            tbl.add_row(
                str(p.pid),
                p.name,
                f"{p.cpu_percent:.1f}%",
                f"{p.memory_mb:.1f}",
                f"{p.estimated_watts:.2f} W",
                f"{p.cost_uah_per_day:.3f} ₴",
            )
        if not snap.top_processes:
            tbl.add_row("-", "Немає активних навантажень", "0.0%", "0.0", "0.00 W", "0.000 ₴")

        return Panel(
            tbl,
            title="[bold white]🔥 ТОП ПРОЦЕСІВ ЗА ЕНЕРГОСПОЖИВАННЯМ (PER-PROCESS WATTAGE)[/]",
            border_style="white",
            box=box.ROUNDED,
        )

    def render_full_dashboard(self, snap: SystemSnapshot) -> RenderableType:
        self.push_snapshot(snap)

        header = self.build_header_panel(snap)
        hw_row = Columns(
            [self.build_cpu_panel(snap), self.build_gpu_panel(snap)],
            expand=True,
            equal=True,
        )
        sys_panel = self.build_system_io_panel(snap)
        fin_row = Columns(
            [
                self.build_electricity_cost_panel(snap),
                self.build_mining_rental_panel(snap),
            ],
            expand=True,
            equal=True,
        )
        proc_panel = self.build_processes_panel(snap)

        return Group(header, hw_row, sys_panel, fin_row, proc_panel)


def render_tariffs_view(
    console: Console,
    profiles: List[TariffProfile],
    active_profile_id: str,
    fx: ExchangeRates,
    crypto: CryptoMarketSnapshot,
) -> None:
    fx_panel = Panel(
        f"[bold bright_green]Курси валют ({fx.source}, оновлено {fx.updated_at}):[/] "
        f"1 USD = [bold yellow]{fx.usd_to_uah:.2f} ₴[/]  |  "
        f"1 EUR = [bold yellow]{fx.eur_to_uah:.2f} ₴[/]  |  "
        f"1 PLN = [bold yellow]{fx.pln_to_uah:.2f} ₴[/]\n"
        f"[bold bright_magenta]Крипто / GPU Compute ринок ({crypto.source}):[/] "
        f"BTC: [bold white]${crypto.btc_usd:,.2f}[/]  |  "
        f"ETH: [bold white]${crypto.eth_usd:,.2f}[/]  |  "
        f"SOL: [bold white]${crypto.sol_usd:,.2f}[/]  |  "
        f"RENDER: [bold white]${crypto.render_usd:,.3f}[/]",
        title="[bold cyan]🌐 АВТООНОВЛЕННЯ ЦІН ТА КУРСІВ З ІНТЕРНЕТУ[/]",
        border_style="cyan",
        box=box.ROUNDED,
    )
    console.print(fx_panel)

    tbl = Table(
        title="⚡ Доступні тарифи на електроенергію (Україна та ЄС ENTSO-E Spot)",
        box=box.ROUNDED,
        header_style="bold bright_cyan",
        expand=True,
    )
    tbl.add_column("ID профілю", style="bold white", min_width=15, no_wrap=True)
    tbl.add_column("Назва тарифу", min_width=32, ratio=2)
    tbl.add_column("Зараз (₴/кВт·год)", justify="right", style="bold yellow", min_width=14)
    tbl.add_column("Зараз ($/кВт·год)", justify="right", style="cyan", min_width=14)
    tbl.add_column("День (₴)", justify="right", min_width=9)
    tbl.add_column("Ніч 23-07 (₴)", justify="right", style="bright_green", min_width=12)
    tbl.add_column("Активна зона", justify="center", min_width=22)
    tbl.add_column("Джерело", style="dim", min_width=18)

    for p in profiles:
        is_active = p.profile_id == active_profile_id
        prefix = "▶ " if is_active else "  "
        id_style = "bold bright_green" if is_active else "white"
        tbl.add_row(
            Text(f"{prefix}{p.profile_id}", style=id_style),
            p.name,
            f"{p.current_rate_uah_kwh:.3f} ₴",
            f"${p.rate_in_usd(fx):.4f}",
            f"{p.day_rate_uah_kwh:.2f} ₴",
            f"{p.night_rate_uah_kwh:.3f} ₴",
            p.current_zone_label,
            p.source,
        )

    console.print(tbl)


def render_mining_scenarios_view(
    console: Console,
    snap: SystemSnapshot,
    scenarios: List[WorkloadScenarioRow],
) -> None:
    renderer = DashboardRenderer()
    console.print(renderer.build_header_panel(snap))
    console.print(
        Columns(
            [
                renderer.build_electricity_cost_panel(snap),
                renderer.build_mining_rental_panel(snap),
            ],
            expand=True,
            equal=True,
        )
    )

    tbl = Table(
        title="📊 Симуляція режимів оренди сервера та майнінгу для вашого ПК (Розрахунок витрат та чистого прибутку)",
        box=box.ROUNDED,
        header_style="bold bright_green",
        expand=True,
    )
    tbl.add_column("Сценарій навантаження ПК", min_width=34, ratio=2, style="bold white")
    tbl.add_column("З розетки (W)", justify="right", style="yellow", min_width=11)
    tbl.add_column("кВт·год/доба", justify="right", min_width=10)
    tbl.add_column("Електрика/доба", justify="right", style="red", min_width=12)
    tbl.add_column("Дохід/доба", justify="right", style="cyan", min_width=11)
    tbl.add_column("Чистий/доба (₴)", justify="right", style="bold bright_green", min_width=13)
    tbl.add_column("Чистий/міс 24/7", justify="right", style="bold bright_green", min_width=18)
    tbl.add_column("Тільки Ніч (8г/міс)", justify="right", style="bright_cyan", min_width=14)
    tbl.add_column("Статус ROI", justify="center", min_width=15)

    for row in scenarios:
        net_style = "bold bright_green" if row.net_day_uah >= 0 else "bold bright_red"
        tbl.add_row(
            row.scenario_name,
            f"{row.wall_watts:.1f} W",
            f"{row.kwh_per_day:.2f}",
            f"-{row.cost_day_uah:.2f} ₴",
            f"+{row.gross_day_uah:.2f} ₴",
            Text(f"{row.net_day_uah:+.2f} ₴", style=net_style),
            Text(
                f"{row.net_month_uah:+.0f} ₴ (${row.net_month_usd:+.1f})",
                style=net_style,
            ),
            f"+{row.night_8h_net_month_uah:.0f} ₴",
            row.roi_status,
        )

    console.print(tbl)
