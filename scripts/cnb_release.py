"""Copy verified GitHub release files to CNB without rebuilding them."""

import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


API_ORIGIN = "https://api.cnb.cool"
CHUNK_SIZE = 1024 * 1024
PROVENANCE = "<!-- lwe-cnb-mirror source_sha={} -->"


class CnbError(RuntimeError):
    pass


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def _https_url(url):
    try:
        parts = urllib.parse.urlsplit(url)
        valid = (
            parts.scheme == "https" and parts.hostname and not parts.username
            and not parts.password and not parts.fragment
            and parts.port in (None, 443)
        )
    except (TypeError, ValueError):
        valid = False
    if not valid:
        raise CnbError("CNB returned an unsafe transfer URL")
    return parts


def _file_digest(path):
    digest = hashlib.sha256()
    size = 0
    try:
        with Path(path).open("rb") as source:
            for chunk in iter(lambda: source.read(CHUNK_SIZE), b""):
                digest.update(chunk)
                size += len(chunk)
    except OSError:
        raise CnbError("Cannot read a release artifact") from None
    return size, digest.hexdigest()


def _server_hash_matches(asset, size, sha256):
    if type(asset.get("size")) is not int or asset["size"] != size:
        raise CnbError("CNB artifact size conflicts with the GitHub artifact")
    algorithm, value = asset.get("hash_algo"), asset.get("hash_value")
    if isinstance(algorithm, str) and algorithm.lower() in ("sha256", "sha-256") and value:
        if not isinstance(value, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", value) or value.lower() != sha256:
            raise CnbError("CNB artifact content conflicts with the GitHub artifact")
        return True
    return False


class CnbClient:
    def __init__(self, token, repo, timeout=120):
        if not isinstance(token, str) or not token.strip():
            raise CnbError("A CNB access token is required")
        if not isinstance(repo, str) or not re.fullmatch(r"[\w.-]+(?:/[\w.-]+)+", repo):
            raise CnbError("Invalid CNB repository")
        if any(part in (".", "..") for part in repo.split("/")):
            raise CnbError("Invalid CNB repository")
        self.token = token
        self.repo = repo
        self.base_path = "/" + urllib.parse.quote(repo, safe="/") + "/-"
        self.timeout = timeout
        self.opener = urllib.request.build_opener(_NoRedirect())

    def _api_url(self, path):
        parts = urllib.parse.urlsplit(path)
        if (
            parts.scheme or parts.netloc or parts.fragment or not path.startswith("/")
            or path.startswith("//")
            or any(x in (".", "..") for x in urllib.parse.unquote(parts.path).split("/"))
        ):
            raise CnbError("Invalid CNB API path")
        return API_ORIGIN + self.base_path + path

    def _api_request(self, method, path, data=None):
        headers = {
            "Authorization": "Bearer " + self.token,
            "Accept": "application/vnd.cnb.api+json",
        }
        payload = None
        if data is not None:
            payload = json.dumps(data, ensure_ascii=False).encode("utf-8")
            headers["Content-Type"] = "application/json"
        return urllib.request.Request(self._api_url(path), payload, headers, method=method)

    def request(self, method, path, data=None, allow_not_found=False):
        """Call a repository-relative API path. Never log error bodies or URLs."""
        req = self._api_request(method, path, data)
        try:
            with self.opener.open(req, timeout=self.timeout) as response:
                if not 200 <= response.getcode() < 300:
                    raise CnbError("Unexpected CNB API response")
                raw = response.read(10 * CHUNK_SIZE + 1)
        except urllib.error.HTTPError as error:
            try:
                if error.code == 404 and allow_not_found:
                    return None
                raise CnbError(f"CNB API request failed (HTTP {error.code})") from None
            finally:
                error.close()
        except (OSError, ValueError, http.client.HTTPException):
            raise CnbError("CNB API request failed") from None
        if len(raw) > 10 * CHUNK_SIZE:
            raise CnbError("CNB API response is too large")
        if not raw:
            return None
        try:
            result = json.loads(raw)
        except (ValueError, UnicodeError):
            raise CnbError("Invalid CNB API response") from None
        if isinstance(result, dict) and result.get("errcode") not in (None, 0):
            raise CnbError("CNB API reported an error")
        return result

    def download_asset(self, tag, name, destination):
        """Resolve the authenticated API redirect, then download without Bearer."""
        path = "/releases/download/{}/{}".format(
            urllib.parse.quote(tag, safe=""), urllib.parse.quote(name, safe="")
        )
        req = self._api_request("GET", path)
        try:
            with self.opener.open(req, timeout=self.timeout) as response:
                if response.getcode() != 302:
                    raise CnbError("Expected a CNB asset download redirect")
                location = response.headers.get("Location")
        except urllib.error.HTTPError as error:
            try:
                if error.code != 302:
                    raise CnbError(f"CNB asset lookup failed (HTTP {error.code})") from None
                location = (error.headers or {}).get("Location")
            finally:
                error.close()
        except (OSError, ValueError, http.client.HTTPException):
            raise CnbError("CNB asset lookup failed") from None
        _https_url(location)
        destination = Path(destination)
        temporary = None
        try:
            transfer = urllib.request.Request(location, method="GET")
            with self.opener.open(transfer, timeout=self.timeout) as response:
                if response.getcode() != 200:
                    raise CnbError("Unexpected CNB asset download response")
                with tempfile.NamedTemporaryFile(
                    dir=destination.parent, prefix=".cnb-download-", delete=False
                ) as output:
                    temporary = Path(output.name)
                    digest = hashlib.sha256()
                    for chunk in iter(lambda: response.read(CHUNK_SIZE), b""):
                        output.write(chunk)
                        digest.update(chunk)
            os.replace(temporary, destination)
            return digest.hexdigest()
        except urllib.error.HTTPError as error:
            error.close()
            raise CnbError(f"CNB asset download failed (HTTP {error.code})") from None
        except (OSError, ValueError, http.client.HTTPException):
            raise CnbError("CNB asset download failed") from None
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)

    def _put_file(self, url, path, size):
        _https_url(url)
        try:
            with Path(path).open("rb") as source:
                req = urllib.request.Request(
                    url, source,
                    {"Content-Type": "application/octet-stream", "Content-Length": str(size)},
                    method="PUT",
                )
                with self.opener.open(req, timeout=self.timeout) as response:
                    if not 200 <= response.getcode() < 300:
                        raise CnbError("Unexpected CNB asset upload response")
        except urllib.error.HTTPError as error:
            error.close()
            raise CnbError(f"CNB asset upload failed (HTTP {error.code})") from None
        except (OSError, ValueError, http.client.HTTPException):
            raise CnbError("CNB asset upload failed") from None

    def _confirmation_path(self, url, release_id):
        if isinstance(url, str) and url.startswith("/") and not url.startswith("//"):
            url = API_ORIGIN + url
        parts = _https_url(url)
        prefix = self.base_path + "/releases/" + urllib.parse.quote(release_id, safe="")
        prefix += "/asset-upload-confirmation/"
        if parts.netloc != "api.cnb.cool" or not parts.path.startswith(prefix):
            raise CnbError("CNB returned an unrelated upload confirmation URL")
        token, separator, asset = parts.path[len(prefix):].partition("/")
        if not token or not separator or not asset:
            raise CnbError("CNB returned an invalid upload confirmation URL")
        query = dict(urllib.parse.parse_qsl(parts.query, keep_blank_values=True))
        query["ttl"] = "0"
        path = parts.path[len(self.base_path):] + "?" + urllib.parse.urlencode(query)
        self._api_url(path)
        return path

    def publish_release(self, release, artifacts, source_sha):
        """Publish or resume a release only after local and remote hash checks."""
        if not isinstance(source_sha, str) or not re.fullmatch(r"[0-9a-f]{40}", source_sha):
            raise CnbError("A full source commit SHA is required")
        tag = release.get("tag_name")
        if not isinstance(tag, str) or not tag or release.get("draft"):
            raise CnbError("A published GitHub release is required")
        if not isinstance(release.get("prerelease"), bool):
            raise CnbError("GitHub release channel is missing")
        make_latest = release.get("sync_make_latest", False)
        if not isinstance(make_latest, bool):
            raise CnbError("GitHub latest release decision must be a boolean")
        metadata = release.get("assets")
        if not isinstance(metadata, list):
            raise CnbError("GitHub release assets are missing")
        names = list(artifacts)
        if any(not isinstance(name, str) for name in names):
            raise CnbError("Invalid release artifact name")
        extensions = [Path(name).suffix for name in names]
        if len(names) != 3 or sorted(extensions) != [".AppImage", ".deb", ".rpm"]:
            raise CnbError("Expected one AppImage, deb and rpm artifact")
        expected = {}
        for name in names:
            if not isinstance(name, str) or Path(name).name != name or "\\" in name:
                raise CnbError("Invalid release artifact name")
            matches = [a for a in metadata if isinstance(a, dict) and a.get("name") == name]
            if len(matches) != 1:
                raise CnbError("GitHub artifact metadata is missing or ambiguous")
            asset = matches[0]
            digest = asset.get("digest")
            if not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
                raise CnbError("GitHub artifact SHA256 is missing")
            size = asset.get("size")
            if type(size) is not int or size <= 0:
                raise CnbError("GitHub artifact size is invalid")
            actual_size, actual_hash = _file_digest(artifacts[name])
            if actual_size != size or actual_hash != digest[7:]:
                raise CnbError("Local artifact does not match the GitHub size and SHA256")
            expected[name] = (size, actual_hash)

        encoded_tag = urllib.parse.quote(tag, safe="")
        git_tag = self.request("GET", "/git/tags/" + encoded_tag, allow_not_found=True)
        commit = git_tag.get("commit") if isinstance(git_tag, dict) else None
        if not isinstance(commit, dict) or commit.get("sha") != source_sha:
            raise CnbError("CNB tag does not match the source commit; mirror Git refs first")
        existing = self.request("GET", "/releases/tags/" + encoded_tag, allow_not_found=True)
        if existing is not None and not isinstance(existing, dict):
            raise CnbError("Invalid CNB release response")
        if existing:
            body = existing.get("body") or ""
            if not isinstance(body, str):
                raise CnbError("Invalid CNB release provenance")
            markers = re.findall(r"<!-- lwe-cnb-mirror source_sha=([^\s>]+) -->", body)
            if any(value != source_sha for value in markers) or (
                "<!-- lwe-cnb-mirror" in body and not markers
            ):
                raise CnbError("CNB release provenance conflicts with the source commit")
        body = release.get("body") or ""
        title = release.get("name") or tag
        if not isinstance(body, str) or not isinstance(title, str):
            raise CnbError("Invalid GitHub release title or body")
        body = body + "\n\n" + PROVENANCE.format(source_sha)
        latest = make_latest and not release["prerelease"]
        fields = {
            "name": title, "body": body, "draft": True,
            "prerelease": release["prerelease"], "make_latest": "false",
        }
        if existing is None:
            existing = self.request("POST", "/releases", {
                **fields, "tag_name": tag, "target_commitish": source_sha,
            })
            if not isinstance(existing, dict) or existing.get("draft") is not True:
                raise CnbError("CNB did not create the release as a draft")
        if existing.get("tag_name") != tag:
            raise CnbError("CNB release does not match the requested tag")
        release_id = existing.get("id")
        if not isinstance(release_id, str) or not re.fullmatch(r"[\w.-]+", release_id):
            raise CnbError("Invalid CNB release identifier")
        release_path = "/releases/" + urllib.parse.quote(release_id, safe="")
        remote_assets = existing.get("assets") or []
        if not isinstance(remote_assets, list):
            raise CnbError("Invalid CNB asset list")
        remote = {}
        for asset in remote_assets:
            if isinstance(asset, dict) and asset.get("name") in expected:
                if asset["name"] in remote:
                    raise CnbError("CNB has duplicate release assets")
                remote[asset["name"]] = asset
        for name, asset in remote.items():
            _server_hash_matches(asset, *expected[name])
        missing = set(expected) - set(remote)
        if missing and existing.get("draft") is not True:
            self.request("PATCH", release_path, fields)

        with tempfile.TemporaryDirectory(prefix="lwe-cnb-verify-") as directory:
            for name, (size, sha256) in expected.items():
                if name not in remote:
                    upload = self.request("POST", release_path + "/asset-upload-url", {
                        "asset_name": name, "size": size, "overwrite": False, "ttl": 0,
                    })
                    if not isinstance(upload, dict) or type(upload.get("expires_in_sec")) is not int or upload["expires_in_sec"] <= 0:
                        raise CnbError("Invalid CNB upload session")
                    confirmation = self._confirmation_path(upload.get("verify_url"), release_id)
                    _https_url(upload.get("upload_url"))
                    self._put_file(upload["upload_url"], artifacts[name], size)
                    self.request("POST", confirmation)
                    refreshed = self.request("GET", release_path)
                    if not isinstance(refreshed, dict) or not isinstance(refreshed.get("assets"), list):
                        raise CnbError("Invalid CNB release asset readback")
                    matches = [a for a in refreshed["assets"] if isinstance(a, dict) and a.get("name") == name]
                    if len(matches) != 1:
                        raise CnbError("CNB confirmed asset readback is incomplete or ambiguous")
                    remote[name] = matches[0]
                if _server_hash_matches(remote[name], size, sha256):
                    continue
                destination = Path(directory) / name
                actual = self.download_asset(tag, name, destination)
                if actual != sha256 or destination.stat().st_size != size:
                    raise CnbError("CNB artifact content conflicts with the GitHub artifact")

        current = self.request("GET", release_path)
        if not isinstance(current, dict) or not isinstance(current.get("assets"), list):
            raise CnbError("Invalid CNB release readback")
        listed = {a.get("name"): a for a in current.get("assets", []) if isinstance(a, dict)}
        if any(name not in listed for name in expected):
            raise CnbError("CNB release asset readback is incomplete")
        for name, (size, sha256) in expected.items():
            _server_hash_matches(listed[name], size, sha256)
        final_fields = {**fields, "draft": False, "make_latest": str(latest).lower()}
        if (
            any(current.get(key) != final_fields[key] for key in ("name", "body", "draft", "prerelease"))
            or current.get("is_latest") != latest
        ):
            self.request("PATCH", release_path, final_fields)
        for attempt in range(12):
            current = self.request("GET", "/releases/tags/" + encoded_tag)
            if (
                not isinstance(current, dict) or current.get("tag_name") != tag
                or any(not isinstance(current.get(key), bool) for key in ("draft", "prerelease", "is_latest"))
                or any(not isinstance(current.get(key), str) for key in ("name", "body"))
                or not isinstance(current.get("assets"), list)
            ):
                raise CnbError("Invalid CNB release publication readback")
            if (
                all(current[key] == final_fields[key] for key in ("name", "body", "draft", "prerelease"))
                and current["is_latest"] == latest
            ):
                listed = {a.get("name"): a for a in current["assets"] if isinstance(a, dict)}
                if all(name in listed for name in expected):
                    for name, (size, sha256) in expected.items():
                        _server_hash_matches(listed[name], size, sha256)
                    return current
            if attempt < 11:
                time.sleep(2)
        raise CnbError("CNB release publication readback failed after 12 attempts")
