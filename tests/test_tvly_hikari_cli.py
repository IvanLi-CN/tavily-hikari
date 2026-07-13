#!/usr/bin/env python3
import json
import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WRAPPER = ROOT / "scripts" / "tvly-hikari"
INSTALLER = ROOT / "scripts" / "install-tvly-hikari.sh"
VALID_TOKEN = "th-test-secretsecret"


class TvlyHikariCliTest(unittest.TestCase):
    def run_cmd(self, args, *, env, check=True):
        result = subprocess.run(
            args,
            cwd=ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        if check and result.returncode != 0:
            self.fail(
                f"command failed: {args}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
            )
        return result

    def base_env(self, home: Path, fake_bin: Path):
        env = os.environ.copy()
        env["HOME"] = str(home)
        env.pop("TAVILY_HIKARI_BASE_URL", None)
        env.pop("TAVILY_HIKARI_TOKEN", None)
        env.pop("TAVILY_HIKARI_CONFIG_DIR", None)
        env.pop("TAVILY_HIKARI_CONFIG_FILE", None)
        env["PATH"] = f"{fake_bin}:{env.get('PATH', '')}"
        return env

    def write_fake_tvly(self, fake_bin: Path, capture_file: Path):
        fake_tvly = fake_bin / "tvly"
        fake_tvly.write_text(
            "#!/usr/bin/env bash\n"
            "set -euo pipefail\n"
            "if [[ \"${1:-}\" == \"--version\" ]]; then echo 'tavily-cli 0.1.4'; exit 0; fi\n"
            "python3 - \"$@\" <<'PY'\n"
            "import json, os, sys\n"
            "payload = {\n"
            "    'api_base_url': os.environ.get('TAVILY_API_BASE_URL'),\n"
            "    'api_key': os.environ.get('TAVILY_API_KEY'),\n"
            "    'args': sys.argv[1:],\n"
            "}\n"
            f"open({str(capture_file)!r}, 'w', encoding='utf-8').write(json.dumps(payload))\n"
            "PY\n",
            encoding="utf-8",
        )
        fake_tvly.chmod(0o755)

    def test_configure_writes_0600_config(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            env = self.base_env(root / "home", fake_bin)
            config_dir = root / "config"

            self.run_cmd(
                [
                    "bash",
                    str(WRAPPER),
                    "configure",
                    "--base-url",
                    "http://127.0.0.1:58087/",
                    "--token",
                    VALID_TOKEN,
                    "--config-dir",
                    str(config_dir),
                ],
                env=env,
            )

            config_file = config_dir / "config.json"
            data = json.loads(config_file.read_text(encoding="utf-8"))
            self.assertEqual(data, {
                "baseUrl": "http://127.0.0.1:58087",
                "token": VALID_TOKEN,
            })
            self.assertEqual(stat.S_IMODE(config_file.stat().st_mode), 0o600)

    def test_configure_rejects_masked_token(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            env = self.base_env(root / "home", fake_bin)

            result = self.run_cmd(
                [
                    "bash",
                    str(WRAPPER),
                    "configure",
                    "--base-url",
                    "http://127.0.0.1:58087",
                    "--token",
                    "th-test-************************",
                ],
                env=env,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unmasked Hikari token", result.stderr)

    def test_configure_rejects_backend_invalid_token_shape(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            env = self.base_env(root / "home", fake_bin)

            result = self.run_cmd(
                [
                    "bash",
                    str(WRAPPER),
                    "configure",
                    "--base-url",
                    "http://127.0.0.1:58087",
                    "--token",
                    "th-test-secret",
                ],
                env=env,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("th-xxxx-xxxxxxxxxxxx", result.stderr)

    def test_configure_respects_config_file_env(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            custom_config = root / "custom" / "hikari.json"
            env = self.base_env(root / "home", fake_bin)
            env["TAVILY_HIKARI_CONFIG_FILE"] = str(custom_config)

            self.run_cmd(
                [
                    "bash",
                    str(WRAPPER),
                    "configure",
                    "--base-url",
                    "http://127.0.0.1:58087",
                    "--token",
                    VALID_TOKEN,
                ],
                env=env,
            )

            self.assertTrue(custom_config.exists())
            self.assertEqual(stat.S_IMODE(custom_config.stat().st_mode), 0o600)
            result = self.run_cmd(["bash", str(WRAPPER), "config", "show"], env=env)
            self.assertIn("apiBaseUrl: http://127.0.0.1:58087/api/tavily", result.stdout)

    def test_passthrough_injects_hikari_env_for_tvly(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            capture = root / "capture.json"
            self.write_fake_tvly(fake_bin, capture)
            env = self.base_env(root / "home", fake_bin)

            self.run_cmd(
                [
                    "bash",
                    str(WRAPPER),
                    "configure",
                    "--base-url",
                    "http://127.0.0.1:58087",
                    "--token",
                    VALID_TOKEN,
                ],
                env=env,
            )
            self.run_cmd(["bash", str(WRAPPER), "search", "query", "--json"], env=env)

            payload = json.loads(capture.read_text(encoding="utf-8"))
            self.assertEqual(payload["api_base_url"], "http://127.0.0.1:58087/api/tavily")
            self.assertEqual(payload["api_key"], VALID_TOKEN)
            self.assertEqual(payload["args"], ["search", "query", "--json"])

    def test_installer_installs_wrapper_config_and_optional_skills(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "fake-bin"
            install_bin = root / "install-bin"
            config_dir = root / "config"
            fake_bin.mkdir()
            install_bin.mkdir()
            (fake_bin / "tvly").write_text(
                "#!/usr/bin/env bash\n"
                "if [[ \"${1:-}\" == \"--version\" ]]; then echo 'tavily-cli 0.1.4'; exit 0; fi\n"
                "exit 0\n",
                encoding="utf-8",
            )
            (fake_bin / "tvly").chmod(0o755)
            npx_log = root / "npx.log"
            (fake_bin / "npx").write_text(
                "#!/usr/bin/env bash\n"
                f"printf '%s\\n' \"$*\" > {str(npx_log)!r}\n",
                encoding="utf-8",
            )
            (fake_bin / "npx").chmod(0o755)

            env = self.base_env(root / "home", fake_bin)
            env["HIKARI_INSTALL_LOCAL_TVLY_HIKARI"] = str(WRAPPER)

            self.run_cmd(
                [
                    str(INSTALLER),
                    "--base-url",
                    "http://127.0.0.1:58087",
                    "--token",
                    VALID_TOKEN,
                    "--install-dir",
                    str(install_bin),
                    "--config-dir",
                    str(config_dir),
                    "--with-skills",
                ],
                env=env,
            )

            installed = install_bin / "tvly-hikari"
            self.assertTrue(installed.exists())
            self.assertTrue(os.access(installed, os.X_OK))
            self.assertEqual(stat.S_IMODE((config_dir / "config.json").stat().st_mode), 0o600)
            result = self.run_cmd([str(installed), "config", "show"], env=env)
            self.assertIn(f"config: {config_dir / 'config.json'}", result.stdout)
            self.assertEqual(
                npx_log.read_text(encoding="utf-8").strip(),
                "skills add https://github.com/IvanLi-CN/tavily-hikari --global",
            )

    def test_installer_default_skips_skills_without_failing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "fake-bin"
            install_bin = root / "install-bin"
            config_dir = root / "config"
            fake_bin.mkdir()
            install_bin.mkdir()
            (fake_bin / "tvly").write_text(
                "#!/usr/bin/env bash\n"
                "if [[ \"${1:-}\" == \"--version\" ]]; then echo 'tavily-cli 0.1.4'; exit 0; fi\n"
                "exit 0\n",
                encoding="utf-8",
            )
            (fake_bin / "tvly").chmod(0o755)

            env = self.base_env(root / "home", fake_bin)
            env["HIKARI_INSTALL_LOCAL_TVLY_HIKARI"] = str(WRAPPER)

            self.run_cmd(
                [
                    str(INSTALLER),
                    "--base-url",
                    "http://127.0.0.1:58087",
                    "--token",
                    VALID_TOKEN,
                    "--install-dir",
                    str(install_bin),
                    "--config-dir",
                    str(config_dir),
                ],
                env=env,
            )

            installed = install_bin / "tvly-hikari"
            self.assertTrue(installed.exists())
            self.assertEqual(stat.S_IMODE((config_dir / "config.json").stat().st_mode), 0o600)

    def test_old_tvly_is_rejected_before_passthrough(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            (fake_bin / "tvly").write_text(
                "#!/usr/bin/env bash\n"
                "if [[ \"${1:-}\" == \"--version\" ]]; then echo 'tavily-cli 0.1.2'; exit 0; fi\n"
                "exit 0\n",
                encoding="utf-8",
            )
            (fake_bin / "tvly").chmod(0o755)
            env = self.base_env(root / "home", fake_bin)

            self.run_cmd(
                [
                    "bash",
                    str(WRAPPER),
                    "configure",
                    "--base-url",
                    "http://127.0.0.1:58087",
                    "--token",
                    VALID_TOKEN,
                ],
                env=env,
            )
            result = self.run_cmd(["bash", str(WRAPPER), "search", "query", "--json"], env=env, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("too old", result.stderr)


if __name__ == "__main__":
    unittest.main()
