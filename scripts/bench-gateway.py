#!/usr/bin/env python3
"""Gateway latency against a running stack, measured from outside the process.

Uses one keep-alive connection (like the browser) and reports client-observed latency
plus the gateway's own Server-Timing total, so network/Docker overhead and in-process
cost can be told apart. Standard library only.

    python3 scripts/bench-gateway.py [--url http://localhost:8080] [-n 2000]
"""

import argparse
import http.client
import json
import os
import statistics
import time
import urllib.parse
from pathlib import Path


def percentile(values: list[float], p: float) -> float:
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, round(p / 100 * len(ordered) + 0.5) - 1))
    return ordered[index]


def server_total_ms(header: str | None) -> float | None:
    for metric in (header or "").split(","):
        name, _, rest = metric.strip().partition(";")
        if name == "total" and rest.startswith("dur="):
            return float(rest[4:])
    return None


def run(conn: http.client.HTTPConnection, path: str, n: int, headers: dict[str, str]) -> dict[str, float]:
    client, server = [], []
    for i in range(n + n // 10):  # first 10% is warm-up
        started = time.perf_counter()
        conn.request("GET", path, headers=headers)
        response = conn.getresponse()
        response.read()
        elapsed = (time.perf_counter() - started) * 1000
        if response.status != 200:
            raise SystemExit(f"{path} returned HTTP {response.status}")
        if i >= n // 10:
            client.append(elapsed)
            total = server_total_ms(response.getheader("server-timing"))
            if total is not None:
                server.append(total)
    summary = {
        "client_p50_ms": percentile(client, 50),
        "client_p95_ms": percentile(client, 95),
        "client_p99_ms": percentile(client, 99),
        "client_mean_ms": statistics.fmean(client),
    }
    if server:
        summary |= {"server_p50_ms": percentile(server, 50), "server_p95_ms": percentile(server, 95)}
    return {k: round(v, 3) for k, v in summary.items()}


def login(host: str, port: int, email: str, password: str) -> str:
    conn = http.client.HTTPConnection(host, port, timeout=10)
    body = json.dumps({"email": email, "password": password})
    conn.request("POST", "/api/auth/login", body=body, headers={"content-type": "application/json"})
    response = conn.getresponse()
    response.read()
    if response.status != 200:
        raise SystemExit(f"login failed: HTTP {response.status}")
    cookie = response.getheader("set-cookie") or ""
    return cookie.split(";", 1)[0]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://localhost:8080")
    parser.add_argument("-n", type=int, default=2000)
    parser.add_argument("--email", default="demo@lattice.dev")
    parser.add_argument("--password", default="lattice-demo")
    parser.add_argument("--out", default=str(Path(__file__).resolve().parent.parent / "perf-results" / "gateway.json"))
    args = parser.parse_args()

    url = urllib.parse.urlparse(args.url)
    host, port = url.hostname or "localhost", url.port or 80
    conn = http.client.HTTPConnection(host, port, timeout=10)

    results = {"healthz": run(conn, "/healthz", args.n, {})}
    cookie = login(host, port, args.email, args.password)
    results["api_projects_authenticated"] = run(conn, "/api/projects", args.n // 2, {"cookie": cookie})

    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "w") as f:
        json.dump(results, f, indent=2)
        f.write("\n")
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
