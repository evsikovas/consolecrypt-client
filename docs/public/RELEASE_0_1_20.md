# ConsoleCrypt 0.1.20 · проверка выпуска

Дата: 1 октября 2026 года. Код — тег `v0.1.20`,
commit `9cc4baaca4a6b451c23d835823a35300a0c77e54`. Все пакеты используют
настоящее Rust-ядро; `CC_MOCK=false`.

| Платформа | Пакет | Нативная версия |
|---|---|---|
| Windows 10/11 x64 | EXE и переносимый ZIP | 0.1.20+1231 |
| macOS Apple Silicon и Intel | Универсальный DMG | 0.1.20+1232 |
| Android 11+ ARM64 | APK | 0.1.20+49 |
| iOS Simulator на Mac | ZIP, две части для GitLab | 0.1.20+48 |

## Что изменилось

Общий доступ к выбранным хостам, сниппетам, коллекциям и отдельно
подтверждённым секретам. Личный VRK не передаётся. Устройства сверяют полный
код; для каждого объекта используются свои DEK и конверты участников.
Доступны Reader/Editor, отзыв с ротацией, очередь зашифрованных изменений,
проверка signed history и восстановление после прерванных операций.
Дополнительные собственные устройства допускаются через owner-online
разрешения с потолком роли, сроком и квотой; ручной режим используется
по умолчанию. [Процесс и границы защиты](SHARING.md).

Исправлена доставка окончательного состояния SSH и закрытие idle
FRB stream: закрывается нативный producer, затем старая подписка.
Поздние события старого соединения не завершают новое.

Добавлены iOS runner, статическое Rust-ядро, Keychain/LocalAuthentication,
мобильные guards и независимые задания regression/preview.

## Проверки

- Клиент Rust: **634 PASS**, 7 исключённых тестов внешних fixtures;
  весь workspace clippy проходит. Общие crates: **61 PASS**.
- Flutter: **461 PASS**, анализ без замечаний. GitLab pipeline160 также
  выполнил все 461 теста на Windows и macOS перед сборкой; jobs1231/1232
  завершены успешно. Это не проверка физической клавиатуры всех Windows10 ПК.
- Реальная app-core sharing интеграция: **12 PASS, 0 ignored**, PostgreSQL,
  SQLCipher, FileMailer, штатная регистрация/верификация и device approval.
  Проверены четыре вида объектов, роли, отзыв, новый DEK, ручной/автоматический
  допуск, потерянные ответы, повтор, restart и rollback/freeze.
- Сервер: **166 real-PG проверок**, в том числе dump/restore, CAS, RBAC
  и подписи. Сервер 0.1.10 выпускается отдельно, четыре возможности общего
  доступа включаются оператором; клиент проверяет возможности выбранного сервера.
- Windows ZIP: CRC, resource version0.1.20+1231, наличие настоящего
  `cc_bridge.dll`, совпадение SHA-256 с артефактами job1231.
- macOS DMG: целостность, `codesign --verify --deep --strict`, версия1232,
  ARM64/x86_64 у приложения и всех frameworks, включая Rust bridge;
  внутри есть ссылка Applications.
- Android APK: подпись, `zipalign -P 16`, ELF mapping/RELRO всех трёх native
  библиотек. Сертификат совпадает с0.1.13; версия0.1.20/code49.
- iOS native regression47: **2 PASS**. Keychain/SQLCipher, TextInput
  кириллица/Return, выделение/копирование/вставка, две вкладки, lifecycle,
  Unicode SFTP, disconnect/reconnect, сохранение буфера и закрытие активной
  вкладки ≤5 секунд; cleanup без таймаутов. Отдельный продукт48 установлен
  и запущен в Simulator, версия/ZIP/SHA-256/ARM64+Intel проверены.
- Чистый публичный экспорт: **982 файла**, без приватной истории,
  секретов, live deployment overrides, agent Markdown и сборок.
  Обязательные пути сборки и локальные ссылки документации присутствуют.

## Публикация

GitLab-релиз содержит семь ссылок на файлы: DMG, EXE, Windows ZIP,
две части iOS Simulator и APK, а также общий SHA256SUMS. Они доступны без входа.
[Сайт](https://consolecrypt.evsikov.net) показывает новые пакеты на русском
и английском; личный кабинет и API обслуживаются отдельно от лендинга.

Подписанный канал обновлений выдаёт 0.1.20 для Windows 1231, macOS 1232
и Android 49. Подпись Ed25519 проверена ключом клиента; размеры и SHA-256
совпадают с опубликованными установщиками. Серверные возможности совместной
работы включаются оператором независимо от публикации клиента.

## Границы приёмки

iOS ZIP работает в Simulator на Mac. Он не является IPA для телефона.
Face ID, физическая клавиатура, Wi-Fi/сотовая сеть и background на настоящем
iPhone требуют подключённого устройства и подписи Personal Team/Apple Account.
Тест инжектирует Flutter TextInput/lifecycle, а не реальные события ОС телефона.
[Установка частей архива и дальнейшая проверка](BUILD_IOS.md).

Новые EXE/APK не устанавливались поверх пользовательских приложений в этом
прогоне. Приёмка на проблемных Windows10 ПК и физическом Android остаётся
отдельной проверкой. Пакеты предварительные: Windows без Authenticode, macOS
без Apple notarization, Android с прежней development-подписью.

Отзыв защищает новые данные, но не удаляет прочитанные копии и не меняет
SSH credentials на целевом сервере. Перед обновлением сохраните
зашифрованную резервную копию и комплект восстановления.

[Релиз и SHA-256](https://git.evsikov.net/publics/consolecrypt/-/releases/v0.1.20)
· [Обновления](UPDATES.md) · [Свой сервер](HOSTING.md).

English: this release adds selective end-to-end encrypted sharing, verified
Reader/Editor devices and explicit owner-online device enrollment. The client
passed 634 Rust, 461 Flutter and 12 real-backend sharing checks. All three
desktop/mobile installers were built and checked; the iOS48 preview passed
installation/startup in Simulator after native47 regression. Physical iPhone
acceptance and signing remain separate; the ZIP does not install on a phone.
