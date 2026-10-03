<p align="center"><img src="docs/brand/consolecrypt.svg" alt="ConsoleCrypt" width="520"></p>
<p align="center"><strong>Ваши серверы. Ваши ключи. Ваше рабочее пространство.</strong></p>
<p align="center">
<a href="https://consolecrypt.evsikov.net/?lang=ru">Сайт RU</a> ·
<a href="https://consolecrypt.evsikov.net/?lang=en">Website EN</a> ·
<a href="https://consolecrypt.evsikov.net/guide">Руководство</a> ·
<a href="https://consolecrypt.evsikov.net/account">Личный кабинет</a> ·
<a href="https://git.evsikov.net/publics/consolecrypt/-/releases/v0.3.0">Скачать 0.3.0</a> ·
<a href="docs/public/BUILD_WINDOWS.md">Собрать для Windows</a> ·
<a href="docs/public/BUILDING.md">Сборка и свой сервер</a> ·
<a href="https://git.evsikov.net/publics/consolecrypt/-/issues">Сообщить об ошибке</a>
</p>

ConsoleCrypt — клиент SSH и RDP с терминалом, удалёнными рабочими столами Windows, SFTP, группами хостов, сниппетами и
ИИ-помощником. В панели сверху — круглый «+» для подключения, широкий поиск по имени или IP хоста и компактный статус синхронизации. Работает с локальным зашифрованным хранилищем; для синхронизации
между устройствами можно подключить собственный сервер или публичный
**https://consolecrypt.evsikov.net**. Этот же адрес укажите в настройках сервера
приложения. Учётные записи на разных серверах независимы.

Запуск своего сервера: [Docker Compose](docs/public/HOSTING.md#docker-compose)
или [Kubernetes / Helm](docs/public/HOSTING.md#kubernetes-helm).
Сайт и кабинет находятся в отдельном [репозитории consolecrypt-site](https://git.evsikov.net/publics/consolecrypt-site) и выпускаются независимо от API.

**Предварительная версия.** Функции продолжают развиваться. Перед обновлением
сохраняйте зашифрованную резервную копию хранилища и комплект восстановления.

## Возможности

- **SSH и терминал:** вкладки, jump-хосты, туннели, поиск и действия с выделенным текстом.
  Прокрутка при выделении, автоматическое продолжение выделения за краями экрана
  и счётчик открытых терминалов помогают работать с длинными журналами.
- **RDP:** удалённые рабочие столы Windows во вкладках, выбор SSH/RDP при добавлении хоста, обмен текстом и доступ к выбранной локальной папке по явному разрешению. [Руководство RDP](docs/public/RDP.md).
- **Хосты и группы:** карточки или список, поиск, наследование настроек подключения.
  В глобальном поиске `Ctrl+K` / `⌘K` найдите хост по имени или IP и нажмите Enter
  для SSH-подключения с обычной проверкой ключа сервера.
- **SFTP:** передача и просмотр файлов, редактирование во внешнем редакторе на компьютере.
- **Сниппеты:** свои команды, наборы команд и синхронизация через хранилище.
- **ИИ-помощник:** Ollama, LM Studio и совместимые с OpenAI API провайдеры; просмотр команд перед выполнением.
- **Настройка внешнего вида:** светлая и тёмная темы, синий акцент по умолчанию,
  размер текста, собственные цвета терминала, всплывающая или закреплённая правая панель.
- **Совместная работа:** выбранные хосты, сниппеты, коллекции и отдельные секреты,
  проверка устройств, роли и отзыв доступа. Отдельные вкладки показывают полученные и опубликованные объекты; добавление устройств вынесено в собственный блок. [Руководство общего доступа](docs/public/SHARING.md).
- **Linux:** пакеты DEB/RPM для x86-64, системная ключница Secret Service и выбор внешнего SFTP-редактора. [Установка и сборка](docs/public/BUILD_LINUX.md).
- **iOS:** предварительный порт с нативным Rust-ядром, SSH/SFTP и Keychain;
  [сборка Simulator и подпись для телефона](docs/public/BUILD_IOS.md).
- **Защита данных:** зашифрованное хранилище и резервные копии, автоблокировка,
  подтверждение новых SSH-ключей хостов и устройств синхронизации.
- **Android:** интерфейс для телефона, биометрическая разблокировка при доступности
  и переключатель разрешения скриншотов.

SSH- и RDP-соединения идут напрямую к вашим серверам. Сервер синхронизации получает
зашифрованные записи и не располагает ключами для их расшифровки. ИИ-помощник
опционален: выбранный провайдер получает отправленный вами запрос и разрешённый
контекст; не включайте в него секреты.

## Скачать и установить

**Текущий релиз — [0.3.0](https://git.evsikov.net/publics/consolecrypt/-/releases/v0.3.0)** для macOS, Windows, Android и Linux x86-64.

| Платформа | Версия | Скачать | Установка |
|---|---|---|---|
| macOS 12+ · Apple Silicon и Intel | **0.3.0+1380** | [DMG](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1380-macos-universal.dmg) | Перенесите ConsoleCrypt в Applications и запускайте оттуда |
| Windows 10/11 · x64 | **0.3.0+1379** | [EXE](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1379-windows-x64-setup.exe) · [ZIP](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1379-windows-x64-portable.zip) | Запустите установщик без прав администратора или распакуйте ZIP целиком |
| Android 11+ · ARM64 | **0.3.0+1381** | [APK](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1381-android-arm64.apk) | Скачайте APK на телефон и установите |
| Linux x86-64 · Ubuntu 22.04 / Debian 12 | **0.3.0+1378** | [DEB](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1378-linux-x64.deb) | Установите через `apt`; нужна разблокированная ключница Secret Service |
| Linux x86-64 · Fedora 43 | **0.3.0+1378** | [RPM](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1378-linux-x64.rpm) | Установите через `dnf`; нужна разблокированная ключница Secret Service |

iOS Simulator на Mac: **0.3.0+1377**, [ZIP, часть 1](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1377-ios-simulator-universal.zip.001) · [часть 2](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1377-ios-simulator-universal.zip.002) · [инструкция](docs/public/BUILD_IOS.md). Для настоящего iPhone нужна отдельная подпись; ZIP на телефон не устанавливается.

[Контрольные суммы SHA-256 для всех пакетов](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/SHA256SUMS-0.3.0.txt) ·
[Проверки выпуска 0.3.0](docs/public/RELEASE_0_3_0.md) · [Как работают обновления](docs/public/UPDATES.md).

На Windows, macOS и Android в настройках есть автопроверка при запуске, ручная проверка и скачивание новой версии с проверкой подписи списка релизов. На Linux скачайте новый DEB/RPM и обновите приложение через менеджер пакетов; автоматической установки и подписанной ленты обновлений Linux в этом выпуске нет. Для перехода с 0.1.10 или более ранней версии установите этот релиз вручную. На macOS при переходе со старой кнопки обновления скачайте DMG через браузер и один раз замените приложение в Applications. Это нужно и для любой новой копии, если она была скачана старым приложением и не открывается: повторное скачивание старой кнопкой может снова перенести тот же запрет запуска. Начиная с 0.2.2 следующие обновления сохраняются через системный диалог «Сохранить и открыть», чтобы macOS разрешала запуск новой копии. Профили и хранилища сохраняются.

Номер после `+` увеличивается при каждой сборке, поэтому он различается между
платформами. Бинарные файлы публикуются через GitLab Releases и не хранятся в Git.

На Windows программа устанавливается в `%LOCALAPPDATA%\Programs\ConsoleCrypt`.
Хранилища находятся в `%LOCALAPPDATA%\consolecrypt\ConsoleCrypt\data` и сохраняются
при обновлении и удалении приложения.

macOS-сборки пока без нотариализации Apple, APK использует тестовую подпись,
Windows-установщик без цифровой подписи издателя. Это пакеты для предварительного
тестирования, не публикация в App Store, Google Play или Microsoft Store.

## Первое подключение

1. Откройте приложение и выберите локальный профиль либо подключение к своему серверу синхронизации.
2. Создайте хранилище, задайте парольную фразу и сохраните комплект восстановления в надёжном месте.
3. В разделе **Учётные данные** добавьте способ SSH-аутентификации.
4. В **Хостах** нажмите **Новый хост**, укажите адрес, имя пользователя, порт и учётные данные.
5. Нажмите карточку хоста или значок терминала. При первом подключении сверьте отпечаток SSH-ключа с администратором сервера.

В левой панели оставлен раздел **Хосты**: группы, вложенные группы и их настройки доступны внутри него. В меню хоста можно открыть SFTP, изменить хост или перенести его в группу.

Встроенный SSH запрашивает UTF-8 для ввода кириллицы и корректного удаления символов. После обновления откройте новую SSH-сессию. Если сервер запрещает запросы окружения, не поддерживает `C.UTF-8` или принудительно задаёт `LC_ALL=C`, администратору нужно настроить UTF-8 на сервере. Явно выбранный системный OpenSSH backend не изменён.
Сниппеты и ИИ доступны в правой панели на компьютере и через мобильную навигацию
на телефоне. После работы с панелью одного клика по терминалу достаточно для
продолжения ввода. Длинный вывод можно прокручивать колесом или трекпадом, сохраняя
выделение; удерживайте левую кнопку мыши за верхним или нижним краем терминала,
чтобы продолжить выделение в буфере прокрутки. Параметры редактора, интерфейса и терминала находятся в настройках.

Пароль учётной записи, парольная фраза хранилища и пароль системной связки ключей
macOS — разные вещи. После обновления неподписанной сборки macOS может заново
запросить доступ к связке ключей. Не удаляйте системную связку ключей для устранения
такого запроса. Биометрия не заменяет сохранённую парольную фразу и комплект восстановления.

## Собрать из исходников

Для чтения публичного репозитория SSH-ключ не нужен:

```sh
git clone https://git.evsikov.net/publics/consolecrypt.git
cd consolecrypt
```

Для работы по SSH:

```sh
git clone ssh://git@git.evsikov.net:2222/publics/consolecrypt.git
```

На Windows после установки инструментов из [пошаговой инструкции](docs/public/BUILD_WINDOWS.md):

```powershell
.\client\scripts\build-windows.ps1 -Installer -NoCli
```

Готовый установщик, ZIP и SHA-256 появятся в `dist/windows/`.
[Сборка macOS/Android/Linux и запуск своего сервера →](docs/public/BUILDING.md)

## Состав проекта

| Каталог | Содержимое |
|---|---|
| `client/flutter` | Интерфейс, нативные оболочки и ресурсы |
| `client/rust` | Криптография, SSH/SFTP, хранилище, синхронизация и bridge |
| `crates` | Общие модели и протокол клиента и сервера |
| `server` | Сервер синхронизации, миграции PostgreSQL, Docker и Helm |
| `client/scripts`, `client/packaging` | Сборка и упаковка приложений |

Зависимости загружаются при сборке; lock-файлы включены в репозиторий.
Дистрибутивы и история релизов — в GitLab Releases.
[Как выпускать новую версию →](docs/public/RELEASING.md)

## English

ConsoleCrypt is an SSH/RDP workspace with host groups, snippets, an optional AI
assistant and encrypted sync/team sharing. Version **0.3.0** adds Windows remote
desktops, native fullscreen controls, explicit text clipboard and selected-folder
access. Downloads and exact platform build numbers are in the table above; [Linux installation](docs/public/BUILD_LINUX.md) explains `apt`/`dnf` and
the required existing unlocked persistent Secret Service keyring.
Linux updates are manual package-manager updates. Signed in-app update metadata
is supported on Windows, macOS and Android, not Linux or iOS.

The iOS **0.3.0+1377** ZIP parts are for Simulator on Mac, not an iPhone IPA.
macOS is not Apple-notarized, Windows installers do not carry an issuer signature,
and Android retains its preview signer.
See [release checks and limits](docs/public/RELEASE_0_3_0.md).

If an old macOS in-app downloader produced an app that cannot launch, download
the DMG once through your browser, quit ConsoleCrypt, replace it in Applications,
and open that copy. Keep your profiles, vaults and Keychain. Subsequent updates
use the system **Save and open** dialog; security protections stay enabled.

## Автор и лицензии

**Alexander Evsikov** · [i@evsikov.net](mailto:i@evsikov.net)

Клиент, сервер, общие библиотеки и документация ConsoleCrypt:
[GNU AGPL-3.0-only](LICENSE). [Подробности лицензирования →](docs/public/LICENSING.md)
Сторонние компоненты сохраняют свои лицензии.
Уязвимости: [порядок сообщения](SECURITY.md).
