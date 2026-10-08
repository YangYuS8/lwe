import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]


class TauriCliTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        (self.repo / "scripts").mkdir(parents=True)
        self.script = self.repo / "scripts" / "ensure-tauri-cli.sh"
        shutil.copy2(REPO_ROOT / "scripts" / "ensure-tauri-cli.sh", self.script)
        self.version = (REPO_ROOT / ".tauri-cli-version").read_text().strip()
        (self.repo / ".tauri-cli-version").write_text(f"{self.version}\n")
        self.install_root = self.root / "install root"
        self.fake_bin = self.root / "fake-bin"
        self.fake_bin.mkdir()
        self.log = self.root / "cargo.jsonl"
        fake_cargo = self.fake_bin / "cargo"
        fake_cargo.write_text(
            f"#!{sys.executable}\n"
            "import json, os, pathlib, shutil, subprocess, sys\n"
            "args = sys.argv[1:]\n"
            "with open(os.environ['FAKE_CARGO_LOG'], 'a') as log:\n"
            "    log.write(json.dumps(args) + '\\n')\n"
            "if args == ['tauri', '--version']:\n"
            "    cli = shutil.which('cargo-tauri')\n"
            "    sys.exit(subprocess.run([cli, '--version']).returncode if cli else 1)\n"
            "if args[:2] == ['install', 'tauri-cli']:\n"
            "    if os.environ.get('FAKE_INSTALL_FAIL'):\n"
            "        sys.exit(1)\n"
            "    root = pathlib.Path(args[args.index('--root') + 1])\n"
            "    cli = root / 'bin' / 'cargo-tauri'\n"
            "    cli.parent.mkdir(parents=True, exist_ok=True)\n"
            "    version = os.environ.get('FAKE_INSTALL_VERSION', args[args.index('--version') + 1].lstrip('='))\n"
            "    cli.write_text('#!/bin/sh\\necho tauri-cli ' + version + '\\n')\n"
            "    cli.chmod(0o755)\n"
            "    sys.exit(0)\n"
            "sys.exit(2)\n"
        )
        fake_cargo.chmod(0o755)
        self.env = os.environ.copy()
        self.env.update(
            PATH=f"{self.fake_bin}{os.pathsep}{os.environ['PATH']}",
            CARGO_INSTALL_ROOT=str(self.install_root),
            FAKE_CARGO_LOG=str(self.log),
        )

    def run_script(self, *args, **overrides):
        return subprocess.run(
            ["bash", str(self.script), *args],
            env={**self.env, **overrides},
            text=True,
            capture_output=True,
            cwd=self.root,
        )

    def cached_cli(self, version, executable=True):
        cli = self.install_root / "bin" / "cargo-tauri"
        cli.parent.mkdir(parents=True, exist_ok=True)
        cli.write_text(f"#!/bin/sh\nprintf 'tauri-cli {version}\\n'\n")
        cli.chmod(0o755 if executable else 0o644)
        return cli

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def installs(self):
        return [call for call in self.calls() if call[:2] == ["install", "tauri-cli"]]

    def test_cold_cache_installs_exact_locked_version(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            self.installs(),
            [["install", "tauri-cli", "--version", f"={self.version}", "--locked", "--force", "--root", str(self.install_root)]],
        )
        self.assertIn(f"Tauri CLI verified: tauri-cli {self.version}", result.stdout)

    def test_hot_cache_does_not_install(self):
        self.cached_cli(self.version)
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.installs(), [])
        self.assertEqual(self.calls(), [["tauri", "--version"], ["tauri", "--version"]])

    def test_wrong_cached_version_is_replaced(self):
        self.cached_cli("2.10.1")
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.installs()), 1)
        self.assertIn(f"Tauri CLI verified: tauri-cli {self.version}", result.stdout)

    def test_non_executable_cache_is_replaced(self):
        self.cached_cli(self.version, executable=False)
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.installs()), 1)

    def test_wrong_installed_version_is_rejected(self):
        result = self.run_script(FAKE_INSTALL_VERSION="2.11.4")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Tauri CLI version mismatch", result.stderr)
        self.assertNotIn("Tauri CLI verified", result.stdout)

    def test_install_failure_is_not_reported_as_success(self):
        result = self.run_script(FAKE_INSTALL_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("Tauri CLI verified", result.stdout)

    def test_version_query_does_not_install(self):
        result = self.run_script("--version")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, f"{self.version}\n")
        self.assertEqual(self.calls(), [])
        self.assertFalse(self.install_root.exists())

    def test_invalid_version_is_rejected_before_cargo(self):
        (self.repo / ".tauri-cli-version").write_text("2.12.1\n3.0.0\n")
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Invalid version", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_global_cli_is_not_overwritten(self):
        global_cli = self.fake_bin / "cargo-tauri"
        global_cli.write_text("#!/bin/sh\nprintf 'tauri-cli 2.10.1\\n'\n")
        global_cli.chmod(0o755)
        original = global_cli.read_bytes()
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(global_cli.read_bytes(), original)
        self.assertEqual(len(self.installs()), 1)


if __name__ == "__main__":
    unittest.main()
