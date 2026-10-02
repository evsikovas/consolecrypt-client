<p align="center"><img src="https://git.evsikov.net/publics/consolecrypt/-/raw/v0.2.5/docs/brand/consolecrypt.svg" width="480" alt="ConsoleCrypt"></p>

# ConsoleCrypt 0.2.5 · Теперь и на Linux

🐧 Рабочее пространство ConsoleCrypt доступно на Linux: терминал SSH, SFTP,
хосты и группы, сниппеты, ИИ-помощник и зашифрованная совместная работа.
Новые пакеты **DEB и RPM** устанавливаются через обычный менеджер программ.

## Что нового

- Нативный клиент **Linux x86-64** с настоящим Rust-ядром, иконкой и пунктом меню приложений.
- Ключи хранилища защищает системная **Secret Service**. Клиент использует существующую разблокированную ключницу и не сохраняет ключи в открытом виде, если она недоступна.
- Установка через `apt` или `dnf` автоматически получает необходимые графические библиотеки.
- В настройках Linux указан адрес новых пакетов; обновление выполняется менеджером пакетов, без подписанной ленты и автоматической установки.
- В SFTP можно выбрать установленный в Linux редактор файлов.
- Сборка GitLab проверяет настоящий Rust bridge и создание хранилища через интерфейс, сохранение хоста, перезапуск и разблокировку перед упаковкой.

## Скачать

| Платформа | Сборка | Файлы |
|---|---|---|
| Linux x86-64 · Debian/Ubuntu | 0.2.5+1358 | [DEB](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.2.5/ConsoleCrypt-0.2.5%2B1358-linux-x64.deb) |
| Linux x86-64 · Fedora | 0.2.5+1358 | [RPM](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.2.5/ConsoleCrypt-0.2.5%2B1358-linux-x64.rpm) |
| Windows 10/11 · x64 | 0.2.5+1353 | [Установщик EXE](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.2.5/ConsoleCrypt-0.2.5%2B1353-windows-x64-setup.exe) · [Portable ZIP](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.2.5/ConsoleCrypt-0.2.5%2B1353-windows-x64-portable.zip) |
| macOS 12+ · Apple Silicon и Intel | 0.2.5+1354 | [Универсальный DMG](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.2.5/ConsoleCrypt-0.2.5%2B1354-macos-universal.dmg) |
| Android 11+ · ARM64 | 0.2.5+75 | [APK](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.2.5/ConsoleCrypt-0.2.5%2B75-android-arm64.apk) |
| iOS Simulator · Mac, ARM64/x86-64 | 0.2.5+76 | [ZIP, часть 1](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.2.5/ConsoleCrypt-0.2.5%2B76-ios-simulator-universal.zip.001) · [Часть 2](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.2.5/ConsoleCrypt-0.2.5%2B76-ios-simulator-universal.zip.002) |

[SHA-256 всех файлов](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.2.5/SHA256SUMS-0.2.5.txt)
· [Сайт и загрузка](https://consolecrypt.evsikov.net/download?lang=ru)
· [Руководство](https://consolecrypt.evsikov.net/guide?lang=ru)
· [Установка Linux и сборка из исходников](https://git.evsikov.net/publics/consolecrypt/-/blob/v0.2.5/docs/public/BUILD_LINUX.md)

## Установить Linux

Нужны графический рабочий стол, D-Bus текущего сеанса и разблокированная основная
ключница Secret Service, например GNOME Keyring. Запускайте клиент от своего
пользователя, без `sudo`.

```sh
# Debian / Ubuntu
sudo apt install ./ConsoleCrypt-0.2.5+1358-linux-x64.deb

# Fedora
sudo dnf install ./ConsoleCrypt-0.2.5+1358-linux-x64.rpm

consolecrypt
```

Для обновления скачайте новый пакет и повторите команду установки. Установщик
и удаление приложения не очищают домашний каталог и ключницу. Перезапуск и
разблокировка настоящего тестового хранилища проверены отдельно через интерфейс.

## Проверки выпуска

Исходники выпуска: `96fc23fb5426dbee5efb75855d837cfc29f6ad66`.

| Платформа | Происхождение и подтверждённая проверка |
|---|---|
| Linux +1358 | [build-linux 1358](https://git.evsikov.net/publics/consolecrypt/-/jobs/1358), исходники выпуска; Ubuntu 22.04, реальные Rust bridge 2/2 и UI 1/1 с Secret Service, перезапуском и разблокировкой |
| Windows +1353 | [build-windows 1353](https://git.evsikov.net/publics/consolecrypt/-/jobs/1353), `41323da718694fb56a6868abba65c6fc1a3e91fb`; целостность ZIP, SHA-256, x64 DLL/CRT и версии EXE; установщик не запускался |
| macOS +1354 | [build-macos 1354](https://git.evsikov.net/publics/consolecrypt/-/jobs/1354), `41323da718694fb56a6868abba65c6fc1a3e91fb`; DMG, SHA-256, строгая подпись, universal ARM64/x86-64 и sandbox entitlement; приложение не запускалось |
| Android +75 | Локальная нативная сборка; ARM64 APK, прежняя подпись, ZIP/ELF выравнивание 16 KiB и SHA-256 проверены; физический телефон не проверялся |
| iOS Simulator +76 | Локальная нативная сборка; ARM64/x86-64 ZIP, подпись, SHA-256 и восстановление из частей проверены; отдельный временный Simulator показал экран приветствия, физический iPhone не проверялся |

[Pipeline 224](https://git.evsikov.net/publics/consolecrypt/-/pipelines/224)
в целом завершился **FAILED** из-за прежнего Linux-задания 1352.
Windows 1353 и macOS 1354 прошли отдельно; исправленная Linux-сборка 1358
прошла вместо 1352. Между их снимком и итоговыми исходниками менялись только
Linux CI/упаковка, тесты и метаданные сборки. Код приложения Windows/macOS
остался тем же. Android/iOS собраны локально: по неизменяемой записи проверки
подтверждено совпадение поставляемого кода с публичным итоговым снимком.
Из сравнения исключены метаданные отдельных сборок и интеграционный тест,
не входящий в приложение; это не утверждение о сборке из точного тега.
Готовые файлы Windows/macOS/Android/iOS сохранены
побайтно; изменения Linux CI не потребовали их пересборки.

Готовые DEB/RPM **+1358** отдельно установлены и запущены в изолированных
Debian 12 и Fedora 43 с X11 и программным рендерингом. Проверены автоматическая
установка графических зависимостей через `apt`/`dnf`, обновление
`0.2.5-79 → 0.2.5-1358`, удаление/повторная установка, четыре нативных библиотеки,
лицензии и экран приветствия. Сохранность при обновлении/удалении проверялась
только на синтетических маркерах собственного HOME/ключницы; это не проверка
миграции настоящего пользовательского хранилища. CI отдельно проверяет
сохранение и восстановление настоящего тестового зашифрованного хранилища.

Во всех пакетах проверены полный текст AGPL и сохранённая MIT-лицензия xterm.
Физический GPU, Wayland, Linux ARM64, RHEL и Flatpak/AppImage не входят в
подтверждённую матрицу. Linux обновляется вручную пакетами; подписанного
автообновления Linux нет. Это предварительный выпуск: macOS без нотариализации
Apple, Windows без Authenticode, APK с прежней тестовой подписью.
Известное предупреждение зависимости RSA `RUSTSEC-2023-0071` остаётся открытым.

**macOS:** если копия, скачанная старой кнопкой обновления, не открывается,
один раз загрузите DMG через браузер, завершите ConsoleCrypt, замените копию
в Applications и запустите её. Повторная загрузка старой кнопкой может
сохранить запрет запуска. Следующие обновления используют системный диалог
«Сохранить и открыть». Профили, хранилище и Keychain удалять не нужно;
защита macOS остаётся включена. [Подробнее](UPDATES.md).

🔐 Хранилище остаётся зашифрованным на устройстве. Можно работать локально,
подключить свой сервер синхронизации или выбрать публичный сервер
`https://consolecrypt.evsikov.net`. SSH идёт непосредственно к вашим хостам.

---

**English:** ConsoleCrypt now supports Linux x86-64 with DEB and RPM packages,
native Rust SSH/SFTP, encrypted workspaces and team sharing. Install with
`sudo apt install ./ConsoleCrypt-0.2.5+1358-linux-x64.deb` or
`sudo dnf install ./ConsoleCrypt-0.2.5+1358-linux-x64.rpm`, then run `consolecrypt`
as your regular desktop user. An existing unlocked persistent Secret Service
keyring is required. Linux updates use the package manager. Download links
for every platform and SHA-256 checksums are listed above. Linux has no signed
in-app updater feed or automatic package installation. Debian 12 and Fedora 43
package installation, upgrade and software-X11 welcome-screen launch passed;
marker preservation is limited to synthetic HOME/keyring fixtures, not a real
user-vault migration. Physical GPU/Wayland and physical Windows/mobile-device
acceptance are not claimed. Pipeline 224 failed on the superseded Linux job;
Windows 1353 and macOS 1354 passed separately, followed by successful Linux 1358.
Their shipping runtime matches the final source despite later CI/test changes.
Android/iOS were built locally; an immutable comparison confirmed shipping-code
equivalence with the public final source, excluding per-build metadata and a
non-shipping integration test. They are not claimed to have been built from the exact tag.
If an old macOS downloader produced an app that cannot launch, download the DMG
once through your browser and replace the app in Applications; keep the vault
and Keychain. Subsequent updates use the system Save and open dialog.

**Alexander Evsikov** · [i@evsikov.net](mailto:i@evsikov.net) ·
[GNU AGPL-3.0-only](https://git.evsikov.net/publics/consolecrypt/-/blob/v0.2.5/LICENSE).
Third-party components retain their own licenses.
