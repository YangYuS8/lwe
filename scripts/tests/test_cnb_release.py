import copy
import hashlib
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch
import urllib.error


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cnb_release import API_ORIGIN, CnbClient, CnbError, PROVENANCE, _NoRedirect


SOURCE_SHA = "b" * 40


class Response(io.BytesIO):
    def __init__(self, content=b"", status=200, headers=None):
        super().__init__(content)
        self.status = status
        self.headers = headers or {}

    def getcode(self):
        return self.status


class FakeCnb(CnbClient):
    def __init__(self, release):
        super().__init__("fixture-only-token", "Nesoriel/lwe")
        self.github = release
        self.remote = None
        self.files = {}
        self.calls = []
        self.pending = {}
        self.source_sha = SOURCE_SHA
        self.fail_upload = None
        self.fail_confirmation = False
        self.wrong_content = None
        self.server_hash = False
        self.hash_override = None
        self.downloads = []
        self.publication_patch = False
        self.stale_final_readbacks = 0
        self.invalid_final_readback = False

    def request(self, method, path, data=None, allow_not_found=False):
        self.calls.append((method, path, copy.deepcopy(data)))
        if path.startswith("/git/tags/"):
            return {"commit": {"sha": self.source_sha}}
        if method == "GET":
            if self.publication_patch and path.startswith("/releases/tags/"):
                if self.invalid_final_readback:
                    return {"tag_name": self.github["tag_name"]}
                if self.stale_final_readbacks:
                    self.stale_final_readbacks -= 1
                    return {**copy.deepcopy(self.remote), "draft": True}
            return copy.deepcopy(self.remote)
        if method == "POST" and path == "/releases":
            self.remote = {**data, "id": "release-1", "assets": [], "is_latest": False}
            return copy.deepcopy(self.remote)
        if method == "PATCH":
            self.remote.update(data)
            self.remote["is_latest"] = data["make_latest"] == "true"
            self.publication_patch = data["draft"] is False
            return None
        if path.endswith("/asset-upload-url"):
            name = data["asset_name"]
            self.pending[name] = data
            return {
                "upload_url": "https://asset.cnb.cool/upload?signature=private-value",
                "verify_url": API_ORIGIN + self.base_path + "/releases/release-1/asset-upload-confirmation/private-value/" + name,
                "expires_in_sec": 600,
            }
        if "/asset-upload-confirmation/" in path:
            if self.fail_confirmation:
                raise CnbError("fixture confirmation failure")
            name = path.split("?")[0].rsplit("/", 1)[1]
            asset = {"name": name, "size": len(self.files[name])}
            if self.server_hash:
                asset.update(hash_algo="sha256", hash_value=self.hash_override or hashlib.sha256(self.files[name]).hexdigest())
            self.remote["assets"].append(asset)
            return None
        raise AssertionError((method, path, data))

    def _put_file(self, url, path, size):
        name = Path(path).name
        if name == self.fail_upload:
            raise CnbError("fixture upload failure")
        self.files[name] = Path(path).read_bytes()

    def download_asset(self, tag, name, destination):
        self.downloads.append(name)
        data = self.wrong_content if name == next(iter(self.files), None) and self.wrong_content is not None else self.files[name]
        Path(destination).write_bytes(data)
        return hashlib.sha256(data).hexdigest()


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.files = {}
        assets = []
        for suffix in ("AppImage", "deb", "rpm"):
            name = "lwe_0.9.11." + suffix
            path = self.root / name
            content = ("original-" + suffix).encode()
            path.write_bytes(content)
            self.files[name] = path
            assets.append({"name": name, "size": len(content), "digest": "sha256:" + hashlib.sha256(content).hexdigest()})
        self.release = {
            "tag_name": "v0.9.11", "name": "LWE v0.9.11", "body": "Original release notes.",
            "draft": False, "prerelease": False, "sync_make_latest": True, "assets": assets,
        }
        self.client = FakeCnb(self.release)

    def publish(self):
        return self.client.publish_release(self.release, self.files, SOURCE_SHA)

    def test_all_files_verified_then_release_published(self):
        result = self.publish()
        self.assertFalse(result["draft"])
        self.assertTrue(result["is_latest"])
        self.assertEqual(result["name"], self.release["name"])
        self.assertEqual(result["body"], self.release["body"] + "\n\n" + PROVENANCE.format(SOURCE_SHA))
        uploads = [data for _, path, data in self.client.calls if path.endswith("/asset-upload-url")]
        self.assertEqual(len(uploads), 3)
        self.assertTrue(all(data["ttl"] == 0 and data["overwrite"] is False for data in uploads))
        confirmations = [path for method, path, _ in self.client.calls if method == "POST" and "/asset-upload-confirmation/" in path]
        self.assertEqual(len(confirmations), 3)
        self.assertTrue(all(path.endswith("?ttl=0") for path in confirmations))

    def test_local_same_size_wrong_hash_causes_no_api_calls(self):
        path = next(iter(self.files.values()))
        path.write_bytes(b"x" * path.stat().st_size)
        with self.assertRaisesRegex(CnbError, "Local artifact"):
            self.publish()
        self.assertEqual(self.client.calls, [])

    def test_local_wrong_size_causes_no_api_calls(self):
        next(iter(self.files.values())).write_bytes(b"truncated")
        with self.assertRaisesRegex(CnbError, "Local artifact"):
            self.publish()
        self.assertEqual(self.client.calls, [])

    def test_missing_linux_package_rejected_without_api(self):
        del self.files[next(iter(self.files))]
        with self.assertRaisesRegex(CnbError, "Expected one"):
            self.publish()
        self.assertEqual(self.client.calls, [])

    def test_tag_commit_conflict_stops_before_write(self):
        self.client.source_sha = "a" * 40
        with self.assertRaisesRegex(CnbError, "tag does not match"):
            self.publish()
        self.assertTrue(all(method == "GET" for method, _, _ in self.client.calls))

    def test_beta_never_promotes_latest(self):
        self.release.update(tag_name="v0.9.11-beta.190+b45efdd", prerelease=True)
        result = self.publish()
        self.assertTrue(result["prerelease"])
        self.assertFalse(result["is_latest"])
        self.assertIn("%2B", self.client.calls[0][1])

    def test_latest_string_cannot_accidentally_promote_an_old_release(self):
        self.release["sync_make_latest"] = "false"
        with self.assertRaisesRegex(CnbError, "must be a boolean"):
            self.publish()
        self.assertEqual(self.client.calls, [])

    def test_wrong_uploaded_content_keeps_release_draft(self):
        self.client.wrong_content = b"x" * next(iter(self.files.values())).stat().st_size
        with self.assertRaisesRegex(CnbError, "content conflicts"):
            self.publish()
        self.assertTrue(self.client.remote["draft"])
        self.assertFalse(any(method == "PATCH" and data.get("draft") is False for method, _, data in self.client.calls))

    def test_partial_failure_keeps_draft_and_retry_skips_completed_assets(self):
        names = list(self.files)
        self.client.fail_upload = names[1]
        with self.assertRaises(CnbError):
            self.publish()
        self.assertTrue(self.client.remote["draft"])
        self.assertEqual([a["name"] for a in self.client.remote["assets"]], [names[0]])
        self.client.calls.clear()
        self.client.fail_upload = None
        self.publish()
        uploads = [data["asset_name"] for _, path, data in self.client.calls if path.endswith("/asset-upload-url")]
        self.assertEqual(uploads, names[1:])

    def test_confirmation_failure_keeps_draft(self):
        self.client.fail_confirmation = True
        with self.assertRaises(CnbError):
            self.publish()
        self.assertTrue(self.client.remote["draft"])
        self.assertEqual(self.client.remote["assets"], [])

    def test_existing_complete_release_is_idempotent(self):
        self.publish()
        self.client.calls.clear()
        self.publish()
        self.assertTrue(all(method == "GET" for method, _, _ in self.client.calls))

    def test_matching_server_sha_skips_downloads_on_upload_and_retry(self):
        self.client.server_hash = True
        self.publish()
        self.assertEqual(self.client.downloads, [])
        self.client.calls.clear()
        self.publish()
        self.assertEqual(self.client.downloads, [])
        self.assertTrue(all(method == "GET" for method, _, _ in self.client.calls))

    @patch("cnb_release.time.sleep")
    def test_publication_waits_for_one_stale_tag_readback_without_repatch(self, sleep):
        self.client.stale_final_readbacks = 1
        result = self.publish()
        self.assertFalse(result["draft"])
        sleep.assert_called_once_with(2)
        patches = [call for call in self.client.calls if call[0] == "PATCH"]
        self.assertEqual(len(patches), 1)
        patch_index = self.client.calls.index(patches[0])
        self.assertEqual([path for method, path, _ in self.client.calls[patch_index + 1:]],
                         ["/releases/tags/v0.9.11"] * 2)

    @patch("cnb_release.time.sleep")
    def test_persistent_publication_mismatch_stops_after_twelve_reads(self, sleep):
        self.client.stale_final_readbacks = 12
        with self.assertRaisesRegex(CnbError, "after 12 attempts"):
            self.publish()
        self.assertEqual(sleep.call_count, 11)
        self.assertTrue(all(call.args == (2,) for call in sleep.call_args_list))
        patches = [call for call in self.client.calls if call[0] == "PATCH"]
        self.assertEqual(len(patches), 1)
        patch_index = self.client.calls.index(patches[0])
        self.assertEqual([path for method, path, _ in self.client.calls[patch_index + 1:]],
                         ["/releases/tags/v0.9.11"] * 12)

    @patch("cnb_release.time.sleep")
    def test_unknown_publication_readback_fails_without_retry(self, sleep):
        self.client.invalid_final_readback = True
        with self.assertRaisesRegex(CnbError, "Invalid CNB release publication"):
            self.publish()
        sleep.assert_not_called()

    def test_server_sha_conflict_stops_without_download_or_overwrite(self):
        self.client.server_hash = True
        self.publish()
        self.client.remote["assets"][0]["hash_value"] = "0" * 64
        self.client.calls.clear()
        with self.assertRaisesRegex(CnbError, "content conflicts"):
            self.publish()
        self.assertEqual(self.client.downloads, [])
        self.assertTrue(all(method == "GET" for method, _, _ in self.client.calls))

    def test_uploaded_server_sha_conflict_keeps_draft_without_download(self):
        self.client.server_hash = True
        self.client.hash_override = "0" * 64
        with self.assertRaisesRegex(CnbError, "content conflicts"):
            self.publish()
        self.assertTrue(self.client.remote["draft"])
        self.assertEqual(self.client.downloads, [])

    def test_missing_sha_uses_download_fallback(self):
        self.publish()
        self.assertEqual(self.client.downloads, list(self.files))

    def test_sha256_without_hash_value_uses_download_fallback(self):
        self.client.server_hash = True
        self.publish()
        for asset in self.client.remote["assets"]:
            asset.pop("hash_value")
        self.publish()
        self.assertEqual(self.client.downloads, list(self.files))

    def test_conflicting_existing_hash_stops_before_uploading_missing_files(self):
        self.client.server_hash = True
        self.publish()
        self.client.remote["assets"].pop(0)
        self.client.remote["assets"][-1]["hash_value"] = "0" * 64
        self.client.calls.clear()
        with self.assertRaisesRegex(CnbError, "content conflicts"):
            self.publish()
        self.assertTrue(all(method == "GET" for method, _, _ in self.client.calls))
        self.assertEqual(self.client.downloads, [])

    def test_legacy_non_sha256_uses_download_fallback(self):
        self.client.server_hash = True
        self.publish()
        for asset in self.client.remote["assets"]:
            asset.update(hash_algo="md5", hash_value="a" * 32)
        self.publish()
        self.assertEqual(self.client.downloads, list(self.files))

    def test_confirmation_refetches_server_hash_before_publication(self):
        self.client.server_hash = True
        self.publish()
        confirmations = [i for i, (_, path, _) in enumerate(self.client.calls) if "/asset-upload-confirmation/" in path]
        self.assertEqual(len(confirmations), 3)
        for index in confirmations:
            self.assertEqual(self.client.calls[index + 1], ("GET", "/releases/release-1", None))
        self.assertEqual(self.client.downloads, [])

    def test_server_size_conflict_stops_without_download(self):
        self.client.server_hash = True
        self.publish()
        self.client.remote["assets"][0]["size"] += 1
        self.client.calls.clear()
        with self.assertRaisesRegex(CnbError, "size conflicts"):
            self.publish()
        self.assertEqual(self.client.downloads, [])
        self.assertTrue(all(method == "GET" for method, _, _ in self.client.calls))

    def test_existing_same_size_wrong_content_is_not_overwritten(self):
        self.publish()
        self.client.calls.clear()
        original = next(iter(self.client.files.values()))
        self.client.wrong_content = b"x" * len(original)
        with self.assertRaisesRegex(CnbError, "content conflicts"):
            self.publish()
        self.assertTrue(all(method == "GET" for method, _, _ in self.client.calls))

    def test_existing_partial_public_release_returns_to_draft_before_retry(self):
        self.publish()
        self.client.remote["assets"].pop()
        self.client.calls.clear()
        self.client.fail_upload = list(self.files)[-1]
        with self.assertRaises(CnbError):
            self.publish()
        self.assertTrue(self.client.remote["draft"])
        changes = [data for method, _, data in self.client.calls if method == "PATCH"]
        self.assertEqual(changes[0]["draft"], True)

    def test_existing_provenance_conflict_rejected(self):
        self.publish()
        self.client.remote["body"] = PROVENANCE.format("a" * 40)
        self.client.calls.clear()
        with self.assertRaisesRegex(CnbError, "provenance conflicts"):
            self.publish()
        self.assertTrue(all(method == "GET" for method, _, _ in self.client.calls))

    def test_wrong_release_tag_stops_before_writes(self):
        self.publish()
        self.client.remote["tag_name"] = "v0.9.10"
        self.client.calls.clear()
        with self.assertRaisesRegex(CnbError, "requested tag"):
            self.publish()
        self.assertTrue(all(method == "GET" for method, _, _ in self.client.calls))


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.client = CnbClient("private-token", "Nesoriel/lwe")
        self.client.opener = Mock()
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.destination = Path(self.temp.name) / "download.AppImage"

    def test_api_404_can_return_none(self):
        body = io.BytesIO(b"private-token")
        self.client.opener.open.side_effect = urllib.error.HTTPError("https://private.invalid", 404, "private-token", {}, body)
        self.assertIsNone(self.client.request("GET", "/releases/tags/no-tag", allow_not_found=True))
        self.assertTrue(body.closed)

    def test_api_redirect_is_rejected_without_leaking_error(self):
        self.client.opener.open.side_effect = urllib.error.HTTPError("https://private.invalid/?signature=private-value", 302, "private-token", {}, None)
        with self.assertRaises(CnbError) as context:
            self.client.request("GET", "/releases")
        self.assertNotIn("private", str(context.exception))
        self.assertIsNone(_NoRedirect().redirect_request(None, None, 302, None, {}, "https://private.invalid"))

    def test_api_error_body_is_not_logged(self):
        self.client.opener.open.return_value = Response(b'{"errcode":16,"errmsg":"private-token signature=private-value"}')
        with self.assertRaises(CnbError) as context:
            self.client.request("GET", "/releases")
        self.assertNotIn("private", str(context.exception))

    def test_api_cannot_be_redirected_by_a_supplied_path(self):
        for path in ("https://other.invalid/release", "//other.invalid/release", "/../user", "/%2e%2e/user"):
            with self.subTest(path=path), self.assertRaises(CnbError):
                self.client.request("GET", path)
        self.client.opener.open.assert_not_called()

    def test_download_encodes_beta_plus_and_drops_authorization(self):
        signed = "https://asset.cnb.cool/file?signature=private-value"
        body = io.BytesIO(b"private-value")
        redirect = urllib.error.HTTPError("https://api.cnb.cool", 302, "Found", {"Location": signed}, body)
        content = b"original public asset"
        self.client.opener.open.side_effect = [redirect, Response(content)]
        digest = self.client.download_asset("v0.9.11-beta.190+b45efdd", "lwe file.AppImage", self.destination)
        self.assertEqual(digest, hashlib.sha256(content).hexdigest())
        self.assertEqual(self.destination.read_bytes(), content)
        self.assertTrue(body.closed)
        requests = [call.args[0] for call in self.client.opener.open.call_args_list]
        self.assertIn("%2B", requests[0].full_url)
        self.assertIn("lwe%20file.AppImage", requests[0].full_url)
        self.assertEqual(requests[0].get_header("Authorization"), "Bearer private-token")
        self.assertIsNone(requests[1].get_header("Authorization"))

    def test_signed_download_failure_does_not_leak_or_replace_file(self):
        self.destination.write_bytes(b"existing")
        signed = "https://asset.cnb.cool/file?signature=private-value"
        body = io.BytesIO(b"private-token signature=private-value")
        self.client.opener.open.side_effect = [
            urllib.error.HTTPError("https://api.cnb.cool", 302, "Found", {"Location": signed}, None),
            urllib.error.HTTPError(signed, 403, "private-token", {}, body),
        ]
        with self.assertRaises(CnbError) as context:
            self.client.download_asset("v0.9.11", "lwe.AppImage", self.destination)
        self.assertNotIn("private", str(context.exception))
        self.assertEqual(self.destination.read_bytes(), b"existing")
        self.assertTrue(body.closed)

    def test_signed_download_redirect_is_not_followed(self):
        signed = "https://asset.cnb.cool/file?signature=private-value"
        self.client.opener.open.side_effect = [
            urllib.error.HTTPError("https://api.cnb.cool", 302, "Found", {"Location": signed}, None),
            urllib.error.HTTPError(signed, 302, "private-token", {"Location": "https://other.invalid/file"}, None),
        ]
        with self.assertRaises(CnbError) as context:
            self.client.download_asset("v0.9.11", "lwe.AppImage", self.destination)
        self.assertNotIn("private", str(context.exception))
        self.assertEqual(self.client.opener.open.call_count, 2)
        self.assertIsNone(self.client.opener.open.call_args.args[0].get_header("Authorization"))

    def test_unsafe_download_redirect_never_followed(self):
        for target in ("http://asset.cnb.cool/file", "https://private-token@asset.cnb.cool/file", "//asset.cnb.cool/file"):
            with self.subTest(target=target):
                self.client.opener.open.reset_mock()
                self.client.opener.open.side_effect = urllib.error.HTTPError("https://api.cnb.cool", 302, "Found", {"Location": target}, None)
                with self.assertRaises(CnbError):
                    self.client.download_asset("v0.9.11", "lwe.AppImage", self.destination)
                self.assertEqual(self.client.opener.open.call_count, 1)

    def test_upload_uses_stream_without_authorization(self):
        self.destination.write_bytes(b"upload original")
        self.client.opener.open.return_value = Response()
        self.client._put_file("https://asset.cnb.cool/upload?signature=private-value", self.destination, 15)
        request = self.client.opener.open.call_args.args[0]
        self.assertIsNone(request.get_header("Authorization"))
        self.assertEqual(request.get_header("Content-length"), "15")
        self.assertTrue(hasattr(request.data, "read"))

    def test_upload_http_error_is_closed_and_never_leaks_signed_url(self):
        self.destination.write_bytes(b"upload original")
        signed = "https://asset.cnb.cool/upload?signature=private-value"
        body = io.BytesIO(b"private-token")
        self.client.opener.open.side_effect = urllib.error.HTTPError(signed, 403, "private-token", {}, body)
        with self.assertRaises(CnbError) as context:
            self.client._put_file(signed, self.destination, 15)
        self.assertNotIn("private", str(context.exception))
        self.assertTrue(body.closed)
        self.assertIsNone(self.client.opener.open.call_args.args[0].get_header("Authorization"))

    def test_confirmation_cannot_send_token_to_wrong_origin_or_release(self):
        prefix = API_ORIGIN + self.client.base_path
        for url in (
            "https://other.invalid/Nesoriel/lwe/-/releases/release-1/asset-upload-confirmation/token/file",
            prefix + "/releases/release-2/asset-upload-confirmation/token/file",
            prefix + "/git/commit-assets/sha/asset-upload-confirmation/token/file",
            prefix + "/releases/release-1/asset-upload-confirmation/token/../../elsewhere",
        ):
            with self.subTest(url=url), self.assertRaises(CnbError):
                path = self.client._confirmation_path(url, "release-1")
                self.client._api_url(path)


if __name__ == "__main__":
    unittest.main()
