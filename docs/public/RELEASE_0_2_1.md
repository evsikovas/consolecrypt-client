# ConsoleCrypt 0.2.1

Большой поток вывода терминала теперь обрабатывается порциями, чтобы интерфейс чаще возвращался к вводу и отрисовке. Исправлены ошибки обработки повреждённых ANSI-цветов; статические слои стеклянного интерфейса меньше перерисовываются.

[Скачать клиент](https://consolecrypt.evsikov.net/download?lang=ru) · [Руководство](https://consolecrypt.evsikov.net/guide?lang=ru) · [Своя установка сервера](https://git.evsikov.net/publics/consolecrypt/-/blob/v0.2.1/docs/public/HOSTING.md)

## Что изменилось

- **Терминал:** постепенное декодирование UTF-8, ограниченные порции обработки вывода, безопасная обработка длинных управляющих последовательностей и неверных цветов. Поддержка Unicode и выделения сохранена.
- **Интерфейс:** повторное использование фоновых фильтров и изоляция статических поверхностей от перерисовки содержимого. Более равномерный фон благодаря текстуре шума.
- **Секреты и ИИ:** запоздалые ответы и секреты не переходят в другой профиль; блокировка очищает состояние диалогов. Копирование секрета на Android и iOS требует защищённого нативного канала с TTL. При его недоступности копирование отклоняется.
- **SSH и SFTP:** остановка SSH-агента отзывает доступ к ключам, включая ожидающее подтверждение подписи. Закрытие SFTP менеджера отменяет ожидающий выбор редактора и очищает рабочие копии. Усилена проверка аргументов jump-хостов.
- **Сетевые ответы ИИ:** ограничены размеры ответов и потоковых событий, отмена не ждёт освобождения заполненной очереди интерфейса.
- **Публичный сервер:** доступны общий доступ, группы, секреты и подтверждённое добавление устройств. Передача происходит по явному действию владельца; данные хранилища остаются зашифрованными на сервере.

## Сборки

| Платформа | Сборка | Файл |
|---|---|---|
| Windows x64 | 0.2.1+1280 | установщик EXE или portable ZIP |
| macOS Intel / Apple Silicon | 0.2.1+1281 | universal DMG |
| Android ARM64 | 0.2.1+61 | APK |
| iOS Simulator Intel / Apple Silicon | 0.2.1+62 | ZIP, две части `.001` / `.002` |

Все файлы доступны ниже в **Assets**. `SHA256SUMS-0.2.1.txt` содержит контрольные суммы каждого файла и собранного iOS ZIP. Windows и macOS собраны в [CI №197](https://git.evsikov.net/publics/consolecrypt/-/pipelines/197) из коммита `e37af672`; Android и iOS — из тех же исходников с отдельными номерами сборки.

Для iOS Simulator скачайте обе части в одну папку на Mac:

```sh
cat ConsoleCrypt-0.2.1+62-ios-simulator-universal.zip.001 \
    ConsoleCrypt-0.2.1+62-ios-simulator-universal.zip.002 \
    > ConsoleCrypt-0.2.1+62-ios-simulator-universal.zip
shasum -a 256 ConsoleCrypt-0.2.1+62-ios-simulator-universal.zip
unzip ConsoleCrypt-0.2.1+62-ios-simulator-universal.zip -d consolecrypt-ios
```

Порядок запуска — в [инструкции iOS Simulator](https://git.evsikov.net/publics/consolecrypt/-/blob/main/docs/public/BUILD_IOS.md). Это сборка для Simulator, не устанавливаемый на iPhone IPA.

## Проверки и ограничения

Flutter: **559 тестов**, анализ без замечаний. Клиент Rust: **653 теста**, **7 явно помеченных пропусков**, Clippy без предупреждений. Проверены версии, архитектуры, целостность архивов, полные лицензии и контрольные суммы нативных пакетов; iOS проверен запуском в отдельном Simulator.

Уменьшение задержек подтверждено синтетическими измерениями на Mac. Поведение на физических Windows-машинах, где сообщалось о зависаниях и полосах градиента, требует повторной проверки пользователями. Это ранние сборки: Windows без Authenticode, macOS с прежней локальной подписью без нотариализации Apple, Android с сохранённой preview-подписью.

При проверке зависимостей остаётся известное предупреждение `RUSTSEC-2023-0071` для RSA. Исправленная версия пока недоступна; путь расшифрования PKCS#1 с доступным извне оракулом в проверенном коде не обнаружен. Это не обещание отсутствия всех уязвимостей.

Лицензия собственных компонентов: **AGPL-3.0-only**. Сторонние компоненты сохраняют свои лицензии; уведомления MIT для терминального компонента включены в пакеты.

---

## English

**ConsoleCrypt 0.2.1** processes terminal output in smaller turns, guards malformed ANSI colours and oversized control sequences, and reduces repaint work for static glass surfaces. It also tightens secret, profile and AI lifecycle checks, requires the protected native clipboard path on mobile, revokes stopped SSH-agent keys, and cancels pending SFTP editor selection when its manager shuts down.

[Downloads](https://consolecrypt.evsikov.net/download?lang=en) · [Guide](https://consolecrypt.evsikov.net/guide?lang=en)

Packages: Windows x64 **+1280** (EXE / portable ZIP), macOS universal **+1281** (DMG), Android ARM64 **+61** (APK), and iOS Simulator universal **+62** (two ZIP parts). SHA-256 checksums are attached below. The public synchronization server supports sharing, groups, secrets and approved device enrolment; owners explicitly choose what to share, and vault content stays encrypted on the server.

Validation: **559 Flutter tests**, **653 client Rust tests**, **7 explicitly ignored Rust tests**, clean analysis and Clippy, native package integrity/licence checks, and an isolated iOS Simulator startup. Performance measurements were synthetic on Mac; reported physical Windows/GPU issues still need acceptance testing. These remain preview distributions: Windows is unsigned, macOS has a local signature without Apple notarization, Android retains its preview signer, and the iOS package is for Simulator only. RSA advisory `RUSTSEC-2023-0071` remains open in dependencies.

## Подтверждение публикации

Все семь файлов анонимно скачаны целиком. Размеры и SHA-256 совпадают с проверенными локальными пакетами. Релиз опубликован из тега `v0.2.1` (`e37af672cc64d7a9138b5ba82ab929bc43dda036`); одноразовая публикация выполнена [CI №198](https://git.evsikov.net/publics/consolecrypt/-/pipelines/198).

Подписанная лента обновлений публикует 0.2.1 для Windows1280, macOS1281 и Android61. Подпись Ed25519 проверена прежним публичным ключом клиента; URL, размеры и SHA-256 совпадают с анонимно проверенными пакетами.
