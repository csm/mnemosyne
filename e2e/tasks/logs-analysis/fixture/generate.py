#!/usr/bin/env python3
"""Generate synthetic nginx combined-format access logs with injected
ground-truth phenomena, per doc/E2E-TESTING.md scenario 4 (`logs-analysis`).

Runs on the HOST at task-build time (see ../fixture/Dockerfile, which COPYs
only the generated `logs/` directory into the image). `answers.json` is
written next to the logs on the host and must never be baked into the
image -- run.sh's task build only copies the `logs/` subdirectory into
/task/logs, never this script's other output.

Usage:
    python3 generate.py --seed 42 --out-dir /tmp/run42 --hours 6

Produces:
    <out-dir>/logs/access-YYYYMMDD-HH.log   (one file per hour, combined format)
    <out-dir>/answers.json                   (ground truth; HOST-ONLY)

Injected phenomena (all reproducible from --seed alone):
  1. A handful of "heavy hitter" IPs given amplified response sizes, so
     "top N client IPs by total bytes" has an unambiguous, tie-free answer.
  2. A scraper bot: a distinctive, fixed User-Agent rotating through a
     small IP block, crawling sequential paths at a steady clip.
  3. A single 5xx burst: for exactly 14 minutes, one endpoint fails at a
     high rate while everything else stays healthy, so "the worst 5xx
     window" is unambiguous.
  4. A low-and-slow credential-stuffing pattern: many distinct IPs each
     make a handful of failed /login attempts, spread across the whole
     window rather than bursty.
"""
from __future__ import annotations

import argparse
import ipaddress
import json
import random
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

NORMAL_ENDPOINTS = [
    ("/", 200, (800, 4000)),
    ("/api/items", 200, (400, 2200)),
    ("/api/items/17", 200, (300, 1500)),
    ("/api/items/42", 200, (300, 1500)),
    ("/static/app.css", 200, (2000, 6000)),
    ("/static/app.js", 200, (4000, 12000)),
    ("/about", 200, (900, 1800)),
    ("/contact", 200, (900, 1800)),
    ("/favicon.ico", 404, (150, 300)),
]

SEARCH_ENDPOINT = "/api/search"
LOGIN_ENDPOINT = "/login"
DOWNLOAD_ENDPOINT = "/downloads/report.pdf"

SCRAPER_UA = "ContentHarvester/3.1 (+http://crawler.example.invalid/bot)"

NORMAL_UAS = [
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) "
    "Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) "
    "Version/17.4 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64; rv:126.0) Gecko/20100101 Firefox/126.0",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 "
    "(KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
]

# RFC 5737 documentation ranges: never route anywhere real.
NORMAL_NET = ipaddress.ip_network("198.51.100.0/24")
HEAVY_HITTER_NET = ipaddress.ip_network("198.51.100.0/24")
SCRAPER_NET = ipaddress.ip_network("203.0.113.0/28")
CREDSTUFF_NET = ipaddress.ip_network("192.0.2.0/24")

MONTHS = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
]


@dataclass
class Record:
    ip: str
    ts: datetime
    method: str
    path: str
    status: int
    bytes_sent: int
    referer: str
    user_agent: str

    def format(self) -> str:
        time_local = (
            f"{self.ts.day:02d}/{MONTHS[self.ts.month - 1]}/{self.ts.year}"
            f":{self.ts.hour:02d}:{self.ts.minute:02d}:{self.ts.second:02d} +0000"
        )
        return (
            f'{self.ip} - - [{time_local}] '
            f'"{self.method} {self.path} HTTP/1.1" {self.status} {self.bytes_sent} '
            f'"-" "{self.user_agent}"'
        )


def pick_ips(rng: random.Random, network: ipaddress.IPv4Network, n: int) -> list[str]:
    hosts = list(network.hosts())
    rng.shuffle(hosts)
    return [str(ip) for ip in hosts[:n]]


def gen_normal_traffic(rng: random.Random, start: datetime, hours: int, ip_pool: list[str],
                        events_per_hour: int) -> list[Record]:
    records = []
    for h in range(hours):
        hour_start = start + timedelta(hours=h)
        for _ in range(events_per_hour):
            ip = rng.choice(ip_pool)
            offset = timedelta(seconds=rng.uniform(0, 3600))
            path, status, size_range = rng.choice(NORMAL_ENDPOINTS)
            records.append(Record(
                ip=ip, ts=hour_start + offset, method="GET", path=path, status=status,
                bytes_sent=rng.randint(*size_range),
                referer="-", user_agent=rng.choice(NORMAL_UAS),
            ))
    return records


def gen_heavy_hitters(rng: random.Random, start: datetime, hours: int,
                       heavy_ips: list[str]) -> list[Record]:
    records = []
    for h in range(hours):
        hour_start = start + timedelta(hours=h)
        for ip in heavy_ips:
            for _ in range(rng.randint(15, 25)):
                offset = timedelta(seconds=rng.uniform(0, 3600))
                records.append(Record(
                    ip=ip, ts=hour_start + offset, method="GET", path=DOWNLOAD_ENDPOINT,
                    status=200, bytes_sent=rng.randint(2_000_000, 6_000_000),
                    referer="-", user_agent=rng.choice(NORMAL_UAS),
                ))
    return records


def gen_scraper(rng: random.Random, start: datetime, hours: int,
                 scraper_ips: list[str]) -> list[Record]:
    records = []
    page = 1
    for h in range(hours):
        hour_start = start + timedelta(hours=h)
        # steady crawl: one request every few seconds, rotating IPs and
        # walking sequential product pages.
        t = 0.0
        while t < 3600:
            ip = rng.choice(scraper_ips)
            records.append(Record(
                ip=ip, ts=hour_start + timedelta(seconds=t), method="GET",
                path=f"/product/{page}", status=200,
                bytes_sent=rng.randint(1200, 3000),
                referer="-", user_agent=SCRAPER_UA,
            ))
            page += 1
            t += rng.uniform(2.0, 5.0)
    return records


def gen_5xx_burst(rng: random.Random, burst_start: datetime) -> list[Record]:
    records = []
    duration = timedelta(minutes=14)
    t = timedelta(0)
    while t < duration:
        ip = rng.choice(pick_ips(rng, NORMAL_NET, 30))
        records.append(Record(
            ip=ip, ts=burst_start + t, method="GET", path=SEARCH_ENDPOINT,
            status=500, bytes_sent=rng.randint(200, 500),
            referer="-", user_agent=rng.choice(NORMAL_UAS),
        ))
        t += timedelta(seconds=rng.uniform(1.0, 3.0))
    return records


def gen_background_5xx_noise(rng: random.Random, start: datetime, hours: int,
                              ip_pool: list[str]) -> list[Record]:
    """A little background 5xx everywhere so the injected burst has to be
    identified by rate/duration, not by "any 500 at all"."""
    records = []
    for h in range(hours):
        hour_start = start + timedelta(hours=h)
        for _ in range(rng.randint(0, 3)):
            offset = timedelta(seconds=rng.uniform(0, 3600))
            path, _, _ = rng.choice(NORMAL_ENDPOINTS)
            records.append(Record(
                ip=rng.choice(ip_pool), ts=hour_start + offset, method="GET",
                path=path, status=500, bytes_sent=rng.randint(200, 500),
                referer="-", user_agent=rng.choice(NORMAL_UAS),
            ))
    return records


def gen_credential_stuffing(rng: random.Random, start: datetime, hours: int,
                             attacker_ips: list[str]) -> list[Record]:
    records = []
    total_seconds = hours * 3600
    for ip in attacker_ips:
        attempts = rng.randint(2, 5)
        for _ in range(attempts):
            offset = timedelta(seconds=rng.uniform(0, total_seconds))
            records.append(Record(
                ip=ip, ts=start + offset, method="POST", path=LOGIN_ENDPOINT,
                status=rng.choice([401, 401, 403]), bytes_sent=rng.randint(80, 200),
                referer="-", user_agent=rng.choice(NORMAL_UAS),
            ))
    return records


def gen_legitimate_logins(rng: random.Random, start: datetime, hours: int,
                           ip_pool: list[str]) -> list[Record]:
    records = []
    for h in range(hours):
        hour_start = start + timedelta(hours=h)
        for _ in range(rng.randint(3, 8)):
            offset = timedelta(seconds=rng.uniform(0, 3600))
            records.append(Record(
                ip=rng.choice(ip_pool), ts=hour_start + offset, method="POST",
                path=LOGIN_ENDPOINT, status=200, bytes_sent=rng.randint(100, 300),
                referer="-", user_agent=rng.choice(NORMAL_UAS),
            ))
    return records


def build(seed: int, hours: int, events_per_hour: int) -> tuple[list[Record], dict]:
    rng = random.Random(seed)
    start = datetime(2026, 1, 1, tzinfo=timezone.utc) + timedelta(days=seed)

    normal_ips = pick_ips(rng, NORMAL_NET, 120)
    heavy_ips = normal_ips[:5]
    scraper_ips = pick_ips(rng, SCRAPER_NET, 6)
    n_attackers = rng.randint(40, 60)
    attacker_ips = pick_ips(rng, CREDSTUFF_NET, n_attackers)

    # Burst placed at a random offset, at least 1 hour from either edge so
    # the whole window is fully inside the log range.
    burst_hour = rng.randint(1, max(1, hours - 2))
    burst_start = start + timedelta(hours=burst_hour, minutes=rng.randint(0, 40))

    records: list[Record] = []
    records += gen_normal_traffic(rng, start, hours, normal_ips, events_per_hour)
    records += gen_heavy_hitters(rng, start, hours, heavy_ips)
    records += gen_scraper(rng, start, hours, scraper_ips)
    records += gen_5xx_burst(rng, burst_start)
    records += gen_background_5xx_noise(rng, start, hours, normal_ips)
    records += gen_credential_stuffing(rng, start, hours, attacker_ips)
    records += gen_legitimate_logins(rng, start, hours, normal_ips)
    records.sort(key=lambda r: r.ts)

    bytes_by_ip: dict[str, int] = {}
    for r in records:
        bytes_by_ip[r.ip] = bytes_by_ip.get(r.ip, 0) + r.bytes_sent
    top5 = sorted(bytes_by_ip.items(), key=lambda kv: kv[1], reverse=True)[:5]

    answers = {
        "top_5_ips_by_bytes": [ip for ip, _ in top5],
        "top_5_ips_by_bytes_detail": [{"ip": ip, "bytes": b} for ip, b in top5],
        "worst_5xx_window_start_utc": burst_start.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "worst_5xx_window_endpoint": SEARCH_ENDPOINT,
        "worst_5xx_window_duration_minutes": 14,
        "scraper_user_agent": SCRAPER_UA,
        "credential_stuffing_distinct_ip_count": len(attacker_ips),
        "credential_stuffing_endpoint": LOGIN_ENDPOINT,
        "_generation": {"seed": seed, "hours": hours, "events_per_hour": events_per_hour},
    }
    return records, answers


def write_logs(records: list[Record], out_dir: Path, hours: int, start: datetime) -> None:
    logs_dir = out_dir / "logs"
    logs_dir.mkdir(parents=True, exist_ok=True)
    by_hour: dict[str, list[Record]] = {}
    for r in records:
        key = r.ts.strftime("%Y%m%d-%H")
        by_hour.setdefault(key, []).append(r)
    for key, recs in sorted(by_hour.items()):
        path = logs_dir / f"access-{key}.log"
        with open(path, "w") as f:
            for r in sorted(recs, key=lambda x: x.ts):
                f.write(r.format() + "\n")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--out-dir", required=True)
    ap.add_argument("--hours", type=int, default=6,
                     help="log window length; 24+ with a higher --events-per-hour "
                          "approaches the ~500MB target from the plan doc")
    ap.add_argument("--events-per-hour", type=int, default=300)
    args = ap.parse_args()

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    records, answers = build(args.seed, args.hours, args.events_per_hour)
    start = datetime(2026, 1, 1, tzinfo=timezone.utc) + timedelta(days=args.seed)
    write_logs(records, out_dir, args.hours, start)

    (out_dir / "answers.json").write_text(json.dumps(answers, indent=2) + "\n")
    print(f"wrote {len(records)} records to {out_dir / 'logs'}")
    print(f"wrote ground truth to {out_dir / 'answers.json'}")


if __name__ == "__main__":
    main()
