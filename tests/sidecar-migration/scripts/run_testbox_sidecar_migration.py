import os
import subprocess
import sys
import time


ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO_ROOT = os.path.dirname(os.path.dirname(ROOT))
RUNTIME_DIR = os.path.join(ROOT, "runtime")
DB_PATH = os.path.join(RUNTIME_DIR, "data", "tavily_proxy.db")
COMPOSE_FILE = os.path.join(ROOT, "docker-compose.yml")
PROJECT = os.environ.get("COMPOSE_PROJECT", "tavily_sidecar_migration")
CAPS_OVERRIDE = os.path.join(ROOT, ".codex.caps-compat.yaml")


def run(*args, **kwargs):
    print("+", " ".join(args), flush=True)
    subprocess.run(args, check=True, **kwargs)


def compose_args():
    args = ["docker", "compose", "-p", PROJECT, "-f", COMPOSE_FILE]
    if os.path.exists(CAPS_OVERRIDE):
        args.extend(["-f", CAPS_OVERRIDE])
    return args


def main():
    os.makedirs(os.path.join(RUNTIME_DIR, "data"), exist_ok=True)
    run(sys.executable, os.path.join(ROOT, "scripts", "seed_large_legacy_db.py"), DB_PATH)
    run(
        "cargo",
        "run",
        "--quiet",
        "--bin",
        "observability_sidecar_migrate",
        "--",
        "--db-path",
        DB_PATH,
        "--batch-size",
        "1",
    )

    run(*compose_args(), "up", "-d", "--build")
    try:
        time.sleep(5)
        run(
            *compose_args(),
            "exec",
            "-T",
            "verify",
            "python",
            "/work/scripts/run_sidecar_smoke.py",
        )
    finally:
        run(*compose_args(), "down", "-v", "--remove-orphans")


if __name__ == "__main__":
    main()
