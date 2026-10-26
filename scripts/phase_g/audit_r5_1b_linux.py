"""Read-only snapshot of the Coord Linux relay hop for Phase R5-1B."""

import json
import socket
import subprocess
import urllib.request
from datetime import datetime, timezone


def run(*args: str) -> tuple[int, str]:
    result = subprocess.run(args, text=True, capture_output=True, check=False)
    return result.returncode, result.stdout.strip()


services = {}
for unit in ("coord-relay.service", "feathersjsapp.service", "openresty.service"):
    code, output = run(
        "systemctl",
        "show",
        unit,
        "--property=ActiveState,SubState,MainPID,NRestarts,CanReload",
        "--no-pager",
    )
    services[unit] = {"exit_code": code, "properties": output.splitlines()}

code, listeners = run("ss", "-ltn", "sport", "=", ":13011", "or", "sport", "=", ":13012", "or", "sport", "=", ":13013")
health = {"state": "unavailable"}
try:
    with urllib.request.urlopen("http://127.0.0.1:13013/healthz", timeout=3) as response:
        health = {
            "http_status": response.status,
            "body": json.loads(response.read(16 * 1024).decode("utf-8")),
        }
except Exception as error:  # the sanitized category is enough for this read-only gate
    health = {"error": type(error).__name__}

print(
    json.dumps(
        {
            "state": "PASS",
            "observed_at_utc": datetime.now(timezone.utc).isoformat(),
            "host": socket.gethostname(),
            "services": services,
            "coord_listeners": {"exit_code": code, "output": listeners},
            "sidecar_health": health,
        },
        sort_keys=True,
    )
)
