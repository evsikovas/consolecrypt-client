# ConsoleCrypt 0.2.2

Исправлено сохранение обновлений macOS: новая копия приложения больше не должна получать запрет запуска из-за скачивания в песочнице. Раздел совместного доступа стал понятнее: вкладки данных отделены от добавления устройств, а дополнительные действия не занимают весь экран.

[Скачать](https://consolecrypt.evsikov.net/download?lang=ru) · [Руководство](https://consolecrypt.evsikov.net/guide?lang=ru) · [Свой сервер](https://git.evsikov.net/publics/consolecrypt/-/blob/main/docs/public/HOSTING.md)

## Что изменилось

- **Обновления macOS:** после проверки подписанной ленты и скачанного файла приложение предлагает системный диалог **«Сохранить и открыть»**. Выбранный файл записывается заново с проверкой размера и SHA-256, затем открывается DMG. Песочница и защита macOS остаются включены.
- **Отмена и повторная попытка:** отмена сохранения не выдаётся за успешную установку. Уже проверенная загрузка остаётся доступна для повторной попытки. Существующий файл не перезаписывается — выберите новое имя или другую папку.
- **Совместный доступ:** отдельные вкладки **«Доступно мне»** и **«Я поделился»**, пояснение текущей вкладки, самостоятельный блок устройств и понятные пустые состояния. Основные действия — **«Добавить это устройство»** и **«Подтвердить устройство»**; продолжение добавления и проверка истории доступны в **«Других действиях»**. Узкие экраны и увеличенный текст поддерживаются.
- **Документация:** обновлены инструкции по установке, совместной работе и добавлению устройств; руководство на сайте дополнено новыми снимками интерфейса на русском и английском.

## Один раз обновиться через браузер на macOS

Если у вас **ConsoleCrypt 0.2.1 или старее**, скачайте новый DMG через браузер со [страницы загрузки](https://consolecrypt.evsikov.net/download?lang=ru), закройте приложение и замените его в **Applications**. Открывайте установленную копию из Applications. Если там уже находится копия с ошибкой запуска, замените её копией из нового браузерного скачивания. Профили и зашифрованные хранилища сохраняются; удалять связку ключей или хранилища не требуется.

Старое приложение использует прежний способ скачивания и не может исправить его до обновления самого себя. Начиная с **0.2.2**, следующие обновления проходят через системный диалог сохранения. На macOS установка по-прежнему завершается переносом приложения из DMG в Applications.

## Сборки

| Платформа | Сборка | Пакет |
|---|---|---|
| Windows x64 | **0.2.2+1292** | EXE / portable ZIP |
| macOS Intel / Apple Silicon | **0.2.2+1293** | universal DMG |
| Android ARM64 | **0.2.2+64** | APK |
| iOS Simulator Intel / Apple Silicon | **0.2.2+65** | ZIP в двух частях `.001` / `.002` |

Файлы находятся в **Assets** ниже. `SHA256SUMS-0.2.2.txt` содержит контрольные суммы пакетов и собранного iOS ZIP. Windows и macOS собраны в [CI №201](https://git.evsikov.net/publics/consolecrypt/-/pipelines/201) из `8c29dcef92a5fa248849609c34104c49849990de`; Android и iOS — из тех же исходников с отдельными номерами сборки.

Для iOS Simulator скачайте обе части в одну папку:

```sh
cat ConsoleCrypt-0.2.2+65-ios-simulator-universal.zip.001 \
    ConsoleCrypt-0.2.2+65-ios-simulator-universal.zip.002 \
    > ConsoleCrypt-0.2.2+65-ios-simulator-universal.zip
shasum -a 256 ConsoleCrypt-0.2.2+65-ios-simulator-universal.zip
unzip ConsoleCrypt-0.2.2+65-ios-simulator-universal.zip -d consolecrypt-ios
```

[Порядок запуска iOS Simulator](https://git.evsikov.net/publics/consolecrypt/-/blob/main/docs/public/BUILD_IOS.md). Это сборка для Simulator; на iPhone она не устанавливается.

## Проверки

**566 тестов Flutter**, анализ без замечаний. Проверены нативный Swift-код экспорта обновлений, размер и контрольная сумма, отказ от перезаписи файлов и перехода по символьным ссылкам, отмена и повторная попытка. В отдельном подписанном приложении с песочницей воспроизведён старый запрет запуска и проверена запись через системный диалог с новой политикой. Защита macOS не отключалась.

Проверены версии, архитектуры, целостность архивов, лицензии и контрольные суммы всех пакетов. macOS прошёл `codesign --verify --deep --strict` и проверку новых разрешений песочницы; iOS проверен запуском в отдельном Simulator. Поведение на физических Windows/Android/iPhone не проверялось в этом выпуске.

Это предварительные сборки: Windows без Authenticode, macOS с прежней локальной подписью без нотариализации Apple, Android с сохранённой preview-подписью. Клиент Rust в этом выпуске не изменялся; результаты предыдущего выпуска не выдаются за повторный прогон. Известное предупреждение зависимости RSA `RUSTSEC-2023-0071` остаётся открытым.

Собственные компоненты: **AGPL-3.0-only**. Лицензии сторонних компонентов, включая MIT терминального компонента, сохранены в пакетах.

---

## English

**ConsoleCrypt 0.2.2** fixes macOS update export and simplifies the sharing screen. A verified update is now written through the system **Save and open** dialog, checked for size and SHA-256, and then opened as a DMG. App Sandbox remains enabled. Cancelling is handled honestly, the verified download can be retried, and existing destination files are not overwritten.

Sharing has clear **Available to me / Shared by me** tabs, a separate device card, two primary enrollment actions, secondary actions under **More actions**, and useful empty states. Narrow layouts and larger text are covered by tests. Sharing permissions and device verification requirements have not changed.

**One-time bootstrap on macOS:** when upgrading from **ConsoleCrypt 0.2.1 or earlier**, download the DMG through your browser, quit the app, replace it in **Applications**, and open the installed copy. Profiles and encrypted vaults are retained. Subsequent updates from 0.2.2 use the fixed system save dialog; moving the app from the DMG into Applications remains the final installation step.

[Downloads](https://consolecrypt.evsikov.net/download?lang=en) · [Guide](https://consolecrypt.evsikov.net/guide?lang=en)

Packages: Windows x64 **+1292**, macOS universal **+1293**, Android ARM64 **+64**, iOS Simulator universal **+65** in two ZIP parts. Checksums are attached. Validation: **566 Flutter tests**, clean analysis, native export regression tests, an isolated sandbox/save-panel reproduction, package integrity/licence/architecture checks, strict macOS signature verification, and isolated iOS Simulator startup. Physical device acceptance remains pending. Preview signing limitations and the existing RSA advisory remain as described above.

## Подтверждение публикации

Все семь файлов повторно анонимно скачаны целиком; размеры и SHA-256 совпадают
с проверенными локальными пакетами. Тег `v0.2.2` указывает на
`8c29dcef92a5fa248849609c34104c49849990de`. Одноразовая публикация выполнена
[CI №202](https://git.evsikov.net/publics/consolecrypt/-/pipelines/202).

Подписанная лента выдаёт 0.2.2 для Windows1292, macOS1293 и Android64.
Подпись Ed25519 проверена прежним публичным ключом клиента; URL, размеры и
SHA-256 совпадают с анонимно проверенными пакетами.
