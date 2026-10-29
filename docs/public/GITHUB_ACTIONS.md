# GitHub Actions: native client builds

The `Client tag release` workflow runs only when an exact stable `vX.Y.Z`
tag is pushed. The version must match `pubspec.yaml`, and the tagged commit
must already belong to `main`. Branch pushes and manual dispatch do not build
packages. No pull request code executes on persistent native runners.

| Runner labels | Jobs | Required tools |
|---|---|---|
| `self-hosted, macOS, ARM64, consolecrypt, client` | Android; macOS unless `MACOS_BUILD_RUNNER` is set | Flutter 3.47.5, Rust 1.98, Python; Android SDK 36, build-tools 36.0.0, NDK 28.2.13676358, CMake 3.22.1; full Xcode and CocoaPods for Apple builds |
| `self-hosted, Windows, X64, consolecrypt, client` | Windows x64 | MSVC C++/Windows SDK, Git, Python, Rust; Flutter, Strawberry Perl and Inno Setup are discovered or provisioned by `ci-windows.ps1` |
| `cc-linux-k3s` | Linux DEB/RPM | Home ARC scale set with Docker-in-Docker, Python 3 and a shared work/temp volume; pinned Ubuntu 22.04 builder contains native tools and isolated Secret Service tests |

Repository variable `MACOS_BUILD_RUNNER` can override only the macOS runner
with a JSON array, for example `["macos-26"]` for a hosted runner with full Xcode
(set this way since October 2026). Remove the override after installing Xcode on
the dedicated Mac. Android always uses the dedicated Mac and its existing signing
key. `ci-macos-tools.sh` provisions the pinned Flutter SDK when it is absent.
macOS, Android, Windows and Linux build independently. A single self-hosted Mac
serializes its jobs. The tag workflow builds no iOS package: iOS builds and
testing are deferred, and the last iOS artifact is the 0.5.6 Simulator ZIP
(`client/scripts/build-ios.sh` still builds one locally).

The ARC runner and Docker daemon must mount `RUNNER_TEMP` at the same absolute
path. `ci-linux.sh` stages the source snapshot there, not in an unshared `/tmp`.
The home cluster is a build environment; these jobs do not deploy production there.

Native jobs have `contents: read`. After all four platforms succeed, a hosted
publisher validates the version receipts, assembles a closed list of six
installers (macOS DMG, Windows EXE and portable ZIP, Linux DEB and RPM, Android
APK) plus SHA-256 checksums, creates a draft GitHub Release, uploads its
files, and publishes it. Only that job gets `contents: write` and `actions: write`.
It then dispatches `update-feeds.yml` on trusted `main` to refresh signed metadata.
Website and sync-server deployment use their own tag workflows.

Dependency checks run only on GitHub-hosted `ubuntu-24.04`. The release
`validate` job runs `client/scripts/dependency-audit.py` (pinned `cargo-deny`
and `osv-scanner`, SHA-256 verified) before any native job starts.
`dependency-audit.yml` repeats it weekly, on manual dispatch and on pull requests
that touch manifests or lockfiles, including Dependabot's. Its audit job has
`contents: read`; a separate job with `issues: write` (never on pull requests)
opens, updates or closes the "Уязвимые зависимости" issue. Policy and exceptions:
[BUILDING.md](BUILDING.md#проверка-зависимостей).

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
- No iOS job runs; a local iOS build is a Simulator preview and requests no
  provisioning credentials.
- The update signing keys (`UPDATE_SIGNING_KEY`, and `LINUX_UPDATE_SIGNING_KEY`
  for the separate Linux feed) remain confined to the separate update-feed
  workflow. Native build jobs do not receive production credentials or update
  signing keys. See [RELEASING.md](RELEASING.md) and
  [UPDATE_FEED_PUBLISHER.md](UPDATE_FEED_PUBLISHER.md).
- The hosted `announce` job runs after publication. Each channel starts only
  when the owner has configured it: `TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID`
  (project channel), the variable `TELEGRAM_PERSONAL_CHAT_ID` (a second channel;
  `announce-telegram.py` keeps its journal per channel), and `EVSIKOVNET_TOKEN`
  (`announce-evsikovnet.py` creates or updates the release on evsikov.net with the
  Russian notes and consolecrypt.dev download links). The scripts refuse to post
  outside this release job; locally use `--dry-run`.

Artifacts are retained for 30 days and include only explicit package, checksum
and version paths. The Linux test report uses a redacted, closed JSON schema;
test keyrings, raw runtime output and private fixtures are not uploaded.

## Generated-output cleanup

Full generated-output cleanup runs **only after a stable GitHub Release has
been published**. Persistent Windows and Mac build jobs use `clean: false` for
checkout and retain ignored Cargo/Flutter outputs, local packages and the owned
AI smoke cache after compilation, test or upload failures. Diagnostic jobs also
retain them. The workflow records the repository, source commit, run and platform
in a workspace ownership marker before building.

After publication, a separate Mac cleanup job verifies that marker and
the checkout's current `HEAD`. The persistent Windows runner keeps its
generated build cache after publication so that the next release builds
incrementally; use the guarded maintenance workflow below to clean it. They perform no checkout and refuse a foreign or
newer workspace. The release, diagnostic and maintenance workflows share one
concurrency group, so another build cannot begin while post-publication cleanup
is running. Publication is recorded only after GitHub has accepted the public
release; a later update-feed dispatch failure does not prevent cleanup of that
already published release. Completed artifact uploads and GitHub logs remain.

This policy applies to tags containing these workflow changes. The immutable
`v0.5.2` workflow still contains the earlier per-job `always()` cleanup. Hosted
Mac virtual machines and Linux ARC `emptyDir` workspaces are ephemeral; retaining
files in a job does not preserve them after that machine or pod is destroyed.
An abruptly terminated persistent runner can also miss its final cleanup job;
use the guarded maintenance workflow below after publication.

Cleanup is restricted to the exact `GITHUB_WORKSPACE` of this repository and
checks Git root/origin and tracked files before deleting anything. Its closed
list covers Cargo `target` directories, Flutter `build`/`.dart_tool`, Android
checkout `.gradle`/`.cxx`, and `dist`. A symlink/junction in a target's ancestors
is rejected; links inside generated trees are removed without following them.
Source, `.local` receipts, application models/vault, signing keys, SDKs, Cargo
registry, pub cache and shared tool installations are outside this list.
If a finishing build removes or creates a file during traversal, cleanup retries
only `ENOENT`/`ENOTEMPTY` up to five times (3.85 seconds of total backoff).
Persistent failures remain failures; diagnostics include the error type and errno,
without exception text, absolute paths or environment values.

Windows GitHub jobs reuse `.ci-cache/local-ai-smoke` inside the guarded checkout.
Its ownership marker binds it to this repository and workspace, independently of
the attempt number, so a retry can reuse verified downloads. Post-publication
cleanup removes this owned cache together with generated build outputs. The old
`C:\GitLab-Runner\tools\consolecrypt-local-ai-smoke` cache is preserved unless
an operator explicitly opts into maintenance for that known CI path.

Linux records its job ownership under `RUNNER_TEMP` before starting Docker and
labels the container with repository/run/attempt/job. Both the normal exit trap
and a separate `always()` step call `cleanup-linux-job.py` to stop compute even
when a build fails or is cancelled. The helper checks the ownership receipt,
labels, image and exact `/work` source mount, then removes only the inspected
immutable container ID. Missing/mismatched ownership or linked paths fail closed.
The helper retains the owned source scratch and a safe receipt; ARC eventually
removes its shared `emptyDir` with the pod. The builder image and shared Docker
layers are retained. There is no global Docker prune or volume deletion.

`Runner generated-output maintenance` is a separate, **main-only** manual
workflow. It does not build packages, deploy, receive signing secrets or change
release tags. It uses the same concurrency group as release builds, so it waits
until the release has finished. It requires a published stable tag, verifies
that its GitHub Release is public, and refuses cleanup if the workspace's current
`HEAD` differs from that tag. An older published release therefore cannot be used
to erase a newer failed attempt. Its tools checkout is nested separately with
`clean: false`. Inline guards run **before** checkout: they reject symlink/junction
ancestors, a foreign workspace and an existing tools directory unless it is
empty or already the expected clean Git checkout (including untracked/ignored
files). Checkout must not erase
unknown files before the guarded cleanup runs.

```sh
gh workflow run runner-cleanup.yml --ref main -f platform=macos -f published_tag=v0.5.2
gh workflow run runner-cleanup.yml --ref main -f platform=windows -f published_tag=v0.5.2

# Explicit one-time cleanup of known old Windows CI GGUF/download/runtime files.
gh workflow run runner-cleanup.yml --ref main \
  -f platform=windows -f published_tag=v0.5.2 -f legacy_windows_ai_cache=true
```

The legacy-cache option defaults to false, accepts no arbitrary path, rejects
links/reparse points and unexpected runtime contents, and preserves selection,
logs, receipts and unknown files. It does not inspect Windows application data.
Deletion boundaries are tested with tiny owned temporary repositories by
`python3 -m unittest discover -s client/scripts -p 'test_ci_cleanup.py' -v`.
The Windows junction test runs on Windows and is explicitly skipped elsewhere.
`test_cleanup_policy.py` also verifies that a failed release creation/publication
does not emit the publication signal that enables full cleanup.
`test_linux_cleanup.py` covers Docker ownership boundaries; `tests/test_linux_ci.py`
also kills a synthetic producer shell with `SIGKILL` and invokes final cleanup as
a new process, without relying on its exit trap. These use a fake Docker, not
live user containers.

## Русский

`main` хранит проверенную исходную базу. Для выпуска поднимите версию, добавьте
release notes, отправьте коммит в `main`, затем тег `vX.Y.Z` на этот коммит.
Только тег запускает сборки. После успешной сборки всех платформ workflow
публикует полный GitHub Release и запускает обновление подписанной ленты.
Пакеты Android подписываются прежним ключом раннера. iOS в CI не собирается
(сборки отложены; локально — только Simulator preview).

После публикации задание `announce` объявляет выпуск в каналах, которые
настроил владелец: Telegram-канал проекта (`TELEGRAM_BOT_TOKEN`,
`TELEGRAM_CHAT_ID`), второй канал (переменная `TELEGRAM_PERSONAL_CHAT_ID`; журнал
`announce-telegram.py` ведётся по каналам) и evsikov.net (`EVSIKOVNET_TOKEN`:
`announce-evsikovnet.py` создаёт или обновляет релиз с русскими заметками и
ссылками на consolecrypt.dev). Вне этого задания скрипты не публикуют; локально —
`--dry-run`.


### Диагностический тег Windows

Для изолированной проверки ConPTY до нового стабильного выпуска создаётся
неизменяемый тег `check-windows-0.5.2-1` (следующие попытки — новый номер).
`windows-native-check.yml` на hosted runner проверяет формат тега и принадлежность
коммита `main`, затем запускает `ci-windows.ps1 -NativeOnly` на Windows.
Этот режим компилирует и выполняет только нативные тесты; он не резервирует
номер установщика, не собирает приложение, не скачивает модели и не публикует
релиз. Промежуточные результаты сохраняются и после успеха, и после ошибки;
полная очистка выполняется только после публикации стабильного выпуска.
Диагностические и релизные задания используют общую очередь, чтобы очистка
опубликованного выпуска не пересекалась с новой диагностической сборкой.
Обычный релиз по `vX.Y.Z` повторяет полный набор проверок и упаковку всех платформ.

### Предварительная сборка Windows

`windows-preview.yml` запускается вручную (Actions → «Windows preview build» →
Run workflow, ветка на выбор) на облачном `windows-2025`. Он вызывает
`ci-windows.ps1 -PackageOnly`: ставит инструменты и собирает установщик и
портативный ZIP без тестового прогона релиза. Результат — артефакт
`windows-preview` на 14 дней; релиз, ленты обновлений и номер сборки не
затрагиваются. Это сборка для ручной проверки, а не выпуск.
