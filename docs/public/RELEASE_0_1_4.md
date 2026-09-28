# ConsoleCrypt 0.1.4 — проверка macOS и Android

Дата: 2026-09-28. Windows 0.1.4+13 уже опубликован другим агентом;
его EXE, ZIP и тег `v0.1.4` не изменялись. См. [Windows validation](WINDOWS_VALIDATION.md).

## Исходники и сборки

База клиента — `e20d11c9d2ad17d22f3304bd3b5932cd951e565a` (`v0.1.4`).
Она интегрирована с сайтом в commit `86b8025`; клиент при сборке изменён
только синхронными номерами в pubspec.yaml/app_info.dart. Сначала выполнен
`bash client/scripts/build-macos.sh --no-cli` → **0.1.4+14**, затем
`bash client/scripts/build-android.sh` → **0.1.4+15**. Счётчик не сбрасывался.

- Flutter 3.47.5 / Dart 3.13.4, Rust 1.98.0, Xcode 27.
- Android SDK 36, NDK 28.2.13676358, Java 21.
- macOS universal: arm64 + x86_64, настоящий cc_bridge.framework.
- Android: min API 30 (Android 11), ARM64, настоящий libcc_bridge.so, CC_MOCK=false.

## Выполнено

- `flutter analyze`: без замечаний; `flutter test`: **391 passed** на macOS.
- Python проверки упаковки: **18 tests, OK (1 skipped)**.
- DMG: встроенные 0.1.4/14; universal executable и Rust framework;
  `codesign --verify --deep --strict`, `hdiutil verify`; read-only mount,
  приложение и Applications symlink присутствуют.
- APK: встроенные 0.1.4/15, ARM64/API30; `apksigner verify`,
  `zipalign -P 16` и 16 KiB ELF layout для всех трёх native libraries.
  SHA-256 сертификата совпадает с предыдущим APK 0.1.2+7.
- Публичные скачивания сверены с SHA-256 локальных DMG/APK; существующие
  Windows-файлы также сверены с отчётом Windows-агента.

## Границы проверки

macOS ad-hoc signed, без Apple notarization; возможен новый запрос Keychain после
обновления. Android подписан прежним тестовым ключом; Windows без Authenticode.
Новые native приложения на Mac/телефоне не запускались; SSH, AI, синхронизация
на этих двух новых пакетах вручную не проверялись. Проверки Windows описаны
отдельно и здесь не выдаются за повторно выполненные на Mac.

## SHA-256

```
f309b5e447cf54c5948b486678cb64d0959d9d36acbb2a50961f254abbc9de95  ConsoleCrypt-0.1.4+13-windows-x64-setup.exe
96f461f32576e3e9da183c25cfdccdb756c75168783887dff88daae3b87ef515  ConsoleCrypt-0.1.4+13-windows-x64-portable.zip
4c81f987c009a8c6986f4529fded817e06074e9ff79ca98b76539dd36ee2f108  ConsoleCrypt-0.1.4+14-macos-universal.dmg
effc8f17896fb85ad8cfdd6066d8afeb28b97313f3e533c69a3c26e9c1a08516  ConsoleCrypt-0.1.4+15-android-arm64.apk
```
