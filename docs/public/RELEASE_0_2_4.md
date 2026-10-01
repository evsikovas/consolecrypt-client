# ConsoleCrypt 0.2.4

**Нативный SSH исправлен для ввода кириллицы; поиск стал шире, хосты и группы объединены в одном разделе.**

- **Кириллица:** воспроизведено и исправлено превращение первой «п» в `:�` в реальной SSH-сессии с Bash/Readline. Терминал запрашивает UTF-8 для символов и корректное удаление целого символа; английский ввод продолжает работать. Настройки языка и формат чисел/дат сервера сохраняются. После обновления откройте новую SSH-сессию.
- **Поиск:** занимает всю свободную ширину верхней панели. Имя или IP хоста, сниппет и запрос ИИ доступны через одну строку; круглый **+** открывает новое подключение, зелёная точка и **Синх.** показывают успешную синхронизацию.
- **Хосты:** отдельный пункт «Группы» убран из левого меню. Создание, вложенные группы и общие настройки доступны внутри **Хостов**; старые ссылки на группы работают.
- **Руководство:** 72 новых русских и английских скриншота из текущего интерфейса с демонстрационными данными.

[Скачать](https://consolecrypt.evsikov.net/download?lang=ru) · [Руководство](https://consolecrypt.evsikov.net/guide?lang=ru)

## Установка и обновление

| Платформа | Версия | Пакет |
|---|---|---|
| Windows 10/11 x64 | **0.2.4+1332** | EXE и portable ZIP |
| macOS Intel / Apple Silicon | **0.2.4+1333** | universal DMG |
| Android ARM64 | **0.2.4+71** | APK |
| iOS Simulator Intel / Apple Silicon | **0.2.4+72** | ZIP, части `.001` / `.002` |

Файлы — в **Assets** ниже. `SHA256SUMS-0.2.4.txt` содержит суммы пакетов и объединённого iOS ZIP. Исходники: `39d3a487c6aaa9098a26ff79672b4359acb6da36`. Windows и macOS собраны в [CI №213](https://git.evsikov.net/publics/consolecrypt/-/pipelines/213); мобильные сборки используют те же исходники с отдельными номерами.

**macOS:** если приложение не открывается после скачивания старой кнопкой обновления, один раз загрузите DMG **через браузер**, закройте ConsoleCrypt, замените приложение в **Applications** и запустите установленную копию. Повторная загрузка в старом приложении может переносить тот же запрет запуска. После чистой установки обновления сохраняются через системный диалог **«Сохранить и открыть»** с проверкой размера и SHA-256. Профили, хранилище и Keychain удалять не нужно; защиты macOS остаются включены.

Для iOS Simulator загрузите обе части в одну папку:

```sh
cat ConsoleCrypt-0.2.4+72-ios-simulator-universal.zip.001 \
    ConsoleCrypt-0.2.4+72-ios-simulator-universal.zip.002 \
    > ConsoleCrypt-0.2.4+72-ios-simulator-universal.zip
shasum -a 256 ConsoleCrypt-0.2.4+72-ios-simulator-universal.zip
unzip ConsoleCrypt-0.2.4+72-ios-simulator-universal.zip -d consolecrypt-ios
```

[Запуск iOS Simulator](https://git.evsikov.net/publics/consolecrypt/-/blob/main/docs/public/BUILD_IOS.md). Этот пакет не устанавливается на физический iPhone.

## Проверки и границы

**577 тестов Flutter прошли**, анализ без замечаний. **54 теста SSH прошли**, включая две интеграции с настоящим OpenSSH; один прежний медленный тест генерации RSA4096 пропущен. Clippy и проверка потребителей terminal-core/AppCore/bridge прошли. Реальные регрессии проверяют кириллицу и ASCII, ввод после сниппета, вставку, выполнение и Backspace. Нативные проверки сохранения macOS-обновлений, номера сборки и публичного экспорта прошли.

Исправление локали относится к нативному SSH, используемому по умолчанию. Если администратор запрещает `AcceptEnv`, на сервере нет `C.UTF-8` или задан `LC_ALL=C`, сервер необходимо настроить на UTF-8. Явно выбранный системный OpenSSH backend не изменён. Секреты и пользовательские SSH-сессии при проверках не использовались. Физическая проверка на компьютере пользователя остаётся отдельным шагом.

Предварительные сборки: Windows без Authenticode, macOS с прежней локальной подписью без нотариализации Apple, Android с preview-подписью. Известное предупреждение зависимости RSA `RUSTSEC-2023-0071` остаётся открытым. Собственные компоненты: **AGPL-3.0-only**; сторонние лицензии, включая MIT терминального компонента, сохранены.

---

## English

**ConsoleCrypt 0.2.4** fixes a reproduced Cyrillic input problem in native SSH, gives search the remaining toolbar width, and puts host groups inside **Hosts** instead of a duplicate sidebar entry. Legacy group links continue to work. The guide includes 72 fresh Russian and English screenshots.

A real OpenSSH/Bash regression reproduced the first Cyrillic character becoming `:�` in the C locale. Native terminals now request UTF-8 character handling and whole-character erase without changing the server language or number/date formats. Open a new SSH session after updating. ASCII, Cyrillic after snippets, paste, execution and Backspace are covered. A server refusing environment requests, missing `C.UTF-8` or forcing `LC_ALL=C` requires UTF-8 configuration; the explicitly selected system OpenSSH backend is unchanged.

**macOS bootstrap:** if an old in-app download produced a copy that cannot launch, download the latest DMG **through your browser**, quit ConsoleCrypt, replace it in **Applications**, and open the installed copy. Keep profiles, encrypted vaults and Keychain. After this one-time clean installation, updates use the system **Save and open** dialog. Security protections stay enabled.

[Downloads](https://consolecrypt.evsikov.net/download?lang=en) · [Guide](https://consolecrypt.evsikov.net/guide?lang=en)

Packages: Windows x64 **+1332**, macOS universal **+1333**, Android ARM64 **+71**, iOS Simulator universal **+72** in two ZIP parts. **577 Flutter tests**, **54 SSH tests**, clean analysis/Clippy and native consumer checks passed. Physical user-device acceptance remains pending; preview signing limitations and the existing RSA advisory are described above.

## Проверка опубликованных файлов

Все семь файлов независимо скачаны целиком без входа в GitLab: размеры и SHA-256 совпали. Тег `v0.2.4` указывает на `39d3a487c6aaa9098a26ff79672b4359acb6da36`; публикация — [CI №214](https://git.evsikov.net/publics/consolecrypt/-/pipelines/214).

Подписанная лента выдаёт 0.2.4 для Windows1332, macOS1333 и Android71. Подпись Ed25519 проверена прежним публичным ключом клиента; URL, размеры и SHA-256 совпадают с проверенными пакетами. Проверка 105 представлений известных рабочих секретов в установщиках, вложенных ZIP и распакованном macOS-приложении не нашла совпадений. Это проверка известных значений, а не гарантия отсутствия любых возможных секретов.
