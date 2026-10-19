#!/usr/bin/env python3
"""Remove generated ConsoleCrypt CI output only after release publication.

Never use this as a general-purpose rm command. The workspace, repository,
workflow identity and optional external cache ownership are checked first.
"""
import argparse
import errno
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time

REPOSITORY = 'evsikovas/consolecrypt-client'
REMOTES = {f'https://github.com/{REPOSITORY}', f'https://github.com/{REPOSITORY}.git',
           f'git@github.com:{REPOSITORY}.git'}
GENERATED = (
    'target', 'client/rust/target', 'crates/target',
    'client/flutter/build', 'client/flutter/.dart_tool', 'dist',
    'client/flutter/android/.gradle', 'client/flutter/android/.cxx',
    'client/flutter/android/app/.cxx',
)
OWNER_FILE = '.consolecrypt-ci-owner.json'
WORKSPACE_OWNER_FILE = '.consolecrypt-ci-workspace.json'
BUILD_PLATFORMS = {'windows': 'windows', 'macos': 'macos', 'android': 'macos', 'ios': 'macos'}
PUBLISH_CLEANUP_PLATFORMS = {'cleanup_windows': 'windows', 'cleanup_macos': 'macos'}
LEGACY_WINDOWS_CACHE = r'C:\GitLab-Runner\tools\consolecrypt-local-ai-smoke'
MODEL_DIGESTS = {
    'qwen2.5-coder-1.5b-instruct-q4_k_m.gguf': 'cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046',
    'Qwen3.5-4B-Q4_K_M.gguf': '00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4',
    'Qwen3.5-9B-Q4_K_M.gguf': '03b74727a860a56338e042c4420bb3f04b2fec5734175f4cb9fa853daf52b7e8',
}
WINDOWS_RUNTIME_DIGEST = 'b1e0a14abdb4665c1b821b377ba063b221cec62882c373065cf8fb672988d3ff'
REMOVE_RETRY_DELAYS = (0.1, 0.25, 0.5, 1.0, 2.0)


def absolute(path):
    return Path(os.path.abspath(path))


def is_link(info):
    return stat.S_ISLNK(info.st_mode) or bool(
        getattr(info, 'st_file_attributes', 0) & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0x400))


def no_link_ancestors(path):
    """Check lexical ancestors before resolving anything, including broken links."""
    for item in [*reversed(path.parents), path]:
        try:
            info = item.lstat()
        except FileNotFoundError:
            continue
        if is_link(info):
            raise ValueError('Refusing a symlink/reparse point in an owned path')


def git(root, *args):
    return subprocess.run(['git', '-C', str(root), *args], check=True,
                          capture_output=True, timeout=30).stdout.decode().strip()


def identity(environment):
    if environment.get('GITHUB_ACTIONS') != 'true' or environment.get('GITHUB_REPOSITORY') != REPOSITORY:
        raise ValueError('Cleanup is restricted to the ConsoleCrypt GitHub Actions runner')
    values = [environment.get(key, '') for key in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_JOB')]
    if not all(re.fullmatch(r'[0-9]+', value) for value in values[:2]) or not re.fullmatch(r'[A-Za-z0-9_-]+', values[2]):
        raise ValueError('Missing or invalid CI job identity')
    return {'repository': REPOSITORY, 'run': values[0], 'attempt': values[1], 'job': values[2]}


def validate_workspace(workspace, environment, script_root, maintenance=False):
    identity(environment)
    expected = environment.get('GITHUB_WORKSPACE')
    if not expected or workspace != absolute(expected) or workspace.parent == workspace:
        raise ValueError('Workspace must exactly match GITHUB_WORKSPACE')
    no_link_ancestors(workspace)
    expected_script_root = workspace / '.ci-maintenance-tools' if maintenance else workspace
    if script_root != expected_script_root:
        raise ValueError('Cleanup script is outside its expected source checkout')
    no_link_ancestors(script_root)
    if maintenance and (environment.get('GITHUB_EVENT_NAME') != 'workflow_dispatch'
                        or environment.get('GITHUB_REF') != 'refs/heads/main'):
        raise ValueError('Maintenance runs only by dispatch from main')
    for root in {workspace, script_root}:
        no_link_ancestors(root / '.git')
        if not (root / '.git').is_dir() or (root / '.git').is_symlink():
            raise ValueError('Expected a normal CI Git checkout')
        if absolute(git(root, 'rev-parse', '--show-toplevel')) != root:
            raise ValueError('Git root differs from the expected checkout')
        if git(root, 'remote', 'get-url', 'origin') not in REMOTES:
            raise ValueError('Unexpected repository origin')
        if not (root / 'client/flutter/pubspec.yaml').is_file() or not (root / 'client/rust/Cargo.toml').is_file():
            raise ValueError('Missing ConsoleCrypt workspace identity files')


def workspace_targets(workspace):
    targets = [workspace / name for name in GENERATED]
    # Preflight every target before deleting any. Never delete a tracked source.
    for target in targets:
        no_link_ancestors(target)
    if git(workspace, 'ls-files', '-z', '--', *GENERATED):
        raise ValueError('A generated output path contains tracked files')
    return targets


def cache_location(environment):
    identity(environment)
    workspace = environment.get('GITHUB_WORKSPACE', '')
    if not workspace or not Path(workspace).is_absolute():
        raise ValueError('Expected an absolute CI workspace for the reusable cache')
    workspace = absolute(workspace)
    root = workspace / '.ci-cache/local-ai-smoke'
    no_link_ancestors(root)
    return root, {'schema': 2, 'repository': REPOSITORY, 'workspace': str(workspace)}


def read_json(path):
    no_link_ancestors(path)
    if path.stat().st_size > 1024 * 1024:
        raise ValueError('Oversized ownership metadata')
    return json.loads(path.read_text(encoding='utf-8'))


def prepare_ai_cache(environment):
    root, owner = cache_location(environment)
    marker = root / OWNER_FILE
    if root.exists():
        if not marker.is_file() or read_json(marker) != owner:
            raise ValueError('Existing AI smoke cache has no matching job owner')
    else:
        root.mkdir(parents=True)
        marker.write_text(json.dumps(owner, sort_keys=True) + '\n', encoding='utf-8')
    return root


def owned_ai_cache(environment):
    root, owner = cache_location(environment)
    if not root.exists():
        return []
    if read_json(root / OWNER_FILE) != owner:
        raise ValueError('AI smoke cache belongs to another workspace')
    if git(absolute(environment['GITHUB_WORKSPACE']), 'ls-files', '-z', '--', '.ci-cache/local-ai-smoke'):
        raise ValueError('AI smoke cache contains tracked files')
    return [root]


def workspace_owner(workspace, environment):
    owner = identity(environment)
    platform = BUILD_PLATFORMS.get(owner['job'])
    commit = environment.get('GITHUB_SHA', '')
    if platform is None or not re.fullmatch(r'[a-f0-9]{40}', commit):
        raise ValueError('Expected a native build job and immutable source commit')
    if git(workspace, 'rev-parse', 'HEAD') != commit:
        raise ValueError('Build checkout differs from the requested source commit')
    git(workspace, 'diff', '--quiet', 'HEAD', '--')
    return {'schema': 1, 'repository': REPOSITORY, 'workspace': str(workspace),
            'run': owner['run'], 'attempt': owner['attempt'], 'commit': commit, 'platform': platform}


def read_workspace_owner(workspace):
    marker = workspace / WORKSPACE_OWNER_FILE
    no_link_ancestors(marker)
    if not stat.S_ISREG(marker.lstat().st_mode) or marker.stat().st_nlink != 1:
        raise ValueError('Expected a regular workspace ownership marker')
    owner = read_json(marker)
    if (not isinstance(owner, dict) or set(owner) != {'schema', 'repository', 'workspace', 'run', 'attempt', 'commit', 'platform'}
            or type(owner.get('schema')) is not int or owner['schema'] != 1
            or owner['repository'] != REPOSITORY or owner['workspace'] != str(workspace)
            or owner['platform'] not in ('windows', 'macos')
            or any(not isinstance(owner[key], str) or not re.fullmatch(r'[0-9]+', owner[key]) for key in ('run', 'attempt'))
            or not isinstance(owner['commit'], str) or not re.fullmatch(r'[a-f0-9]{40}', owner['commit'])):
        raise ValueError('Unexpected workspace ownership marker')
    return owner


def claim_workspace(workspace, environment):
    owner = workspace_owner(workspace, environment)
    marker = workspace / WORKSPACE_OWNER_FILE
    no_link_ancestors(marker)
    if marker.exists():
        read_workspace_owner(workspace)  # Never overwrite unknown local data.
    descriptor, temporary = tempfile.mkstemp(prefix=WORKSPACE_OWNER_FILE + '.', dir=workspace)
    try:
        with os.fdopen(descriptor, 'w') as writer:
            json.dump(owner, writer, sort_keys=True)
            writer.write('\n')
            writer.flush()
            os.fsync(writer.fileno())
        os.replace(temporary, marker)
    finally:
        Path(temporary).unlink(missing_ok=True)


def verify_published_workspace(workspace, environment):
    current = identity(environment)
    platform = PUBLISH_CLEANUP_PLATFORMS.get(current['job'])
    if (platform is None or environment.get('CC_RELEASE_PUBLISHED') != 'true'
            or environment.get('GITHUB_EVENT_NAME') != 'push'
            or not re.fullmatch(r'refs/tags/v[0-9]+\.[0-9]+\.[0-9]+', environment.get('GITHUB_REF', ''))):
        raise ValueError('Full cleanup requires a successfully published stable release')
    owner = read_workspace_owner(workspace)
    # Rerun-failed-jobs may publish outputs from an earlier successful attempt
    # of the same run. A newer run/attempt or another checkout is never claimed.
    if (owner['run'] != current['run'] or int(owner['attempt']) > int(current['attempt'])
            or owner['platform'] != platform or owner['commit'] != environment.get('GITHUB_SHA')
            or git(workspace, 'rev-parse', 'HEAD') != owner['commit']):
        raise ValueError('Workspace is not owned by this published release')


def walk_regular(root):
    """Legacy unmarked cache gets stricter treatment: no links anywhere."""
    files = []
    def fail(error):
        raise error
    for current, directories, names in os.walk(root, followlinks=False, onerror=fail):
        for name in directories + names:
            path = Path(current) / name
            info = path.lstat()
            if is_link(info) or not (stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode)):
                raise ValueError('Unexpected link or special file in legacy CI cache')
            if stat.S_ISREG(info.st_mode):
                files.append(path)
    return files


def legacy_cache_targets(root):
    """Only called for the fixed Windows CI path, after explicit operator opt-in."""
    no_link_ancestors(root)
    if not root.exists():
        return []
    if not root.is_dir():
        raise ValueError('Legacy CI cache is not a directory')
    walk_regular(root)
    targets = [root / 'models' / name for name in MODEL_DIGESTS]
    for digest in [*MODEL_DIGESTS.values(), WINDOWS_RUNTIME_DIGEST]:
        targets += [root / 'downloads' / (digest + suffix) for suffix in ('', '.part', '.part.json')]
    runtime = root / 'runtime-b11739-windowsX64'
    if runtime.exists():
        receipt = read_json(runtime / 'verified.json')
        entries = receipt.get('files', {})
        if (receipt.get('archive') != WINDOWS_RUNTIME_DIGEST or not isinstance(entries, dict)
                or not 0 < len(entries) <= 2048 or receipt.get('executable') not in entries):
            raise ValueError('Unknown legacy runtime receipt')
        if PurePosixPath(receipt['executable']).name != 'llama-server.exe':
            raise ValueError('Unexpected legacy runtime executable')
        for name, digest in entries.items():
            path = PurePosixPath(name)
            if (not name or '\\' in name or ':' in name or path.is_absolute()
                    or any(part in ('.', '..') for part in name.split('/'))
                    or not isinstance(digest, str) or not re.fullmatch(r'[0-9a-f]{64}', digest)):
                raise ValueError('Invalid legacy runtime manifest path')
        actual = {path.relative_to(runtime).as_posix() for path in walk_regular(runtime)}
        if actual != {*entries, 'verified.json'}:
            raise ValueError('Unknown files in legacy runtime; preserve it for inspection')
        targets.append(runtime)
    # Preserve selection.json, receipts/logs and any unrelated/unknown file.
    for target in targets:
        if target.exists() and target != runtime and not target.is_file():
            raise ValueError('Expected a regular legacy artifact')
    return targets


def _remove_owned_once(path):
    """Never follow nested links/junctions, even inside a generated tree."""
    try:
        info = path.lstat()
    except FileNotFoundError:
        return
    if is_link(info):
        if os.name == 'nt' and stat.S_ISDIR(info.st_mode) and not path.is_symlink():
            path.rmdir()  # Directory junction: unlink the junction, not its target.
        else:
            path.unlink()
    elif stat.S_ISDIR(info.st_mode):
        # shutil.rmtree uses fd-relative traversal on platforms supporting it,
        # and does not traverse Windows directory junctions (Python >= 3.8).
        def writable_retry(function, name, exc):
            if not isinstance(exc[1], PermissionError):
                raise exc[1]
            no_link_ancestors(Path(name))
            if Path(name).lstat().st_nlink > 1 and not Path(name).is_dir():
                raise exc[1]  # Never chmod a hardlinked source outside the tree.
            os.chmod(name, stat.S_IREAD | stat.S_IWRITE | stat.S_IEXEC)
            function(name)
        shutil.rmtree(path, onerror=writable_retry)
    elif stat.S_ISREG(info.st_mode):
        try:
            path.unlink()
        except PermissionError:
            if path.lstat().st_nlink > 1:
                raise
            path.chmod(stat.S_IREAD | stat.S_IWRITE)
            path.unlink()
    else:
        raise ValueError('Refusing special filesystem object')


def remove_owned(path):
    """Tolerate bounded traversal races while cancelled build writers exit."""
    for attempt in range(len(REMOVE_RETRY_DELAYS) + 1):
        # A retry must not traverse a generated root replaced by a link while
        # the previous deletion was in progress. Nested links remain unlinked.
        no_link_ancestors(path)
        try:
            _remove_owned_once(path)
            return
        except OSError as error:
            if error.errno not in (errno.ENOENT, errno.ENOTEMPTY) or attempt == len(REMOVE_RETRY_DELAYS):
                raise
            time.sleep(REMOVE_RETRY_DELAYS[attempt])


def error_code(error):
    """Stable diagnostics without exception messages, filenames or environment."""
    parts = [type(error).__name__]
    if isinstance(error, OSError):
        if isinstance(error.errno, int):
            parts.append('errno=' + errno.errorcode.get(error.errno, str(error.errno)))
        if isinstance(getattr(error, 'winerror', None), int):
            parts.append('winerror=' + str(error.winerror))
    return ', '.join(parts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, required=True)
    parser.add_argument('--maintenance', action='store_true')
    parser.add_argument('--legacy-windows-ai-cache', action='store_true')
    parser.add_argument('--prepare-ai-cache', action='store_true')
    parser.add_argument('--claim-workspace', action='store_true')
    parser.add_argument('--after-publication', action='store_true')
    args = parser.parse_args()
    workspace = absolute(args.workspace)
    validate_workspace(workspace, os.environ, absolute(__file__).parents[2], args.maintenance)
    if sum((args.prepare_ai_cache, args.claim_workspace, args.after_publication, args.maintenance)) != 1:
        raise ValueError('Choose one explicit ownership, publication or maintenance operation')
    if args.claim_workspace:
        if args.legacy_windows_ai_cache:
            raise ValueError('Workspace registration cannot clean caches')
        claim_workspace(workspace, os.environ)
        print('CI_WORKSPACE_OWNER_REGISTERED')
        return
    if args.prepare_ai_cache:
        if args.maintenance or args.legacy_windows_ai_cache:
            raise ValueError('Cache preparation cannot perform maintenance')
        print(prepare_ai_cache(os.environ))
        return
    if args.after_publication:
        verify_published_workspace(workspace, os.environ)
    if args.maintenance:
        if (os.environ.get('CC_RELEASE_PUBLISHED') != 'true'
                or not re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+', os.environ.get('CC_PUBLISHED_TAG', ''))
                or not re.fullmatch(r'[a-f0-9]{40}', os.environ.get('CC_PUBLISHED_COMMIT', ''))
                or git(workspace, 'rev-parse', 'HEAD') != os.environ['CC_PUBLISHED_COMMIT']):
            raise ValueError('Maintenance requires the published release that owns this checkout')
    targets = workspace_targets(workspace)
    targets += owned_ai_cache(os.environ)
    if args.legacy_windows_ai_cache:
        if not args.maintenance or sys.platform != 'win32':
            raise ValueError('Legacy cache cleanup requires explicit Windows maintenance')
        targets += legacy_cache_targets(Path(LEGACY_WINDOWS_CACHE))
    failures = 0
    for target in targets:
        try:
            no_link_ancestors(target)
            existed = target.exists()
            remove_owned(target)
            if existed:
                print(f'Removed generated output: {target.name}')
        except (OSError, ValueError) as error:
            failures += 1
            print(f'Cleanup could not remove generated output: {target.name} ({error_code(error)})', file=sys.stderr)
    if failures:
        raise RuntimeError(f'{failures} generated output paths could not be removed')
    print('CI_CLEANUP_PASS')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        # No environment or resolved source/config values are printed.
        print(f'CI cleanup refused or failed ({error_code(error)}).', file=sys.stderr)
        sys.exit(1)
