# ConsoleCrypt 0.1.22 · GNU AGPL-3.0-only

Дата: 1 октября 2026 года. Проверенный исходный код: тег `v0.1.22`,
commit `38cbfe1a166227488f251479b911809206ef3402`.

Клиент, сервер, общие библиотеки и документация ConsoleCrypt теперь используют
**GNU AGPL-3.0-only**. README, сведения «О программе», Cargo metadata,
нативные bridge specifications и текст лицензии в дистрибутивах согласованы.
Полный текст AGPL включён в macOS, Windows, Android и iOS Simulator.
Сторонние компоненты сохраняют свои лицензии. Ранее опубликованные выпуски
и предоставленные для них лицензии не изменяются. [Подробности](LICENSING.md).

| Платформа | Пакет | Проверенная версия |
|---|---|---|
| Windows 10/11 x64 | EXE и переносимый ZIP | 0.1.22+1244 |
| macOS Apple Silicon и Intel | Универсальный DMG | 0.1.22+1245 |
| Android 11+ ARM64 | APK | 0.1.22+55 |
| iOS Simulator на Mac | Универсальный ZIP в двух частях | 0.1.22+56 |

## Проверки

- Flutter: анализ без ошибок и **462 теста PASS**. Windows/macOS
  [pipeline185](https://git.evsikov.net/publics/consolecrypt/-/pipelines/185)
  завершился успешно до упаковки.
- Общие Rust crates: **61 тест PASS**. Cargo-deny licenses прошёл для
  клиентского и общего workspace; исключения AGPL ограничены нашими crates.
- Сценарии версии, экспорта и упаковки: **20 PASS, 1 SKIP**. Единственная
  пропущенная локальная проверка требует PowerShell; настоящий Windows job
  1244 успешно собрал установщик и переносимый архив.
- Windows: версия приложения и установщика 0.1.22.1244, комплект DLL,
  целостность ZIP и полный текст AGPL проверены.
- macOS: версия 0.1.22+1245, целостность DMG, universal architectures,
  deep/strict codesign, ссылка Applications и полный текст AGPL проверены.
- Android: версия 0.1.22+55, прежний сертификат APK, zipalign16KB,
  native ELF alignment и AGPL asset проверены.
- iOS Simulator: версия 0.1.22+56, ARM64/Intel slices Runner/App,
  AGPL asset, CRC и SHA-256 объединённых частей ZIP проверены.
- Все семь опубликованных файлов анонимно скачаны; размеры и SHA-256
  совпадают с локальными проверенными пакетами. Ограниченный CI188/job1248
  зарегистрировал asset links без новых постоянных credentials.

## Скачать

[Релиз и контрольные суммы](https://git.evsikov.net/publics/consolecrypt/-/releases/v0.1.22).
Для iOS Simulator скачайте обе части и объедините по [инструкции](BUILD_IOS.md).
Это ZIP для Simulator, не установочный IPA для телефона.

Сайт обновлён независимо от API: версия сайта0.1.18, код84563e1,
готовые установщики доступны на [странице загрузки](https://consolecrypt.evsikov.net/download).
Проверены81публичный маршрут/ресурс, включая54снимка;26Node-тестов
прошли, одна необязательная проверка реального аккаунта пропущена.
Сервер обновлений публикует0.1.22 для Windows1244, macOS1245 и Android55.
Подпись Ed25519 проверена прежним публичным trust anchor клиента; размеры,
SHA-256 и ссылки соответствуют проверенным установщикам.

Поведение SSH, синхронизации и общего доступа в этом выпуске не менялось.
Установка и интерактивная приёмка на физических Windows/Android/iPhone
устройствах в этом прогоне не выполнялись. Нативные проверки прошлого
выпуска описаны в [0.1.21](RELEASE_0_1_21.md).

English: ConsoleCrypt's current first-party client, server, shared libraries
and documentation use GNU AGPL-3.0-only. Third-party licences and historical
licence grants remain unchanged. All462 Flutter tests and61 shared Rust
tests passed. The installer versions and complete AGPL licence text were
verified, and all seven public release files were downloaded and checked
against SHA-256. Physical-device installation and interactive acceptance
were not performed in this release verification.
