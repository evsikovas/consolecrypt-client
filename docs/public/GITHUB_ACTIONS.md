# GitHub Actions: native client builds

The `Client packages` workflow runs on pushes to `main`, the exact transition
branch `codex/github-release-0.3.2`, and manual dispatches of either branch.
It does not run untrusted pull requests on persistent self-hosted machines.
Checkout credentials are removed before repository build scripts execute.
The build token has `contents: read`; no release, website or update feed is
published by this workflow.

| Runner labels | Jobs | Required tools |
|---|---|---|
| `self-hosted, macOS, ARM64, consolecrypt, client` | macOS, then Android, then iOS | Flutter 3.47.5, Rust 1.98, Xcode/iOS Simulator, CocoaPods, Python; Android SDK 36, build-tools 36.0.0, NDK 28.2.13676358, CMake 3.22.1 |
| `self-hosted, Windows, X64, consolecrypt, client` | Windows x64 | MSVC C++/Windows SDK, Git, Python, Rust; Flutter, Strawberry Perl and Inno Setup are discovered or provisioned by `ci-windows.ps1` |
| `self-hosted, Linux, X64, consolecrypt, client, linux-docker` | Linux DEB/RPM | Docker CLI/daemon, Python 3, x86-64 host; pinned Ubuntu 22.04 builder contains native tools and isolated Secret Service tests |

The three Mac jobs are explicitly ordered because they share native SDKs and
one runner. Linux and Windows build in parallel. A Linux runner inside Docker
must bind its work/temp directory at the same absolute path inside and outside
its container: `ci-linux.sh` starts a sibling builder with a source snapshot.
Use a dedicated builder, not the production server.

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
- Update-feed signing and production SSH/Timeweb credentials do not belong in
  this build workflow. See [RELEASING.md](RELEASING.md) for publication.

Artifacts are retained for 30 days and include only explicit package, checksum
and version paths. The Linux test report uses a redacted, closed JSON schema;
test keyrings, raw runtime output and private fixtures are not uploaded.

## Русский

Запуск: **Actions → Client packages → Run workflow**, ветка `main` или точная
переходная ветка выше. Успешная сборка создаёт CI artifacts, без публикации на
сайте и объявления автообновления. Пакеты, подписи и номера сборок проверяются
перед отдельным выпуском. Приватные материалы остаются на соответствующих
раннерах; обычной сборке не нужны доступы к продакшену.
