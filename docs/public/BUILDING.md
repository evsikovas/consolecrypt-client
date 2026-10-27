# Сборка и сервер синхронизации

Во всех случаях клонируйте репозиторий целиком: Rust-клиент использует общие
библиотеки из `crates/`. Flutter SDK 3.47.x, Dart от 3.13.4; Rust закреплён в
`rust-toolchain.toml`. Сохраняйте `Cargo.lock`, `pubspec.lock` и `Podfile.lock`.

## Релизы из GitHub

`main` содержит актуальные исходники. Установщики собираются только при отправке
стабильного тега `vX.Y.Z`, совпадающего с версией в `pubspec.yaml`. Тег должен
указывать на коммит из истории `main`. Push в ветку и ручной запуск без тега
не выпускают пакеты. Старый GitLab pipeline отключён.

Workflow `Client tag release` проверяет тег, запускает Windows, macOS, Android,
Linux и iOS Simulator, затем публикует один полный GitHub Release с SHA-256.
Windows использует `cc-windows-x64`, Android — `cc-macos-arm64`, Linux — ARC
`cc-linux-k3s`. Для macOS/iOS по умолчанию используется тот же Mac с полным Xcode;
переменная `MACOS_BUILD_RUNNER` позволяет задать JSON-массив других меток, например
`["macos-26"]` для подготовленного GitHub runner. Command Line Tools недостаточно.
SDK Android задаётся через `ANDROID_HOME`, JDK — через `JAVA_HOME` в repository
variables. Ключи подписи сохраняются на соответствующих раннерах.

После публикации отдельно запускается подписывание лент обновлений из `main`.
Ежедневный workflow `Signed update feeds` продлевает срок подписанной метаинформации
при необходимости; он не собирает установщики. Уже опубликованные версии пакетов
не заменяются: изменения выпускаются новым тегом.

## macOS

Требуются полный Xcode с командными инструментами, Flutter, Rust/rustup,
CocoaPods и Python 3. Apple Silicon и Intel объединяются в универсальный пакет.
Из корня репозитория:

```sh
client/scripts/build-macos.sh --no-cli
```

Результат: `dist/macos/ConsoleCrypt.dmg`, приложение, ZIP и файл версии.
Образ включает ссылку Applications и фон установки. Python-зависимости упаковки
устанавливаются в локальное окружение внутри `dist/`.
При наличии сертификата можно указать `CC_CODESIGN_IDENTITY` либо `--sign-identity`.
Стабильная подпись необходима, чтобы обновления не меняли доверие связки ключей;
для публичного распространения с Gatekeeper нужна также нотариализация Apple.

## Android · ARM64

Предварительная сборка для Android 11+ (API 30), ARM64:

- JDK 17;
- Android SDK platform 36, build-tools 36.0.0;
- NDK 28.2.13676358, CMake 3.22.1;
- Flutter и Rust; Python 3 для проверки нативных библиотек.

```sh
export ANDROID_HOME="$HOME/Library/Android/sdk"
client/scripts/build-android.sh
```

При другом расположении SDK задайте свой `ANDROID_HOME`. Flutter восстанавливает
Gradle wrapper при подготовке Android-проекта. Скрипт проверяет APK-подпись,
ZIP alignment и выравнивание нативных библиотек для 16 KiB страниц памяти.
Результат: `dist/android/ConsoleCrypt-android-arm64.apk`, SHA-256 и точная версия.

При локальной сборке для обновления уже установленного APK проверьте номер
сборки: опубликованный CI-пакет может иметь номер выше исходного `pubspec.yaml`.
Перед запуском задайте `CC_BUILD_NUMBER` целым числом больше номера установленного
пакета и исходного номера сборки. Скрипт запишет его в метаданные; одинаковая
подпись также необходима. Без `CC_BUILD_NUMBER` увеличивается только исходный
счётчик, поэтому такой APK может оказаться старее установленного.

Текущая конфигурация использует локальный **debug keystore** даже в release APK.
Это тестовый пакет. Приватный ключ не входит в Git. APK, собранные на разных
компьютерах с разными ключами, не обновляют друг друга поверх установленной версии.
Перед переходом на другую подпись сохраните зашифрованную резервную копию.
Для production-дистрибутива нужен отдельный защищённый ключ подписи и настройка
release signing; не публикуйте такой ключ в репозитории.

На Android фоновые ограничения ОС могут прерывать SSH-сессию. SFTP использует
системный выбор документов; интеграция с настольными редакторами доступна на компьютере.

## Windows

[Пошаговая инструкция и создание EXE-установщика](BUILD_WINDOWS.md).

## Linux · x86-64

[Установка DEB/RPM, системная ключница и сборка](BUILD_LINUX.md).

```sh
client/scripts/build-linux.sh
```

Результат: два пакета в `dist/linux/`, SHA-256 и точная версия.
Базовое окружение пакетов — Ubuntu 22.04 x86-64; установка проверена в
Debian 12 и Fedora 43. Для запуска нужны D-Bus графического сеанса и уже
созданная разблокированная основная ключница Secret Service. Обновляйте Linux
вручную через `apt`/`dnf`; подписанная лента обновлений и автоматическая
установка Linux не поддерживаются. Физический GPU и Wayland не проверены.

## iOS · предварительный порт

[Simulator, нативные проверки и подпись для iPhone](BUILD_IOS.md).
Simulator ZIP предназначен для Xcode Simulator; установка на телефон требует
отдельной подписи в Xcode.

## Тесты

```sh
(cd crates && cargo test --locked)
(cd client/rust && cargo test --locked)
(cd client/flutter && flutter pub get && flutter analyze && flutter test)
python3 -m unittest discover -s client/scripts -p 'test_*.py'
```

Не запускайте Flutter-тесты одновременно с нативной сборкой: они обновляют общие
сгенерированные файлы. Bridge уже сгенерирован и включён в исходники;
обычная сборка не требует установки flutter_rust_bridge_codegen.

## Проверка зависимостей

`client/scripts/dependency-audit.py check` запускает `cargo deny check advisories
bans sources licenses` для `client/rust` и `crates`, проверяет правило собственных
crates и сканирует `pubspec.lock` (и npm lockfile, если он появится) через
`osv-scanner`. Локально нужны те же версии, что и в CI:

```sh
cargo install --locked cargo-deny@0.20.2
go install github.com/google/osv-scanner/v2/cmd/osv-scanner@v2.6.0
python3 client/scripts/dependency-audit.py check
```

Одна проверка: `cargo deny --manifest-path client/rust/Cargo.toml --locked check
advisories bans sources licenses`.

В CI шаг выполняется только на GitHub-hosted `ubuntu-24.04`: в `validate` тегового
выпуска (провал останавливает сборку пакетов) и в еженедельном
`dependency-audit.yml`, который также проверяет pull request с изменёнными
зависимостями. Еженедельный прогон открывает или обновляет issue «Уязвимые
зависимости» и закрывает его после чистого прогона. Инструменты скачиваются из
GitHub Releases с проверкой SHA-256, закреплённого в скрипте.

Политика (`deny.toml`):

- уязвимости и yanked-версии запрещены; unmaintained — только для прямых
  зависимостей наших crates;
- TLS только rustls: `native-tls` и `openssl` запрещены, `openssl-sys` разрешён
  лишь как бэкенд SQLCipher (`libsqlite3-sys`);
- git-источник разрешён только `github.com/evsikovas/consolecrypt-server`
  (`cc-protocol` по ревизии);
- собственные crates — `cc-*`, `publish = false`, `AGPL-3.0-only`: новый модуль
  не требует правки исключений лицензий, а скрипт отклоняет любой другой
  неопубликованный crate.

Исключение оформляется только осознанно: в `ignore` файла `deny.toml` (или
`[[IgnoredVulns]]` в `osv-scanner.toml`) с идентификатором совета, причиной,
владельцем и датой пересмотра; срок — не более трёх месяцев. Сейчас принято:

- `RUSTSEC-2023-0071` (rsa, Marvin): исправленной версии нет; закрытый RSA-ключ
  используется только для входа по SSH и в NTLM/CredSSP для RDP, единицы операций
  на подключение. SSH-ключи по умолчанию генерируются Ed25519, рекомендуется
  именно он. Исправленной версии по-прежнему нет (проверено 27.10.2026: rsa
  0.9.12 и 0.10.0-rc.18). Пересмотр 2027-01-21.

PEM-сертификаты и ключи баз данных `db-core` читает через `rustls-pki-types`
(`PemObject`); `rustls-pemfile` больше не используется.

## Свой сервер

Локальный профиль работает без сервера. Для синхронизации используйте
[отдельный репозиторий сервера и протокола](https://github.com/evsikovas/consolecrypt-server).
Инструкция Docker/SMTP и команды запуска находятся в его README и HOSTING.md.
Серверный код не входит в актуальный main клиентского репозитория.
