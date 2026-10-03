# ConsoleCrypt для Linux: установка и сборка

Первая Linux-версия предназначена для **x86-64** и графического рабочего стола.
Пакет `.deb` используется в Debian/Ubuntu, `.rpm` — в Fedora.
Пакеты содержат клиент, Rust bridge, Flutter engine, иконку и пункт меню приложений.
Базовая сборка — **Ubuntu 22.04**; установка пакетов и запуск проверены
в изолированных **Debian 12** и **Fedora 43** с X11 и программным рендерингом.
Текущий выпуск — **0.3.0+1378**: [DEB](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1378-linux-x64.deb) ·
[RPM](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1378-linux-x64.rpm).
Сервер синхронизации устанавливается отдельно: [Docker и Kubernetes](HOSTING.md).

## Системная ключница

Клиенту нужны D-Bus текущего сеанса и провайдер **Secret Service** с уже созданной
и разблокированной основной ключницей. Подходит GNOME Keyring: обычно его
разблокирует вход в графический сеанс. В другом окружении настройте совместимый
провайдер Secret Service и разблокируйте ключницу средствами рабочего стола.
Клиент сам не создаёт и не разблокирует ключницу и не сохраняет ключи в открытом
виде при её отсутствии. Запускайте ConsoleCrypt от своего пользователя,
**без `sudo`** и внутри графического сеанса, а не с отдельной SSH-консоли.

## Системный выбор папки для RDP

В версии 0.3 выбор локальной папки использует **XDG Desktop Portal** в D-Bus
вашего графического сеанса. Пакеты DEB/RPM требуют службу `xdg-desktop-portal`.
Сам диалог предоставляет отдельный провайдер **FileChooser**, соответствующий
рабочему столу: `xdg-desktop-portal-gnome` для GNOME,
`xdg-desktop-portal-kde` для KDE Plasma или `xdg-desktop-portal-gtk` для окружений
с GTK. Провайдер обычно входит в состав рабочего стола. Если его нет,
установите подходящий пакет через `apt` или `dnf` и заново войдите в графический
сеанс. ConsoleCrypt не назначает провайдер вместо настроек рабочего стола.

В окружениях wlroots одного `xdg-desktop-portal-wlr` недостаточно для выбора
папки: нужен дополнительный провайдер с FileChooser, например GTK.
Это требование выбранного [RFD XDG backend](https://docs.rs/rfd/0.15.4/rfd/#xdg-desktop-portal-backend).
Не запускайте приложение с `sudo` или отдельным D-Bus другого пользователя.

Проверки установки и запуска версии 0.2.5 ниже не подтверждают работу нового
диалога RDP. Выбор папки и обмен файлами в Linux 0.3 требуют отдельной проверки
готового клиента в графическом сеансе с работающим порталом.

## Установка `.deb`

Скачайте пакет на [странице загрузки](https://consolecrypt.evsikov.net/download?lang=ru).
Откройте терминал в каталоге скачивания и подставьте точное имя полученного файла:

```sh
sudo apt install ./ConsoleCrypt-0.3.0+1378-linux-x64.deb
consolecrypt
```

`apt` устанавливает системные зависимости; можно также открыть пакет графическим
менеджером программ. При отсутствии провайдера Secret Service установите
`gnome-keyring` и войдите заново в графический сеанс.

## Установка `.rpm`

```sh
sudo dnf install ./ConsoleCrypt-0.3.0+1378-linux-x64.rpm
consolecrypt
```

В Fedora также нужен провайдер Secret Service, например `gnome-keyring`.
Не используйте `rpm --nodeps`: проверку библиотек должен выполнять `dnf`.

## Обновление и удаление

В **Настройки → Обновления** показаны текущая версия и адрес новых пакетов.
Скачайте новый пакет и повторите команду `apt install` или `dnf install`.
Устанавливаемая версия содержит номер сборки, поэтому менеджер пакетов различает
повторные сборки одного выпуска. Автоматической замены приложения и подписанной ленты обновлений Linux
в этом выпуске нет.

```sh
# Debian/Ubuntu
sudo apt remove consolecrypt
# Fedora
sudo dnf remove consolecrypt
```

Установка и удаление пакета не изменяют профили в домашнем каталоге или ключницу.
Перед переносом данных между компьютерами сохраните зашифрованную резервную копию
и комплект восстановления. Не удаляйте ключи в системной ключнице вручную.

## Сборка из исходников

Клонируйте весь репозиторий вместе с `crates/`. Нужны Flutter **3.47.5**,
Rust из `rust-toolchain.toml`, Python 3.10+, Clang, CMake, Ninja, pkg-config,
GTK 3 headers, C/C++ compiler, Perl и инструменты `dpkg-deb`/`rpmbuild`.
Для Ubuntu 22.04:

```sh
sudo apt update
sudo apt install clang cmake ninja-build pkg-config libgtk-3-dev libwayland-dev libstdc++-12-dev \
  build-essential perl libssl-dev python3 dpkg-dev rpm
client/scripts/build-linux.sh
```

Скрипт резервирует новый номер сборки, компилирует реальный backend
(`CC_MOCK=false`) и создаёт оба пакета в `dist/linux/` вместе с SHA-256,
точной версией и описанием нативных файлов. Не запускайте одновременно несколько
Flutter-сборок в одном каталоге. Сохраняйте `Cargo.lock` и `pubspec.lock`.

Для воспроизводимого окружения есть
[`client/ci/linux/Dockerfile`](../../client/ci/linux/Dockerfile): Ubuntu 22.04,
проверенная ревизия официального Flutter и закреплённый Rust. GitLab job
`build-linux` использует Docker на уже зарегистрированном Mac runner и монтирует
только подготовленные публичные исходники; домашний каталог, D-Bus, ключницы,
SSH-ключи и Kubernetes-конфигурация в контейнер не передаются.

Образ также содержит Wayland headers для зависимости `ashpd` и службы
`xdg-desktop-portal`/`xdg-desktop-portal-gtk` для собственного изолированного
графического сеанса проверки. Наличие этих пакетов само по себе не подтверждает
открытие диалога: проверке нужны запущенный портал, FileChooser provider и
отдельная проверка выбора папки. GTK-провайдер тестового образа не назначается
пользовательскому рабочему столу.
Перед упаковкой job проходит тесты настоящего Rust bridge и сценарий создания
хранилища, перезапуска и разблокировки через интерфейс. Для них создаются
отдельные Xvfb, D-Bus и зашифрованная тестовая ключница, которые удаляются после
проверки. В журнал попадают только фиксированные этапы и результаты,
без парольных фраз и слов восстановления.

## Проверки и ограничения

GitLab [build-linux 1358](https://git.evsikov.net/publics/consolecrypt/-/jobs/1358)
прошёл две интеграции Rust bridge и один полный сценарий UI: создание хранилища,
сохранение хоста, перезапуск и разблокировка с настоящей тестовой Secret Service.
Готовые DEB/RPM отдельно установлены в Debian 12 и Fedora 43: проверены
зависимости, обновление `0.2.5-79 → 0.2.5-1358`, удаление/повторная установка,
нативные библиотеки и экран приветствия. Проверка сохранности использовала
только синтетические маркеры в отдельном HOME/ключнице; она не подтверждает
миграцию настоящего пользовательского хранилища.


```sh
python3 -m unittest discover -s client/scripts -p test_linux_packaging.py -v
(cd client/rust && cargo test --locked -p cc-platform-core --features os-keychain)
(cd client/flutter && flutter analyze && flutter test)
```

Тесты реальной ключницы запускаются только в отдельном одноразовом окружении:
`client/rust/platform-core/tests/run_linux_secret_service.py` создаёт собственные
сеансы D-Bus и тестовые ключницы. Не запускайте их на ключнице входа пользователя.

Основной SSH backend встроен в клиент. Опциональный backend **System OpenSSH**
дополнительно требует `ssh`; его режим proxy использует OpenBSD-совместимый `nc`
с ключами `-X`/`-x`. Fedora Ncat и другие варианты `nc` с иными параметрами
для этого режима не подходят. Для обычных подключений выбирайте основной backend.

Linux ARM64, Flatpak/AppImage, RPM для RHEL, физический GPU и Wayland пока
не проверены и не входят в подтверждённую матрицу этого выпуска.

Полная лицензия AGPL-3.0-only включена в пакет. Лицензии сторонних компонентов
сохраняются в Flutter notices и вложенных файлах лицензий.


## English

ConsoleCrypt **0.3.0+1378** targets Linux x86-64. The build baseline is Ubuntu
22.04; DEB/RPM installation and an actual welcome-screen launch passed in
isolated Debian 12 and Fedora 43 using software X11 rendering.
Install the linked DEB with `sudo apt install ./ConsoleCrypt-0.3.0+1378-linux-x64.deb`
or the RPM with `sudo dnf install ./ConsoleCrypt-0.3.0+1378-linux-x64.rpm`.
Then run `consolecrypt` as your regular desktop user, without `sudo`.

An existing unlocked default persistent Secret Service keyring and session
D-Bus are required; GNOME Keyring is a suitable provider. The app does not create
or unlock it and has no plaintext-key fallback. Download future DEB/RPM packages
and update with the package manager: Linux has no signed in-app updater feed
or automatic installation. Keep your profiles, encrypted backups and keyring.

CI passed real Rust-core storage and UI restart/unlock tests. Package upgrade,
removal and reinstall preserved owned synthetic HOME/keyring markers only;
this was not a migration test of a real user's vault. Physical GPU, Wayland,
Linux ARM64, RHEL and Flatpak/AppImage remain outside the verified matrix.
The optional System OpenSSH proxy mode requires OpenBSD-compatible `nc`;
the default native SSH backend has no such requirement.

Starting with 0.3, the RDP folder picker uses the session **XDG Desktop Portal**.
The DEB/RPM packages require `xdg-desktop-portal`. Your desktop also needs a
FileChooser provider: `xdg-desktop-portal-gnome` for GNOME,
`xdg-desktop-portal-kde` for KDE Plasma, or `xdg-desktop-portal-gtk` for GTK-based
desktops. Install the provider appropriate to your desktop with `apt`/`dnf`
if it is missing, then log back into the graphical session. ConsoleCrypt does
not override the desktop's provider selection.

The wlroots portal alone does not supply the required FileChooser API; add a
compatible provider, such as GTK. See the [RFD backend requirements](https://docs.rs/rfd/0.15.4/rfd/#xdg-desktop-portal-backend).
The previous 0.2.5 installation/startup checks do not verify the new RDP picker.
Linux 0.3 folder selection and file exchange need separate acceptance with the
packaged client and an active desktop portal.
