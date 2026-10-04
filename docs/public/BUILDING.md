# Сборка и сервер синхронизации

Во всех случаях клонируйте репозиторий целиком: Rust-клиент использует общие
библиотеки из `crates/`. Flutter SDK 3.47.x, Dart от 3.13.4; Rust закреплён в
`rust-toolchain.toml`. Сохраняйте `Cargo.lock`, `pubspec.lock` и `Podfile.lock`.

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

## Свой сервер

Локальный профиль работает без сервера. Для синхронизации используйте
[отдельный репозиторий сервера и протокола](https://github.com/evsikovas/consolecrypt-server).
Инструкция Docker/SMTP и команды запуска находятся в его README и HOSTING.md.
Серверный код не входит в актуальный main клиентского репозитория.
