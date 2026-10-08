"""Metadata fixtures and static AppImage parsing regressions (no GUI execution)."""

import base64
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "validate-appimage.py"
SPEC = importlib.util.spec_from_file_location("validate_appimage", SCRIPT)
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)
PNG = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jS7YAAAAASUVORK5CYII=")


class AppDirTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="lwe-appdir-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "AppRun").write_text("#!/bin/sh\nexit 0\n")
        (self.root / "AppRun").chmod(0o755)
        (self.root / "lwe.desktop").write_text("[Desktop Entry]\nType=Application\nName=LWE\nExec=lwe %U\nIcon=lwe\nCategories=Utility;\n")
        (self.root / "lwe.png").write_bytes(PNG)
        (self.root / ".DirIcon").write_bytes(PNG)

    def link(self, name, target):
        path = self.root / name
        path.unlink(missing_ok=True)
        path.symlink_to(target)

    def test_plain_png_and_executable_pass(self):
        validator.validate_appdir(self.root)

    def test_relative_desktop_apprun_and_multihop_icon_pass(self):
        nested = self.root / "usr" / "share"
        nested.mkdir(parents=True)
        (self.root / "lwe.desktop").rename(nested / "lwe.desktop")
        (self.root / "AppRun").rename(nested / "lwe")
        self.link("lwe.desktop", "usr/share/lwe.desktop")
        self.link("AppRun", "usr/share/lwe")
        self.link("middle.png", "usr/share/../../lwe.png")
        self.link(".DirIcon", "middle.png")
        validator.validate_appdir(self.root)

    def test_svg_root_icon_and_explicit_icon_extension_pass(self):
        (self.root / "lwe.png").unlink()
        (self.root / "lwe.svg").write_text('<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"></svg>')
        desktop = self.root / "lwe.desktop"
        desktop.write_text(desktop.read_text().replace("Icon=lwe", "Icon=lwe.svg"))
        validator.validate_appdir(self.root)

    def test_diricon_bad_links_fail(self):
        cases = (
            ("/home/runner/work/lwe/lwe/target/release/bundle/appimage/lwe.AppDir/lwe.png", "absolute symlink"),
            (str(self.root / "lwe.png"), "absolute symlink"),
            ("missing.png", "missing or broken"),
            ("../outside.png", "escapes the AppDir"),
            (".DirIcon", "symlink loop"),
        )
        for target, message in cases:
            with self.subTest(target=target):
                self.link(".DirIcon", target)
                with self.assertRaisesRegex(validator.ValidationError, message):
                    validator.validate_appdir(self.root)

    def test_intermediate_absolute_links_fail(self):
        self.link("middle.png", str(self.root / "lwe.png"))
        self.link(".DirIcon", "middle.png")
        with self.assertRaisesRegex(validator.ValidationError, r"\.DirIcon: absolute symlink at middle.png"):
            validator.validate_appdir(self.root)

    def test_relative_parent_directory_escape_fails(self):
        self.link("outside", "..")
        self.link(".DirIcon", "outside/icon.png")
        with self.assertRaisesRegex(validator.ValidationError, "escapes the AppDir"):
            validator.validate_appdir(self.root)

    def test_missing_and_non_png_diricon_fail(self):
        (self.root / ".DirIcon").unlink()
        with self.assertRaisesRegex(validator.ValidationError, r"\.DirIcon: missing"):
            validator.validate_appdir(self.root)
        (self.root / ".DirIcon").write_text("not a PNG")
        with self.assertRaisesRegex(validator.ValidationError, "expected image/png"):
            validator.validate_appdir(self.root)

    def test_multiple_desktops_including_hidden_broken_link_fail(self):
        self.link(".desktop", "missing.desktop")
        with self.assertRaisesRegex(validator.ValidationError, "found 2"):
            validator.validate_appdir(self.root)

    def test_broken_and_absolute_desktop_fail(self):
        for target in ("missing.desktop", "/usr/share/applications/lwe.desktop"):
            with self.subTest(target=target):
                self.link("lwe.desktop", target)
                with self.assertRaises(validator.ValidationError):
                    validator.validate_appdir(self.root)

    def test_invalid_desktop_and_mismatched_icon_fail(self):
        desktop = self.root / "lwe.desktop"
        original = desktop.read_text()
        desktop.write_text(original.replace("Type=Application", "Type=Invalid"))
        with self.assertRaisesRegex(validator.ValidationError, "desktop-file-validate failed"):
            validator.validate_appdir(self.root)
        desktop.write_text(original.replace("Icon=lwe", "Icon=missing"))
        with self.assertRaisesRegex(validator.ValidationError, "no matching root PNG/SVG"):
            validator.validate_appdir(self.root)

    def test_root_icon_cannot_escape_or_disguise_another_format(self):
        self.link("lwe.png", "../outside.png")
        with self.assertRaisesRegex(validator.ValidationError, "lwe.png: link target escapes"):
            validator.validate_appdir(self.root)
        (self.root / "lwe.png").unlink()
        (self.root / "lwe.png").write_text('<svg xmlns="http://www.w3.org/2000/svg"></svg>')
        with self.assertRaisesRegex(validator.ValidationError, "lwe.png: expected image/png"):
            validator.validate_appdir(self.root)

    def test_non_executable_and_broken_apprun_fail(self):
        (self.root / "AppRun").chmod(0o644)
        with self.assertRaisesRegex(validator.ValidationError, "not executable"):
            validator.validate_appdir(self.root)
        self.link("AppRun", "missing")
        with self.assertRaisesRegex(validator.ValidationError, "AppRun: missing or broken"):
            validator.validate_appdir(self.root)

    @unittest.skipIf(os.geteuid() == 0, "root may execute files with any execute bit")
    def test_apprun_must_be_executable_by_the_current_user(self):
        for mode in (0o641, 0o650):
            with self.subTest(mode=oct(mode)):
                (self.root / "AppRun").chmod(mode)
                with self.assertRaisesRegex(validator.ValidationError, "not executable"):
                    validator.validate_appdir(self.root)

    def test_cli_reports_all_inputs_and_fails_if_any_fails(self):
        with contextlib.redirect_stdout(io.StringIO()) as output, contextlib.redirect_stderr(io.StringIO()) as errors:
            result = validator.main(["--appdir", str(self.root), "--appdir", str(self.root / "missing")])
        self.assertEqual(result, 1)
        self.assertIn("PASS:", output.getvalue())
        self.assertIn("FAIL:", errors.getvalue())

    def test_cli_reports_missing_dependency(self):
        with mock.patch.object(validator.shutil, "which", return_value=None), contextlib.redirect_stderr(io.StringIO()) as errors:
            self.assertEqual(validator.main(["--appdir", str(self.root)]), 1)
        self.assertIn("missing required executable", errors.getvalue())


class AppImageParsingTests(unittest.TestCase):
    def test_squashfs_magic_inside_an_elf_segment_is_not_the_payload(self):
        header = b"\x7fELF\x02\x01\x01\x00AI\x02" + b"\x00" * 5
        header += struct.pack("<HHIQQQIHHHHHH", 3, 62, 1, 0, 64, 0, 0, 64, 56, 1, 0, 0, 0)
        segment = struct.pack("<IIQQQQQQ", 1, 5, 0, 0, 0, 256, 256, 4096)
        superblock = struct.pack("<5I6H8Q", 0x73717368, 1, 0, 4096, 0, 1, 12, 0, 1, 4, 0, 0, 128, 112, 0xFFFFFFFFFFFFFFFF, 96, 104, 0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF)
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "fixture.AppImage"
            image.write_bytes(header + segment + b"\x00" * 8 + superblock + b"\x00" * 32 + superblock + b"\x00" * 32)
            self.assertEqual(validator.squashfs_offset(image), 256)

    def test_static_offset_skips_invalid_magic_and_never_executes_image(self):
        header = b"\x7fELF\x02\x01\x01\x00AI\x02" + b"\x00" * 5
        header += struct.pack("<HHIQQQIHHHHHH", 3, 62, 1, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0)
        invalid = b"hsqs" + b"\x00" * 92
        superblock = struct.pack("<5I6H8Q", 0x73717368, 1, 0, 4096, 0, 1, 12, 0, 1, 4, 0, 0, 128, 112, 0xFFFFFFFFFFFFFFFF, 96, 104, 0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF)
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "fixture.AppImage"
            image.write_bytes(header + invalid + superblock + b"\x00" * 32)
            with mock.patch.object(validator.subprocess, "run", side_effect=AssertionError("must not execute")):
                self.assertEqual(validator.squashfs_offset(image), 160)
            image.write_bytes(header + invalid)
            with self.assertRaisesRegex(validator.ValidationError, "no valid v4 SquashFS"):
                validator.squashfs_offset(image)

    def test_extraction_failure_is_reported_without_running_the_image(self):
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "fixture.AppImage"
            image.write_bytes(b"fixture")
            failure = validator.subprocess.CompletedProcess([], 1, "", "invalid squashfs")
            with mock.patch.object(validator, "squashfs_offset", return_value=944632), mock.patch.object(validator.subprocess, "run", return_value=failure) as run:
                with self.assertRaisesRegex(validator.ValidationError, "unsquashfs failed .*invalid squashfs"):
                    validator.validate_appimage(image)
            self.assertEqual(run.call_args.args[0][0], "unsquashfs")
            self.assertIn(str(image.resolve()), run.call_args.args[0])

    def test_truncated_or_non_appimage_elf_is_rejected(self):
        for data in (b"", b"not an AppImage" * 10):
            with self.subTest(data=data):
                with tempfile.TemporaryDirectory() as directory:
                    image = Path(directory) / "fixture.AppImage"
                    image.write_bytes(data)
                    with self.assertRaises(validator.ValidationError):
                        validator.squashfs_offset(image)


if __name__ == "__main__":
    unittest.main()
