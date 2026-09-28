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

Предварительная сборка для Android 11+ (API 30), включая Pixel:

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

Для локального профиля сервер не нужен. Для синхронизации между устройствами
можно запустить сервер из этого репозитория (Docker и Compose):

```sh
cp server/.env.example server/.env
# Откройте server/.env и замените CC_POSTGRES_PASSWORD на случайный пароль.
docker compose -f server/docker-compose.yml --env-file server/.env up -d --build
```

В `.env` задайте `CC_SOURCE_CODE_URL=https://git.evsikov.net/publics/consolecrypt`
(для изменённого сервера — адрес именно ваших исходников).
По умолчанию HTTP доступен на порту 8080, PostgreSQL хранит данные в Docker volume.
Для удалённого доступа настройте HTTPS через reverse proxy и публичный адрес;
параметры почты, регистрации, лимитов и хранения приведены в `server/.env.example`.
Резервируйте PostgreSQL и проверяйте восстановление. Сервер не может восстановить
забытую парольную фразу хранилища: сохраните комплект восстановления на клиенте.
