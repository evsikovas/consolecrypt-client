# ConsoleCrypt 0.1.21 · установщики и руководство

Дата: 1 октября 2026 года. Проверенный код: тег `v0.1.21`,
commit `f9d9094e44d5118d4d8938b68eac55bb5ea7228b`.
Все пакеты используют настоящее Rust-ядро, `CC_MOCK=false`.

| Платформа | Пакет | Версия внутри пакета |
|---|---|---|
| Windows 10/11 x64 | EXE, переносимый ZIP | 0.1.21+1237 |
| macOS Apple Silicon и Intel | Универсальный DMG | 0.1.21+1238 |
| Android 11+ ARM64 | APK | 0.1.21+51 |
| iOS Simulator на Mac | ZIP в двух частях | 0.1.21+52 |

Выпуск обновляет установщики после завершения проверки совместного доступа и
iOS. Поведение приложения соответствует проверенному 0.1.20; новые номера
сборок присутствуют в About и нативных метаданных. Сайт получил
[руководство на русском и английском со снимками интерфейса](https://consolecrypt.evsikov.net/guide).
Оно объясняет первый запуск, хранилище, подключение устройств, SSH/SFTP,
сниппеты, ИИ, группы, туннели, общий доступ, резервные копии и обновления.
Руководство содержит26глав и54 снимка; сайт прошёл19 автоматических
проверок, одну необязательную live API проверку пропустили. Развёртывание
сайта отдельно от API завершено; проверены83 публичных routes/assets,
переключение языков, поиск, отображение на телефоне и неизменность API.
Изображения показывают реальные виджеты с демонстрационными данными и скрытыми
секретами; это не снимки пользовательского хранилища.

## Проверки

- GitLab pipeline164: Windows job1237 и macOS job1238 успешно выполнили
  **461 Flutter-тест** и **6 проверок версии** каждый, затем собрали пакеты.
- macOS: DMG integrity и deep/strict codesign проходят; Runner, Flutter,
  App и Rust frameworks содержат ARM64 и Intel slices; ссылка Applications
  присутствует. Windows: CRC переносимого ZIP, настоящее Rust DLL и
  нативная версия EXE проверены.
- Android: versionName0.1.21/code51, подпись совпадает с прежними APK,
  zipalign16КБ и ELF mapping/RELRO всех трёх native libraries проходят.
- iOS52: CRC, версия, SHA-256 и ARM64/Intel Simulator slices проверены;
  пакет установлен и запущен в отдельном Simulator. После проверки временный
  Simulator удалён, пользовательский сохранён.
- Основной Rust и серверный код не менялись: проверки 0.1.20 остаются
  применимы — 634 Rust, 61 shared, 12 real-backend sharing, 166 real-PG server
  и две iOS native regression проверки. [Подробная приёмка](RELEASE_0_1_20.md).
- Локальная упаковка Android теперь атомарно заменяет latest APK; два
  regression-теста проверяют сохранение hardlinked старого релиза и
  сохранение прежнего latest при сбое копирования.

## Установка и границы проверки

[Релиз, установщики и SHA256SUMS](https://git.evsikov.net/publics/consolecrypt/-/releases/v0.1.21).
Сервер обновлений уже публикует подписанный feed0.1.21: Windows1237, macOS1238
и Android51. Подпись Ed25519 проверена публичным trust anchor клиента;
размеры и SHA-256 совпадают с анонимно скачанными файлами релиза.
Для iOS скачайте обе части ZIP и объедините их по [инструкции](BUILD_IOS.md).
Это сборка для Simulator, а не установочный IPA для телефона. Face ID,
экранная клавиатура, сеть и background на настоящем iPhone требуют
подключённого устройства и подписи через Apple Account/Personal Team.

Windows EXE и Android APK в этом прогоне не устанавливались поверх
пользовательских приложений. Проверка на проблемных Windows10 компьютерах и
физическом Android остаётся отдельной приёмкой. Пакеты предварительные:
Windows без Authenticode, macOS без Apple notarization, Android с прежней
development-подписью. Перед обновлением сохраните зашифрованную резервную
копию и комплект восстановления.

Возможности совместной работы включаются оператором сервера отдельно от
публикации клиента. Личный VRK не передаётся; общий доступ выдаётся выбранным
объектам и проверенным устройствам. Отзыв не удаляет ранее прочитанные копии
и не меняет SSH credentials на целевом хосте. [Руководство](SHARING.md).

English: verified Windows, macOS and Android installers plus an iOS Simulator
preview. The bilingual illustrated guide covers the complete workflow.
Windows/macOS CI each passed all461 Flutter tests before packaging. iOS52
was installed and started in Simulator; physical iPhone signing and acceptance
remain separate. Sharing availability is controlled by the selected server.
