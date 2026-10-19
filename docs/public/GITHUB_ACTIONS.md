# GitHub Actions: native client builds

The `Client tag release` workflow runs only when an exact stable `vX.Y.Z`
tag is pushed. The version must match `pubspec.yaml`, and the tagged commit
must already belong to `main`. Branch pushes and manual dispatch do not build
packages. No pull request code executes on persistent native runners.

| Runner labels | Jobs | Required tools |
|---|---|---|
| `self-hosted, macOS, ARM64, consolecrypt, client` | Android; macOS/iOS by default | Flutter 3.47.5, Rust 1.98, Python; Android SDK 36, build-tools 36.0.0, NDK 28.2.13676358, CMake 3.22.1; full Xcode and CocoaPods for Apple builds |
| `self-hosted, Windows, X64, consolecrypt, client` | Windows x64 | MSVC C++/Windows SDK, Git, Python, Rust; Flutter, Strawberry Perl and Inno Setup are discovered or provisioned by `ci-windows.ps1` |
| `cc-linux-k3s` | Linux DEB/RPM | Home ARC scale set with Docker-in-Docker, Python 3 and a shared work/temp volume; pinned Ubuntu 22.04 builder contains native tools and isolated Secret Service tests |

Repository variable `MACOS_BUILD_RUNNER` can override only the macOS/iOS runner
with a JSON array, for example `["macos-26"]` for a hosted runner with full Xcode.
Remove the override after installing Xcode on the dedicated Mac. Android always
uses the dedicated Mac and its existing signing key. `ci-macos-tools.sh` provisions
the pinned Flutter SDK when it is absent. iOS follows macOS; Android, Windows and
Linux can build independently. A single self-hosted Mac serializes its jobs.

The ARC runner and Docker daemon must mount `RUNNER_TEMP` at the same absolute
path. `ci-linux.sh` stages the source snapshot there, not in an unshared `/tmp`.
The home cluster is a build environment; these jobs do not deploy production there.

Native jobs have `contents: read`. After all five platforms succeed, a hosted
publisher validates the version receipts, assembles a closed list of seven
installers plus SHA-256 checksums, creates a draft GitHub Release, uploads its
files, and publishes it. Only that job gets `contents: write` and `actions: write`.
It then dispatches `update-feeds.yml` on trusted `main` to refresh signed metadata.
Website and sync-server deployment use their own tag workflows.

On Windows, enable Developer Mode so Flutter can create plugin symlinks, and
choose a short work directory when registering the runner:

```powershell
.\config.cmd --url https://github.com/evsikovas/consolecrypt-client --work C:\cc-ghw --labels consolecrypt,client
```

Enter the repository registration token at the interactive prompt. The verified
Windows runner uses `C:\cc-ghw`. A longer nested workspace reached 260 characters
inside vendored OpenSSL and made its native Perl configuration fail even though
the file existed. The short directory resolved the failure; enabling symlinks or
Windows long-path support alone does not fix this Perl limit. When relocating an
existing runner, wait until it is idle and preserve its configuration and old
checkout/cache until the new location is verified.

## Build numbering

`client/scripts/github-build-number.py` sets:

```text
10000 + GITHUB_RUN_NUMBER * 1000 + GITHUB_RUN_ATTEMPT * 10 + platform
platform: macOS=1, Windows=2, Linux=3, Android=4, iOS=5
```

The first run/attempt yields `11011`–`11015`, above every GitLab release build.
Retries receive new numbers. Attempts above 99 and Android versionCode overflow
fail closed. Do not reset/recreate this workflow and reuse its run-number sequence
without raising the baseline above all published builds. Do not publish an older
run after a newer run has already shipped.

Windows native `FILEVERSION` and Inno Setup version components are limited to
`65535`. Packaging currently puts the full build number in the fourth component.
Before exceeding that limit, change and test the Windows metadata representation
while preserving the complete application/update version and monotonic Android
`versionCode`. With the current formula, run 56 exceeds the Windows limit even
on its first attempt; many retries can reach it sooner. The 0.3.2 packages are
below this limit. Do not reset the GitHub run sequence to work around it.

## Signing and credentials

- Repository variables `ANDROID_HOME` and `JAVA_HOME` point to the existing
  SDK and JDK on the Mac runner when they are outside standard locations.
- No GitHub repository secret is required for compilation. The short-lived
  workflow token fetches source and uploads CI artifacts.
- The Mac runner must retain the existing Android preview
  `$ANDROID_USER_HOME/debug.keystore` (default `$HOME/.android/debug.keystore`).
  `build-android.sh` refuses to silently replace it. A replacement key prevents
  upgrades over an installed APK.
- Optional repository variable `CC_CODESIGN_IDENTITY` selects an existing
  macOS keychain signing identity by name or SHA-1. Without it, builds retain
  the previous ad-hoc behavior. Developer ID distribution needs notarization.
- iOS output is a Simulator preview; no provisioning credentials are requested.
- The update signing key remains confined to the separate update-feed workflow.
  Native build jobs do not receive production credentials or update signing keys.
  See [RELEASING.md](RELEASING.md).

Artifacts are retained for 30 days and include only explicit package, checksum
and version paths. The Linux test report uses a redacted, closed JSON schema;
test keyrings, raw runtime output and private fixtures are not uploaded.

## Generated-output cleanup

For releases containing the cleanup workflow changes, each native job runs
`cleanup-ci-workspace.py` as its final `always()` step after the artifact upload
step. It also runs when compilation or upload fails. The local `dist` copy is
removed after that attempt; completed uploads and GitHub job logs are retained.
An abruptly terminated runner/host cannot guarantee that its final step runs;
use the maintenance workflow below after the runner is idle.

Cleanup is restricted to the exact `GITHUB_WORKSPACE` of this repository and
checks Git root/origin and tracked files before deleting anything. Its closed
list covers Cargo `target` directories, Flutter `build`/`.dart_tool`, Android
checkout `.gradle`/`.cxx`, and `dist`. A symlink/junction in a target's ancestors
is rejected; links inside generated trees are removed without following them.
Source, `.local` receipts, application models/vault, signing keys, SDKs, Cargo
registry, pub cache and shared tool installations are outside this list.

New Windows GitHub jobs put AI smoke downloads in
`RUNNER_TEMP/consolecrypt-client-ai-smoke/<run>-<attempt>-<job>`, with an exact
ownership marker. Only that job's directory is removed by its final cleanup.
The old `C:\GitLab-Runner\tools\consolecrypt-local-ai-smoke` cache is preserved
unless the operator explicitly opts into maintenance for that known CI path.
Linux stops/removes its own uniquely named container from the EXIT/INT/TERM
cleanup path and removes its temporary source snapshot; the builder image and
shared Docker layers are retained. There is no global Docker prune or volume
deletion.

`Runner generated-output maintenance` is a separate, **main-only** manual
workflow. It does not build packages, deploy, receive signing secrets or change
release tags. It uses the same concurrency group as release builds, so it waits
until the release has finished. Its tools checkout is nested separately with
`clean: false`. Inline guards run **before** checkout: they reject symlink/junction
ancestors, a foreign workspace and an existing tools directory unless it is
empty or already the expected clean Git checkout (including untracked/ignored
files). Checkout must not erase
unknown files before the guarded cleanup runs.

```sh
gh workflow run runner-cleanup.yml --ref main -f platform=macos
gh workflow run runner-cleanup.yml --ref main -f platform=windows

# Explicit one-time cleanup of known old Windows CI GGUF/download/runtime files.
gh workflow run runner-cleanup.yml --ref main \
  -f platform=windows -f legacy_windows_ai_cache=true
```

The legacy-cache option defaults to false, accepts no arbitrary path, rejects
links/reparse points and unexpected runtime contents, and preserves selection,
logs, receipts and unknown files. It does not inspect Windows application data.
Deletion boundaries are tested with tiny owned temporary repositories by
`python3 -m unittest discover -s client/scripts -p 'test_ci_cleanup.py' -v`.
The Windows junction test runs on Windows and is explicitly skipped elsewhere.

## Русский

`main` хранит проверенную исходную базу. Для выпуска поднимите версию, добавьте
release notes, отправьте коммит в `main`, затем тег `vX.Y.Z` на этот коммит.
Только тег запускает сборки. После успешной сборки всех платформ workflow
публикует полный GitHub Release и запускает обновление подписанной ленты.
Пакеты Android подписываются прежним ключом раннера. iOS — только Simulator preview.


### Диагностический тег Windows

Для изолированной проверки RDP/ConPTY до нового стабильного выпуска создаётся
неизменяемый тег `check-windows-0.5.2-1` (следующие попытки — новый номер).
`windows-native-check.yml` на hosted runner проверяет формат тега и принадлежность
коммита `main`, затем запускает `ci-windows.ps1 -NativeOnly` на Windows.
Этот режим компилирует и выполняет только нативные тесты; он не резервирует
номер установщика, не собирает приложение, не скачивает модели и не публикует
релиз. Финальный `always()` удаляет generated outputs даже после ошибки.
Обычный релиз по `vX.Y.Z` повторяет полный набор проверок и упаковку всех платформ.
