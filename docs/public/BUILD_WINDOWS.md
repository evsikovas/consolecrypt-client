# Сборка ConsoleCrypt для Windows

Нужен компьютер с **Windows 10/11 x64**. Flutter-приложение для Windows собирается
на Windows; macOS не может создать этот нативный пакет.

## Один раз установить

1. [Git for Windows](https://git-scm.com/downloads/win).
2. [Flutter SDK](https://docs.flutter.dev/install) **3.47.x** с Dart **3.13.4 или новее совместимой версии**.
   Распакуйте, например, в `C:\dev\flutter` и добавьте `C:\dev\flutter\bin` в PATH.
   Точная минимальная версия указана в `client/flutter/pubspec.yaml`; lock-файлы не удаляйте.
3. [Visual Studio](https://visualstudio.microsoft.com/downloads/) с workload
   **Desktop development with C++ / Разработка классических приложений на C++**:
   MSVC x64/x86, Windows SDK, CMake tools. Одного VS Code недостаточно.
4. [Rust через rustup](https://rustup.rs/), стандартный MSVC toolchain.
   Версия из `rust-toolchain.toml` загрузится при первом запуске Cargo.
5. [Strawberry Perl](https://strawberryperl.com/) x64 и [Python 3](https://www.python.org/downloads/windows/)
   с добавлением в PATH. Perl нужен для встроенного OpenSSL/SQLCipher.
6. [Inno Setup 6](https://jrsoftware.org/isdl.php) для создания установщика.

Откройте **новый PowerShell**. Проверьте:

```powershell
git --version
python --version
perl --version
cargo --version
flutter --version
flutter config --enable-windows-desktop
flutter doctor -v
```

Раздел Windows toolchain / Visual Studio в `flutter doctor` должен быть исправен.
Не нужно устанавливать Android SDK, Xcode или сервер PostgreSQL для сборки Windows-клиента.

## Скачать и собрать

Используйте короткий путь без кириллицы, например `C:\dev\consolecrypt`:

```powershell
New-Item -ItemType Directory -Force C:\dev | Out-Null
Set-Location C:\dev
git clone https://github.com/evsikovas/consolecrypt-client.git
Set-Location consolecrypt
.\client\scripts\build-windows.ps1 -Installer -NoCli
```

Если PowerShell блокирует локальные сценарии, только для текущего окна:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy RemoteSigned
.\client\scripts\build-windows.ps1 -Installer -NoCli
```

Сборка загружает зависимости и может занять значительное время при первом запуске.
Номер сборки автоматически увеличится. Скрипт проверяет ошибки инструментов,
копирует всю папку Flutter и распространяемые DLL Visual C++, затем запускает Inno Setup.
Rust и встроенный OpenSSL собираются в `target\windows-rust` у корня репозитория:
это сокращает пути для Perl на Windows. Сохраняйте короткий путь самого клона.
[Почему нужны все DLL и папка data](https://docs.flutter.dev/platform-integration/windows/building#building-your-own-zip-file-for-windows).

Результат в `dist\windows\`:

- `ConsoleCrypt-<версия>+<сборка>-windows-x64-setup.exe` — установщик.
- `ConsoleCrypt-windows.zip` — переносимая папка приложения.
- `ConsoleCrypt\ConsoleCrypt.exe` — запуск без установки, вся папка должна оставаться рядом.
- `*.sha256` — контрольные суммы; `ConsoleCrypt.version` — точная версия.

Установщик не требует администратора, добавляет пункт меню «Пуск», предлагает
ярлык на рабочем столе и поддерживает удаление. Хранилища пользователя при удалении
приложения сохраняются. Подписи издателя в локальной сборке нет.

Приложение устанавливается только для текущего пользователя:
`%LOCALAPPDATA%\Programs\ConsoleCrypt`. Хранилища и настройки сохраняются отдельно
в `%LOCALAPPDATA%\consolecrypt\ConsoleCrypt\data`, ключи — в Windows Credential
Manager текущего пользователя. Запуск от администратора не требуется.
Обновление и удаление приложения не затрагивают каталог хранилищ.

Без Inno Setup уберите `-Installer` — получите папку и ZIP. Уберите `-NoCli`,
чтобы дополнительно собрать `consolecrypt-cli.exe`. `-Mock` предназначен только
для демонстрации интерфейса и не должен использоваться для рабочего релиза.

## Проверить перед публикацией

На отдельном тестовом профиле проверьте запуск, создание хранилища, SSH,
SFTP, повторный запуск после блокировки, установку/обновление/удаление.
Проверьте ZIP на машине без Visual Studio: приложение должно находить DLL.
Автотесты интерфейса:

```powershell
Set-Location client\flutter
flutter analyze
flutter test
```

Запускайте их после завершения сборки. Не запускайте две Flutter-сборки одновременно
в одной копии проекта. [Загрузка установщика в существующий релиз](RELEASING.md).

## GitLab Runner

Задание `build-windows` использует runner с тегом `windows`. Сценарий
`client/scripts/ci-windows.ps1` находит инструменты в стандартных каталогах,
если PATH службы отличается от PATH пользователя. При отсутствии Flutter
скачивается официальный tag 3.47.5 с проверкой commit; при отсутствии Perl —
Strawberry Perl 5.40.5.1 Portable с проверкой SHA-256 официального релиза.
Кэш располагается вне checkout, по умолчанию `C:\GitLab-Runner\tools`.
Его можно перенести через `CC_CI_TOOLS_DIR`; `CC_CI_FLUTTER_ROOT` указывает
на корень заранее установленного Flutter SDK.

Если ISCC.exe не найден, CI также устанавливает официальный Inno Setup 6.7.3
в собственный каталог кэша, проверяя SHA-256.
Visual Studio C++, Rust и Python установите заранее.
Скрипт не меняет глобальный PATH и учётную запись службы.
Номер каждой нативной сборки берётся из уникального GitLab job ID;
готовые пакеты и контрольные суммы находятся в артефактах задания.
