"""CNB synchronization boundaries; no network, credentials, or package execution."""

import contextlib
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("sync_cnb", SCRIPTS / "sync-cnb.py")
sync = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sync)

OLD, CURRENT, LATEST = (character * 40 for character in "123")
TAG_OBJECT = "a" * 40
TAG = "v0.9.11"


def completed(stdout="", returncode=0):
    return subprocess.CompletedProcess([], returncode, stdout, "")


def release_fixture(tag=TAG, prerelease=False):
    names = ["lwe_0.9.11_amd64.deb", "lwe-0.9.11-1.x86_64.rpm", "lwe_0.9.11_amd64.AppImage"]
    payloads = {name: ("fixture " + name).encode() for name in names}
    release = {
        "tag_name": tag, "draft": False, "prerelease": prerelease,
        "target_commitish": CURRENT,
        "assets": [{
            "name": name, "state": "uploaded", "size": len(content),
            "digest": "sha256:" + hashlib.sha256(content).hexdigest(),
        } for name, content in payloads.items()],
    }
    return release, payloads


def release_git(*args, **kwargs):
    if args == ("rev-parse", "FETCH_HEAD"):
        return completed(TAG_OBJECT)
    if args == ("rev-parse", "FETCH_HEAD^{commit}"):
        return completed(CURRENT)
    if args[0] in ("check-ref-format", "fetch"):
        return completed()
    raise AssertionError(f"Unexpected git operation: {args}")


class MainRemote:
    """A remote advances during fetch/push, reproducing late-run and push races."""

    def __init__(self, tips, remote, push_failures=0, advance_on_failure=None):
        self.tips = iter(tips)
        self.latest = tips[-1]
        self.remote = remote
        self.fetched = None
        self.push_failures = push_failures
        self.advance_on_failure = advance_on_failure
        self.pushes = []

    def git(self, *args, **kwargs):
        if args[0] == "fetch":
            self.fetched = next(self.tips, self.latest) if args[2] == sync.GITHUB_URL else self.remote
            return completed()
        if args == ("rev-parse", "FETCH_HEAD^{commit}"):
            return completed(self.fetched)
        if args[:2] == ("merge-base", "--is-ancestor"):
            ancestors = {(OLD, CURRENT), (OLD, LATEST), (CURRENT, LATEST)}
            return completed(returncode=0 if args[2] == args[3] or args[2:] in ancestors else 1)
        if args[0] == "push":
            self.pushes.append(args)
            if self.push_failures:
                self.push_failures -= 1
                if self.advance_on_failure:
                    self.remote = self.advance_on_failure
                return completed(returncode=1)
            self.remote = args[2].split(":", 1)[0]
            return completed()
        raise AssertionError(f"Unexpected git operation: {args}")


class ReleaseSelectionTests(unittest.TestCase):
    def test_beta_accepts_base_version_filenames_and_never_selects_latest(self):
        tag = "v0.9.11-beta.182+abcdef0"
        release, _ = release_fixture(tag, prerelease=True)
        with mock.patch.object(sync, "github", return_value=release) as github, \
                mock.patch.object(sync, "git", side_effect=release_git):
            selected, tag_object, source = sync.load_release(tag, CURRENT)

        self.assertEqual((tag_object, source), (TAG_OBJECT, CURRENT))
        self.assertFalse(selected["sync_make_latest"])
        github.assert_called_once_with("releases/tags/v0.9.11-beta.182%2Babcdef0")

    def test_wrong_release_or_build_commit_is_rejected(self):
        for metadata_sha, expected_sha in [(OLD, CURRENT), (CURRENT, LATEST)]:
            with self.subTest(metadata_sha=metadata_sha, expected_sha=expected_sha):
                release, _ = release_fixture()
                release["target_commitish"] = metadata_sha
                with mock.patch.object(sync, "github", return_value=release), \
                        mock.patch.object(sync, "git", side_effect=release_git), \
                        self.assertRaisesRegex(sync.SyncError, "disagree"):
                    sync.load_release(TAG, expected_sha)

    def test_draft_missing_or_unfinished_assets_are_rejected(self):
        for problem in ("draft", "missing", "unfinished", "unsafe_name", "missing_digest"):
            with self.subTest(problem=problem):
                release, _ = release_fixture()
                if problem == "draft":
                    release["draft"] = True
                elif problem == "missing":
                    release["assets"].pop()
                elif problem == "unfinished":
                    release["assets"][0]["state"] = "uploading"
                elif problem == "unsafe_name":
                    release["assets"][0]["name"] = "../lwe_0.9.11_amd64.deb"
                else:
                    release["assets"][0].pop("digest")
                with mock.patch.object(sync, "github", return_value=release), \
                        mock.patch.object(sync, "git", side_effect=release_git), \
                        self.assertRaises(sync.SyncError):
                    sync.load_release(TAG, CURRENT)

    def test_conflicting_tag_object_never_pushes(self):
        with mock.patch.object(sync, "remote_ref", return_value="b" * 40), \
                mock.patch.object(sync, "git") as git, \
                self.assertRaisesRegex(sync.SyncError, "refusing to replace"):
            sync.mirror_tag(TAG, TAG_OBJECT, {})
        git.assert_not_called()

    def test_same_tag_object_is_idempotent(self):
        with mock.patch.object(sync, "remote_ref", return_value=TAG_OBJECT), \
                mock.patch.object(sync, "git") as git:
            sync.mirror_tag(TAG, TAG_OBJECT, {})
        git.assert_not_called()


class MainSynchronizationTests(unittest.TestCase):
    def mirror(self, remote):
        with mock.patch.object(sync, "git", side_effect=remote.git), \
                mock.patch.object(sync, "remote_ref", side_effect=lambda *args: remote.remote), \
                mock.patch.object(sync, "docs_changed", return_value=False), \
                mock.patch.object(sync.time, "sleep"):
            return sync.mirror_main({})

    def test_old_main_snapshot_refreshes_without_rewinding_remote(self):
        remote = MainRemote([CURRENT, LATEST], LATEST)
        self.assertEqual(self.mirror(remote), (LATEST, False))
        self.assertEqual(remote.remote, LATEST)
        self.assertEqual(remote.pushes, [])

    def test_failed_fast_forward_retries_new_tip_without_force(self):
        remote = MainRemote([CURRENT, LATEST], OLD, push_failures=1, advance_on_failure=LATEST)
        self.assertEqual(self.mirror(remote), (LATEST, False))
        self.assertEqual(remote.remote, LATEST)
        self.assertEqual(remote.pushes, [("push", sync.CNB_URL, f"{CURRENT}:refs/heads/main")])

    def test_repeated_push_failure_stops_without_force(self):
        remote = MainRemote([CURRENT], OLD, push_failures=3)
        with self.assertRaisesRegex(sync.SyncError, "safe retries"):
            self.mirror(remote)
        self.assertEqual(remote.remote, OLD)
        self.assertEqual(remote.pushes, [("push", sync.CNB_URL, f"{CURRENT}:refs/heads/main")] * 3)

    def test_divergence_is_rejected_without_any_push(self):
        remote = MainRemote([CURRENT], "d" * 40)
        with self.assertRaisesRegex(sync.SyncError, "refusing to force-push"):
            self.mirror(remote)
        self.assertEqual(remote.pushes, [])


class InputAndDownloadTests(unittest.TestCase):
    def test_docs_range_requires_full_commit_shas_before_git(self):
        for before, after in [("--cached", CURRENT), (OLD, CURRENT + "\n"), (OLD, ""), ("", CURRENT)]:
            with self.subTest(before=before, after=after), \
                    mock.patch.object(sync, "git") as git, \
                    self.assertRaisesRegex(sync.SyncError, "complete commit SHAs"):
                sync.docs_changed(before, after)
            git.assert_not_called()
        with mock.patch.object(sync, "git") as git:
            self.assertTrue(sync.docs_changed("0" * 40, CURRENT))
        git.assert_not_called()

    def test_invalid_cli_range_is_rejected_before_remote_side_effects(self):
        ranges = [
            ["--docs-changed-from", OLD],
            ["--docs-changed-from", "invalid", "--docs-changed-to", CURRENT],
        ]
        for arguments in ranges:
            with self.subTest(arguments=arguments), \
                    mock.patch.object(sys, "argv", ["sync-cnb.py", *arguments]), \
                    mock.patch.dict(os.environ, {"CNB_TOKEN": "fixture-only-token"}, clear=True), \
                    mock.patch.object(sync, "CnbClient") as client, \
                    mock.patch.object(sync, "mirror_main", return_value=(CURRENT, True)) as mirror, \
                    mock.patch.object(sync, "deploy_docs") as deploy, \
                    self.assertRaises(sync.SyncError):
                sync.main()
            client.assert_not_called()
            mirror.assert_not_called()
            deploy.assert_not_called()

    def test_expected_sha_requires_a_full_sha_and_a_selected_release(self):
        for arguments in [["--expected-source-sha", CURRENT], ["--release-tag", TAG, "--expected-source-sha", "abc"]]:
            with self.subTest(arguments=arguments), \
                    mock.patch.object(sys, "argv", ["sync-cnb.py", *arguments]), \
                    mock.patch.object(sync, "CnbClient") as client, \
                    self.assertRaises(sync.SyncError):
                sync.main()
            client.assert_not_called()

    def test_wrong_download_digest_fails_before_any_mirroring_or_upload(self):
        release, payloads = release_fixture()
        release["assets"][0]["digest"] = "sha256:" + "0" * 64

        def download(command, **kwargs):
            self.assertEqual(command[:3], ["gh", "release", "download"])
            directory = Path(command[command.index("--dir") + 1])
            for name, content in payloads.items():
                (directory / name).write_bytes(content)
            return completed()

        with mock.patch.object(sys, "argv", ["sync-cnb.py", "--release-tag", TAG]), \
                mock.patch.dict(os.environ, {"CNB_TOKEN": "fixture-only-token"}, clear=True), \
                mock.patch.object(sync, "CnbClient") as client, \
                mock.patch.object(sync, "load_release", return_value=(release, TAG_OBJECT, CURRENT)), \
                mock.patch.object(sync.subprocess, "run", side_effect=download) as run, \
                mock.patch.object(sync, "mirror_main") as mirror_main, \
                mock.patch.object(sync, "mirror_tag") as mirror_tag, \
                self.assertRaisesRegex(sync.SyncError, "checksum differs"):
            client.return_value.request.return_value = {"auto_trigger": False}
            sync.main()

        run.assert_called_once()
        mirror_main.assert_not_called()
        mirror_tag.assert_not_called()
        client.return_value.publish_release.assert_not_called()

    def test_valid_download_runs_the_static_appimage_gate(self):
        release, payloads = release_fixture()
        calls = []
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def run(command, **kwargs):
                calls.append(command)
                if command[:3] == ["gh", "release", "download"]:
                    for name, content in payloads.items():
                        (root / name).write_bytes(content)
                else:
                    self.assertEqual(command, [sys.executable, str(SCRIPTS / "validate-appimage.py"), str(root / "lwe_0.9.11_amd64.AppImage")])
                    self.assertTrue(kwargs["check"])
                return completed()

            with mock.patch.object(sync.subprocess, "run", side_effect=run):
                artifacts = sync.download_release(release, root)
            self.assertEqual(set(artifacts), set(payloads))
        self.assertEqual(len(calls), 2)

    def test_docs_error_stops_polling_immediately(self):
        client = mock.Mock()
        client.request.side_effect = [{"success": True, "sn": "fixture-build"}, {"status": "error"}]
        with mock.patch.object(sync.time, "sleep") as sleep, \
                contextlib.redirect_stdout(io.StringIO()), \
                self.assertRaisesRegex(sync.SyncError, "deployment failed"):
            sync.deploy_docs(client, CURRENT)
        sleep.assert_not_called()
        self.assertEqual(client.request.call_count, 2)


class WorkflowAndCredentialTests(unittest.TestCase):
    def test_sync_has_one_serial_queue_and_does_not_cancel_publications(self):
        text = (SCRIPTS.parent / '.github/workflows/sync-cnb.yml').read_text()
        self.assertIn('\nconcurrency:\n  group: cnb-sync\n  cancel-in-progress: false\n  queue: max\n', text)
        self.assertEqual(len(re.findall(r'^\s*concurrency:', text, re.M)), 1)
        self.assertEqual(len(re.findall(r'^\s*queue:', text, re.M)), 1)

    def test_git_askpass_never_sends_cnb_token_to_other_hosts(self):
        with tempfile.TemporaryDirectory() as temporary:
            helper = Path(temporary) / 'askpass.sh'
            helper.write_text(sync.ASKPASS_SCRIPT)
            env = {**os.environ, 'CNB_TOKEN': 'fixture-token'}
            for prompt, output in [
                ("Username for 'https://cnb.cool':", 'cnb\n'),
                ("Password for 'https://cnb@cnb.cool':", 'fixture-token\n'),
            ]:
                result = subprocess.run(['sh', str(helper), prompt], env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout, output)
            for host in ('github.com', 'cnb.cool.attacker.invalid'):
                result = subprocess.run(['sh', str(helper), f"Password for 'https://{host}':"],
                                        env=env, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, '')


if __name__ == "__main__":
    unittest.main()
