#!/usr/bin/env python3
import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SKILLS_DIR = ROOT / "skills"
REPO_URL = "https://github.com/IvanLi-CN/tavily-hikari"
GLOBAL_COMMAND = f"npx skills add {REPO_URL} --global"
SKILLS = {
    "tavily-hikari-best-practices": "Guide agents to use Tavily Hikari safely through tvly-hikari.",
    "tavily-hikari-cli": "Run Tavily CLI workflows through a configured Tavily Hikari deployment.",
    "tavily-hikari-crawl": "Crawl bounded sites through Tavily Hikari with tvly-hikari.",
    "tavily-hikari-extract": "Extract URL content through Tavily Hikari with tvly-hikari.",
    "tavily-hikari-map": "Discover site URLs through Tavily Hikari with tvly-hikari.",
    "tavily-hikari-research": "Run multi-step Tavily research through Tavily Hikari with tvly-hikari.",
    "tavily-hikari-search": "Search the web through Tavily Hikari with tvly-hikari.",
}
GLOBAL_TARGETS = {
    # skills@1.5.16 treats Codex and OpenCode as universal clients. Its documented
    # global target is the user-level shared .agents directory, not a project path.
    "codex": Path(".agents/skills"),
    "opencode": Path(".agents/skills"),
    "claude-code": Path(".claude/skills"),
}


def parse_frontmatter(path: Path) -> dict[str, str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0] != "---":
        raise AssertionError(f"{path} is missing opening YAML frontmatter delimiter")
    try:
        end = lines.index("---", 1)
    except ValueError as exc:
        raise AssertionError(f"{path} is missing closing YAML frontmatter delimiter") from exc

    metadata = {}
    for line in lines[1:end]:
        key, separator, value = line.partition(":")
        if not separator:
            raise AssertionError(f"{path} has malformed YAML frontmatter line: {line}")
        metadata[key.strip()] = value.strip().strip('"')
    return metadata


class TavilyHikariAgentSkillsTest(unittest.TestCase):
    def test_all_skills_have_the_expected_valid_frontmatter(self):
        actual_names = {path.parent.name for path in SKILLS_DIR.glob("tavily-hikari-*/SKILL.md")}
        self.assertEqual(actual_names, set(SKILLS))

        for name, description in SKILLS.items():
            metadata = parse_frontmatter(SKILLS_DIR / name / "SKILL.md")
            self.assertEqual(metadata.get("name"), name)
            self.assertEqual(metadata.get("description"), description)

    def test_public_guidance_uses_only_the_global_install_command(self):
        public_paths = [
            ROOT / "README.md",
            ROOT / "skills" / "README.md",
            ROOT / "docs-site" / "docs" / "en" / "quick-start.md",
            ROOT / "docs-site" / "docs" / "en" / "faq.md",
            ROOT / "docs-site" / "docs" / "en" / "http-api-guide.md",
            ROOT / "docs-site" / "docs" / "zh" / "quick-start.md",
            ROOT / "docs-site" / "docs" / "zh" / "faq.md",
            ROOT / "docs-site" / "docs" / "zh" / "http-api-guide.md",
            ROOT / "web" / "src" / "user-console" / "guide.tsx",
            ROOT / "web" / "src" / "PublicHome.tsx",
        ]
        public_pattern = re.compile(rf"npx skills add {re.escape(REPO_URL)}(?=\s|$)")

        for path in public_paths:
            text = path.read_text(encoding="utf-8")
            self.assertIn(GLOBAL_COMMAND, text, path)
            for match in public_pattern.finditer(text):
                self.assertTrue(text[match.end() :].lstrip().startswith("--global"), path)

        installer = (ROOT / "scripts" / "install-tvly-hikari.sh").read_text(encoding="utf-8")
        self.assertIn('npx skills add "${REPO_URL}" --global', installer)

    @unittest.skipUnless(
        os.environ.get("RUN_NPX_SKILLS_INTEGRATION") == "1",
        "set RUN_NPX_SKILLS_INTEGRATION=1 to run npx skills target mapping checks",
    )
    def test_npx_skills_global_install_targets_each_supported_client(self):
        if shutil.which("npx") is None:
            self.skipTest("npx is not installed")

        for agent, relative_target in GLOBAL_TARGETS.items():
            with self.subTest(agent=agent), tempfile.TemporaryDirectory() as tmp:
                temp_root = Path(tmp)
                home = temp_root / "home"
                config_home = temp_root / "xdg-config"
                env = os.environ.copy()
                env["HOME"] = str(home)
                env["XDG_CONFIG_HOME"] = str(config_home)
                env["npm_config_cache"] = str(temp_root / "npm-cache")

                result = subprocess.run(
                    [
                        "npx",
                        "skills",
                        "add",
                        str(SKILLS_DIR),
                        "--global",
                        "--agent",
                        agent,
                        "--yes",
                    ],
                    cwd=ROOT,
                    env=env,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                    timeout=180,
                )
                if result.returncode != 0:
                    self.fail(
                        f"npx skills failed for {agent}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
                    )

                target_dir = home / relative_target
                self.assertEqual(
                    {path.name for path in target_dir.glob("tavily-hikari-*") if path.is_dir()},
                    set(SKILLS),
                )
                for name in SKILLS:
                    metadata = parse_frontmatter(target_dir / name / "SKILL.md")
                    self.assertEqual(metadata.get("name"), name)
                    self.assertTrue(metadata.get("description"))

                listed = subprocess.run(
                    ["npx", "skills", "list", "--global", "--agent", agent, "--json"],
                    cwd=ROOT,
                    env=env,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                    timeout=180,
                )
                if listed.returncode != 0:
                    self.fail(
                        f"npx skills list failed for {agent}\nstdout:\n{listed.stdout}\nstderr:\n{listed.stderr}"
                    )
                installed_names = {entry["name"] for entry in json.loads(listed.stdout)}
                self.assertTrue(set(SKILLS).issubset(installed_names))


if __name__ == "__main__":
    unittest.main()
