# ConsoleCrypt 0.2.3

Более просторный поиск, компактная панель и последовательная отправка ввода в SSH-терминал. Обновлены установщики для всех поддерживаемых платформ и снимки руководства на русском и английском.

[Скачать](https://consolecrypt.evsikov.net/download?lang=ru) · [Руководство](https://consolecrypt.evsikov.net/guide?lang=ru) · [Свой сервер: Docker / Kubernetes](https://git.evsikov.net/publics/consolecrypt/-/blob/main/docs/public/HOSTING.md)

## Что изменилось

- **Ввод в терминале:** символы и Enter отправляются по очереди, даже если предыдущая запись SSH ещё не завершилась. Закрытие вкладки или переподключение отменяет оставшуюся очередь. В проверках охвачены кириллица, английский текст, составные Unicode-символы и разбитые на байты ответы SSH. Точный испорченный символ из сообщения пользователя пока не воспроизведён; проверку на его Windows после обновления ещё нужно подтвердить.
- **Новое подключение:** круглая кнопка **+**, подсказка при наведении и прежнее действие подключения.
- **Поиск:** больше места для имени хоста, IP-адреса, сниппета или вопроса ИИ.
- **Синхронизация:** успешное состояние отображается зелёной точкой и **«Синх.»**. Полная подсказка сохранена; ошибки, отсутствие сети и отключённая синхронизация показывают своё состояние.
- **Руководство:** 72 свежих снимка интерфейса на русском и английском, включая мобильный экран. Это демонстрационные данные; реальные хранилища пользователей не использовались.

## Если macOS не открывает приложение после старого автообновления

Скачайте новый DMG **через браузер** со [страницы загрузки](https://consolecrypt.evsikov.net/download?lang=ru), завершите ConsoleCrypt и замените приложение в **Applications**. Запускайте установленную копию оттуда. Зашифрованные хранилища, профили и связку ключей удалять не нужно.

Если 0.2.2 или 0.2.3 была скачана старой кнопкой обновления, повторная загрузка этой же кнопкой может снова переносить запрет запуска macOS. Новая версия не исправляет способ загрузки уже работающего старого клиента до первой чистой установки. После неё обновления сохраняются через системный диалог **«Сохранить и открыть»**, с проверкой размера и SHA-256. Перенос приложения из DMG в Applications остаётся последним шагом установки. Песочница и защита macOS сохраняются.

## Сборки

| Платформа | Версия | Пакет |
|---|---|---|
| Windows 10/11 x64 | **0.2.3+1309** | EXE / portable ZIP |
| macOS Intel / Apple Silicon | **0.2.3+1310** | universal DMG |
| Android ARM64 | **0.2.3+67** | APK |
| iOS Simulator Intel / Apple Silicon | **0.2.3+68** | ZIP, части `.001` / `.002` |

Файлы находятся в **Assets** ниже. `SHA256SUMS-0.2.3.txt` содержит суммы всех пакетов и объединённого iOS ZIP. Исходники: `8d2213f5bc24bd94a4eac6ee85f132ac3bd5600a`. Windows и macOS собраны в [CI №206](https://git.evsikov.net/publics/consolecrypt/-/pipelines/206); мобильные пакеты используют те же исходники с отдельными номерами сборки.

Для iOS Simulator загрузите обе части в одну папку:

```sh
cat ConsoleCrypt-0.2.3+68-ios-simulator-universal.zip.001 \
    ConsoleCrypt-0.2.3+68-ios-simulator-universal.zip.002 \
    > ConsoleCrypt-0.2.3+68-ios-simulator-universal.zip
shasum -a 256 ConsoleCrypt-0.2.3+68-ios-simulator-universal.zip
unzip ConsoleCrypt-0.2.3+68-ios-simulator-universal.zip -d consolecrypt-ios
```

[Запуск iOS Simulator](https://git.evsikov.net/publics/consolecrypt/-/blob/main/docs/public/BUILD_IOS.md). Этот пакет не устанавливается на физический iPhone.

## Проверки выпуска

**576 тестов Flutter прошли**, анализ без замечаний. Отдельные регрессии проверяют очередь ввода и её отмену, локализацию панели, размеры поиска, успешное состояние синхронизации и сохранение предупреждений. Нативные проверки экспорта обновлений, номера сборки и состава публичных исходников прошли.

Проверены версии, архитектуры, библиотеки, лицензии, CRC и SHA-256 пакетов. DMG прошёл проверку целостности; macOS-приложение — строгую проверку подписи и разрешений песочницы. APK сохраняет прежнюю preview-подпись и поддерживает 16 КБ страницы памяти. iOS Simulator проверен запуском на отдельном временном устройстве, которое затем удалено. Физические Windows/Android/iPhone и ввод на компьютере пользователя не проверялись.

Предварительные сборки: Windows без Authenticode, macOS с прежней локальной подписью без нотариализации Apple, Android с preview-подписью. Rust-код клиента не изменялся; прежние результаты Rust не выдаются за повторный прогон. Известное предупреждение зависимости RSA `RUSTSEC-2023-0071` остаётся открытым.

Собственные компоненты: **AGPL-3.0-only**. Лицензии сторонних компонентов, включая MIT терминального компонента, сохранены в пакетах.

---

## English

**ConsoleCrypt 0.2.3** gives host search more room, replaces New connection with a round **+** and tooltip, and shows successful synchronization as a green dot and **Sync**. Errors and offline states retain their full status. The guide includes 72 fresh Russian and English screenshots made with demonstration data.

SSH input is now queued in order: characters and Enter wait for the preceding write, and closing or reconnecting cancels stale input. Regression tests cover Cyrillic, Latin text, Unicode composition and byte-fragmented SSH output. The exact malformed character reported on Windows has not been reproduced; acceptance on that user's computer remains pending.

**macOS bootstrap:** if an old in-app downloader produced a copy that cannot launch, download the latest DMG **through your browser**, quit ConsoleCrypt, replace the app in **Applications**, and open the installed copy. Keep your profiles, encrypted vaults and Keychain. Downloading a newer version again with the old button can carry the same launch restriction. After this first clean installation, updates use the system **Save and open** dialog with size and SHA-256 checks. Moving the app from DMG to Applications remains the installation step; security protections stay enabled.

[Downloads](https://consolecrypt.evsikov.net/download?lang=en) · [Guide](https://consolecrypt.evsikov.net/guide?lang=en)

Packages: Windows x64 **+1309**, macOS universal **+1310**, Android ARM64 **+67**, iOS Simulator universal **+68** in two ZIP parts. **576 Flutter tests**, clean analysis, native export/version/source checks, archive and licence verification, strict macOS signature verification, Android 16 KB checks and isolated iOS Simulator startup passed. Physical device acceptance remains pending. Preview signing limitations and the existing RSA advisory are described above.

## Подтверждение публикации

Все семь файлов независимо скачаны целиком без входа в GitLab: размеры и SHA-256 совпали с проверенными локальными пакетами. Тег `v0.2.3` указывает на `8d2213f5bc24bd94a4eac6ee85f132ac3bd5600a`; публикация выполнена в [CI №207](https://git.evsikov.net/publics/consolecrypt/-/pipelines/207).

Подписанная лента выдаёт 0.2.3 для Windows1309, macOS1310 и Android67. Подпись Ed25519 проверена прежним публичным ключом клиента; URL, размеры и SHA-256 совпадают с анонимно проверенными пакетами. Известные локальные рабочие секреты в проверенных архивах и распакованном macOS-приложении не обнаружены; это проверка известных значений, а не гарантия отсутствия любых возможных секретов.
