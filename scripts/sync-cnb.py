#!/usr/bin/env python3
"""Mirror GitHub main and a verified release to CNB without rebuilding packages."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
from urllib.parse import quote

from cnb_release import CnbClient, CnbError


GITHUB_REPO = 'YangYuS8/lwe'
CNB_REPO = 'Nesoriel/lwe'
GITHUB_URL = f'https://github.com/{GITHUB_REPO}.git'
CNB_URL = f'https://cnb.cool/{CNB_REPO}.git'
SHA = re.compile(r'[0-9a-f]{40}')
DOC_PATHS = {'package.json', 'pnpm-lock.yaml', 'pnpm-workspace.yaml', '.cnb.yml'}
ASKPASS_SCRIPT = '''#!/bin/sh
case "$1" in
*"https://cnb.cool/"*|*"https://cnb.cool'"*|*"https://cnb@cnb.cool/"*|*"https://cnb@cnb.cool'"*) ;;
*) exit 1 ;;
esac
case "$1" in
*Username*) printf "%s\\n" cnb ;;
*Password*) printf "%s\\n" "$CNB_TOKEN" ;;
*) exit 1 ;;
esac
'''


class SyncError(Exception):
    pass


def git(*args, env=None, check=True):
    result = subprocess.run(['git', *args], env=env, capture_output=True, text=True)
    if check and result.returncode:
        # Do not surface credential-helper stderr or URLs from authentication errors.
        raise SyncError(f'Git {args[0]} failed (exit {result.returncode})')
    return result


def github(path):
    result = subprocess.run(['gh', 'api', f'repos/{GITHUB_REPO}/{path}'],
                            capture_output=True, text=True)
    if result.returncode:
        raise SyncError('GitHub metadata request failed')
    return json.loads(result.stdout)


def remote_ref(ref, env):
    output = git('ls-remote', CNB_URL, ref, env=env).stdout.splitlines()
    if not output:
        return None
    if len(output) != 1 or output[0].split()[1] != ref:
        raise SyncError('Unexpected CNB reference response')
    return output[0].split()[0]


def docs_changed(before, after):
    if not SHA.fullmatch(after) or not SHA.fullmatch(before):
        raise SyncError('Documentation range must contain complete commit SHAs')
    if before == '0' * 40:
        return True
    paths = git('diff', '--name-only', before, after, '--').stdout.splitlines()
    return any(path.startswith('docs/') or path in DOC_PATHS for path in paths)


def mirror_main(env):
    needs_docs = False
    for attempt in range(3):
        git('fetch', '--no-tags', GITHUB_URL, 'refs/heads/main')
        source = git('rev-parse', 'FETCH_HEAD^{commit}').stdout.strip()
        old = remote_ref('refs/heads/main', env)
        if old == source:
            return source, needs_docs
        if old:
            git('fetch', '--no-tags', CNB_URL, 'refs/heads/main', env=env)
            if git('merge-base', '--is-ancestor', old, source, check=False).returncode:
                # A newer GitHub push may have reached CNB while this run fetched main.
                git('fetch', '--no-tags', GITHUB_URL, 'refs/heads/main')
                latest = git('rev-parse', 'FETCH_HEAD^{commit}').stdout.strip()
                if latest != source:
                    continue
                raise SyncError('CNB main has diverged from GitHub; refusing to force-push')
            needs_docs = needs_docs or docs_changed(old, source)
        else:
            needs_docs = True
        result = git('push', CNB_URL, f'{source}:refs/heads/main', env=env, check=False)
        if not result.returncode and remote_ref('refs/heads/main', env) == source:
            return source, needs_docs
        if attempt < 2:
            time.sleep(2)
    raise SyncError('CNB main synchronization failed after safe retries')


def load_release(tag, expected_sha=None):
    if git('check-ref-format', f'refs/tags/{tag}', check=False).returncode:
        raise SyncError('Invalid release tag')
    release = github(f'releases/tags/{quote(tag, safe="")}')
    if release['draft'] or release['tag_name'] != tag:
        raise SyncError('Only a published GitHub release can be mirrored')
    git('fetch', '--no-tags', GITHUB_URL, f'refs/tags/{tag}')
    tag_object = git('rev-parse', 'FETCH_HEAD').stdout.strip()
    source = git('rev-parse', 'FETCH_HEAD^{commit}').stdout.strip()
    if release['target_commitish'] != source or (expected_sha and expected_sha != source):
        raise SyncError('Release metadata, source tag, and build commit disagree')
    assets = release['assets']
    if len(assets) != 3 or {Path(a['name']).suffix for a in assets} != {'.deb', '.rpm', '.AppImage'}:
        raise SyncError('Expected exactly one deb, rpm, and AppImage')
    for asset in assets:
        if (Path(asset['name']).name != asset['name'] or '\\' in asset['name']
                or not asset['name'].startswith(('lwe_', 'lwe-'))
                or asset['state'] != 'uploaded'
                or not re.fullmatch(r'sha256:[0-9a-f]{64}', asset.get('digest') or '')):
            raise SyncError('Invalid or incomplete GitHub release asset')
    release['sync_make_latest'] = not release['prerelease'] and github('releases/latest')['tag_name'] == tag
    return release, tag_object, source


def mirror_tag(tag, tag_object, env):
    ref = f'refs/tags/{tag}'
    old = remote_ref(ref, env)
    if old and old != tag_object:
        raise SyncError('CNB release tag differs from GitHub; refusing to replace it')
    if not old:
        result = git('push', CNB_URL, f'{tag_object}:{ref}', env=env, check=False)
        if result.returncode and remote_ref(ref, env) != tag_object:
            raise SyncError('CNB release tag push failed')
    if remote_ref(ref, env) != tag_object:
        raise SyncError('CNB release tag readback failed')


def download_release(release, directory):
    result = subprocess.run(['gh', 'release', 'download', release['tag_name'], '-R', GITHUB_REPO,
                             '--dir', str(directory)], capture_output=True, text=True)
    if result.returncode:
        raise SyncError('GitHub release download failed')
    artifacts = {}
    for asset in release['assets']:
        path = directory / asset['name']
        if not path.is_file() or path.stat().st_size != asset['size']:
            raise SyncError('Downloaded asset size differs from GitHub')
        with path.open('rb') as source:
            actual = hashlib.file_digest(source, 'sha256').hexdigest()
        if asset['digest'] != 'sha256:' + actual:
            raise SyncError('Downloaded asset checksum differs from GitHub')
        artifacts[asset['name']] = path
    image = next(path for name, path in artifacts.items() if name.endswith('.AppImage'))
    subprocess.run([sys.executable, str(Path(__file__).with_name('validate-appimage.py')),
                    str(image)], check=True)
    return artifacts


def deploy_docs(client, source):
    result = client.request('POST', '/build/start', {
        'branch': 'main', 'sha': source, 'event': 'api_trigger_docs', 'sync': 'false'})
    if not result.get('success') or not result.get('sn'):
        raise SyncError('CNB documentation deployment was not triggered')
    print(f'CNB documentation build: {result["sn"]}', flush=True)
    deadline = time.monotonic() + 12 * 60
    while time.monotonic() < deadline:
        status = client.request('GET', f'/build/status/{quote(result["sn"], safe="")}')
        if status.get('status') == 'success':
            return result['sn']
        if status.get('status') in ('error', 'fail', 'failed', 'cancel', 'canceled', 'cancelled', 'timeout'):
            raise SyncError('CNB documentation deployment failed')
        time.sleep(10)
    raise SyncError('CNB documentation deployment did not finish within 12 minutes')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--release-tag')
    parser.add_argument('--expected-source-sha')
    parser.add_argument('--deploy-docs', action='store_true')
    parser.add_argument('--docs-changed-from')
    parser.add_argument('--docs-changed-to')
    args = parser.parse_args()
    if args.expected_source_sha and (not args.release_tag or not SHA.fullmatch(args.expected_source_sha)):
        raise SyncError('An expected source must be a complete SHA for a selected release')
    if bool(args.docs_changed_from) != bool(args.docs_changed_to):
        raise SyncError('Both documentation range commits are required')
    event_docs = docs_changed(args.docs_changed_from, args.docs_changed_to) if args.docs_changed_from else False
    token = os.environ.get('CNB_TOKEN')
    if not token:
        raise SyncError('CNB_TOKEN is required; configure the repository Actions secret')
    client = CnbClient(token, CNB_REPO)
    settings = client.request('GET', '/settings/cloud-native-build')
    if settings.get('auto_trigger') is not False:
        raise SyncError('Disable CNB automatic Git builds before mirroring historical tags')
    # Prepare and validate every source byte before publishing any release objects.
    with tempfile.TemporaryDirectory(prefix='lwe-cnb-') as temporary:
        root = Path(temporary)
        askpass = root / 'askpass.sh'
        askpass.write_text(ASKPASS_SCRIPT)
        askpass.chmod(0o700)
        env = {**os.environ, 'GIT_TERMINAL_PROMPT': '0', 'GIT_ASKPASS': str(askpass),
               'GIT_CONFIG_COUNT': '2', 'GIT_CONFIG_KEY_0': 'credential.helper',
               'GIT_CONFIG_VALUE_0': '', 'GIT_CONFIG_KEY_1': 'credential.https://cnb.cool.helper',
               'GIT_CONFIG_VALUE_1': ''}
        release = None
        if args.release_tag:
            release, tag_object, source = load_release(args.release_tag, args.expected_source_sha)
            artifacts = download_release(release, root)
        main_source, needs_docs = mirror_main(env)
        print(f'CNB main verified at {main_source}', flush=True)
        if release:
            mirror_tag(args.release_tag, tag_object, env)
            report = client.publish_release(release, artifacts, source)
            print(json.dumps({'release_tag': args.release_tag, 'source_sha': source,
                              'cnb_release_id': report['id'], 'assets': {
                                  asset['name']: asset['digest'] for asset in release['assets']
                              }}), flush=True)
        if args.deploy_docs or event_docs or needs_docs:
            deploy_docs(client, main_source)


if __name__ == '__main__':
    try:
        main()
    except (CnbError, SyncError, subprocess.CalledProcessError) as error:
        print(f'CNB synchronization failed: {error}', file=sys.stderr)
        sys.exit(1)
