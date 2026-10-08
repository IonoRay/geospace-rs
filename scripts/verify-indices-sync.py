#!/usr/bin/env python3
"""Exercise real upstream refresh and instrumented offline CLI recovery.

Run through the Nix devShell after building geospace with cli-standard.
Every run creates its own home and evidence directory; existing homes are unused.
"""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/geospace"))
    parser.add_argument("--output", type=Path, default=Path("target/indices-acceptance"))
    parser.add_argument("--online", action="store_true")
    parser.add_argument("--seed-home", type=Path, help="Copy a previously verified home for offline replay")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=True)
    evidence = Path(tempfile.mkdtemp(prefix="run-", dir=args.output.resolve()))
    home = evidence / "home"
    if args.seed_home:
        shutil.copytree(args.seed_home.resolve(strict=True), home, symlinks=True)
    results = []
    datasets = ["dst", "ae", "kp-ap-f107", "iri-ig-rz", "iri-apf107"]

    def run(name, command, environment=None, allowed=(0,)):
        process = subprocess.run(
            [str(binary), "--home", str(home), *command],
            env={**os.environ, "IONORAY_LOG_COLOR": "never", **(environment or {})},
            text=True, capture_output=True, timeout=1200, check=False,
        )
        (evidence / f"{name}.json").write_text(process.stdout)
        (evidence / f"{name}.ndjson").write_text(process.stderr)
        row = {"name": name, "exit_code": process.returncode,
               "allowed": list(allowed), "passed": process.returncode in allowed}
        results.append(row)
        print(json.dumps(row), flush=True)
        return json.loads(process.stdout) if process.stdout.strip() else None

    def interval(dataset, mode):
        return ["indices", "sync-range", "--dataset", dataset, "--mode", mode,
                "--start", "2020-07-01T12:00:00Z", "--end", "2020-07-01T13:00:00Z"]

    run("init", ["data", "init"])
    if args.online:
        for dataset in datasets:
            report = run(f"online-{dataset}", interval(dataset, "refresh"))
            if report and not any(attempt.get("origin") == "upstream" and attempt.get("committed")
                                  for attempt in report["attempts"]):
                results.append({"name": f"upstream-evidence-{dataset}", "passed": False})

    class Counter(http.server.BaseHTTPRequestHandler):
        requests = 0

        def reject(self):
            type(self).requests += 1
            self.send_error(502, "instrumented proxy: network access detected")

        do_CONNECT = reject
        do_GET = reject
        do_HEAD = reject

        def log_message(self, *unused):
            pass

    proxy = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Counter)
    thread = threading.Thread(target=proxy.serve_forever, daemon=True)
    thread.start()
    address = f"http://127.0.0.1:{proxy.server_port}"
    offline_environment = {key: address for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY",
                                                    "http_proxy", "https_proxy", "all_proxy"]}
    offline_environment.update({"NO_PROXY": "", "no_proxy": ""})
    try:
        # Positive control proves the subprocess's HTTP client uses this counter.
        run("proxy-positive-control", interval("dst", "refresh"), offline_environment, allowed=(0, 2))
        control = Counter.requests
        if control == 0:
            raise RuntimeError("proxy positive control did not observe an HTTP request")
        for dataset in datasets:
            run(f"offline-{dataset}", interval(dataset, "offline"), offline_environment,
                allowed=(0, 2) if dataset in {"dst", "ae"} else (0,))
        # Rebuild annual databases from accepted raw bodies, without HTTP.
        shutil.move(str(home / "indices"), evidence / "saved-year-databases")
        for dataset in datasets:
            run(f"rebuild-offline-{dataset}", interval(dataset, "offline"), offline_environment,
                allowed=(0,) if args.online or args.seed_home or dataset not in {"dst", "ae"} else (2,))
        original_home = home
        home = evidence / "empty-offline-home"
        run("empty-offline-dst", interval("dst", "offline"), offline_environment, allowed=(2,))
        run("empty-offline-gfz", interval("kp-ap-f107", "offline"), offline_environment)
        home = original_home
        # A missing Kyoto body cannot be reconstructed from the packaged cache.
        bodies = list((home / "objects/dst/sha256").glob("*/*"))
        if bodies:
            body = bodies[0]
            saved = evidence / "temporarily-missing-body"
            shutil.move(str(body), saved)
            try:
                run("missing-body-offline", interval("dst", "offline"), offline_environment, allowed=(2,))
            finally:
                shutil.move(str(saved), body)
        offline_requests = Counter.requests - control
        if offline_requests:
            raise RuntimeError(f"offline issued {offline_requests} HTTP requests")
        moved = evidence / "moved-home"
        shutil.move(str(home), moved)
        home = moved
        run("moved-offline", interval("kp-ap-f107", "offline"), offline_environment)
        if args.online or args.seed_home:
            run("moved-read", ["indices", "read", "--at", "2020-07-01T12:00:00Z"], offline_environment)
        if Counter.requests != control:
            raise RuntimeError("moved offline home issued HTTP requests")
    finally:
        proxy.shutdown()
        proxy.server_close()
        thread.join()

    objects = []
    links = []
    for path in (home / "objects").rglob("*"):
        if path.is_symlink():
            target = os.readlink(path)
            if os.path.isabs(target) or not path.resolve(strict=True).is_relative_to(home):
                raise RuntimeError(f"invalid relative CAS view: {path}")
            links.append(str(path.relative_to(home)))
        elif path.is_file() and path.parent.parent.name == "sha256":
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            if actual != path.name:
                raise RuntimeError(f"CAS digest mismatch: {path}")
            objects.append(str(path.relative_to(home)))
    if (home / "objects/catalog.db").exists() or (home / "objects/sha256").exists():
        raise RuntimeError("indices created a global catalog/CAS")
    summary = {"home": str(home), "online_requested": args.online,
               "seed_home": str(args.seed_home) if args.seed_home else None,
               "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
               "proxy_positive_control_requests": control, "offline_requests": offline_requests,
               "objects_verified": objects, "relative_links_verified": links, "runs": results}
    (evidence / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"Evidence: {evidence}", flush=True)
    if not all(row["passed"] for row in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
