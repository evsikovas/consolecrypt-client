# Обновления ConsoleCrypt

С версии 0.1.12 на Windows, macOS и Android (на Linux — с 0.9.4, см. ниже) в настройках есть раздел **Обновления**:

- **Проверять обновления при запуске** — сохранённый на этом устройстве
  переключатель. Когда он выключен, запросов при запуске нет.
- **Проверить обновления** — работает независимо от переключателя.
- **Скачать и установить** — появляется для новой версии. Показывает ход
  скачивания и запускает установщик после проверки файла.

Проверка при запуске не устанавливает ничего сама и не прерывает SSH-сеансы.
Перед установкой есть подтверждение. Профили, хранилища и настройки сохраняются.

**Переход со старого адреса обновлений:** на 19 октября 2026 года
`updates.consolecrypt.evsikov.net` недоступен. Сборки, использующие этот адрес,
не смогут получить новый релиз через встроенную проверку. Скачайте установщик
со [страницы загрузок](https://consolecrypt.dev/download?lang=ru), закройте
ConsoleCrypt и обновите существующую установку. На macOS замените приложение
в Applications. Не удаляйте приложение с его данными, хранилище или системную
связку ключей: ручное обновление сохраняет их. Новые сборки используют
`updates.consolecrypt.dev`. Сначала сохраните резервную копию и комплект восстановления.

На Windows запускается установщик для текущего пользователя; приложение
закрывает соединения и базу данных перед выходом. На Android необходимо
разрешить установку обновлений из ConsoleCrypt и подтвердить системное окно.
На macOS после загрузки выберите новую папку или имя DMG в системном диалоге
**Сохранить и открыть**. Сохранённый файл проверяется по SHA-256 и открывается:
завершите приложение, перетащите новую копию в Applications и подтвердите замену.
Отмена диалога не запускает установку; повторная попытка использует проверенную
загрузку. Существующие файлы не перезаписываются.

**Переход с ConsoleCrypt 0.2.1 и более ранних версий на macOS:** если после обновления появляется
«Не удаётся открыть программу», скачайте актуальный DMG с
[сайта через браузер](https://consolecrypt.dev/download?lang=ru)
и один раз замените приложение в Applications. Старый загрузчик помечал DMG как
созданный без согласия пользователя; эта метка переносилась на приложение.
Такое возможно и для копии новой версии, скачанной старым загрузчиком.
Повторное скачивание кнопкой в старом приложении повторяет проблему даже с
новым установщиком: исправление загрузчика начинает работать только после
первого запуска исправленной копии. В версиях начиная с 0.2.2 используется
системное разрешение на сохранение установщика. Защита
macOS и sandbox остаются включены, хранилище и связку ключей удалять не нужно.

Список релизов подписан Ed25519; клиент содержит только открытый ключ проверки.
Файлы проверяются по SHA-256 после скачивания и ещё раз перед запуском.
Неверная подпись, повреждённый файл, просроченный список и попытка отката
блокируют установку. Хранилища, пароли и токены аккаунта не отправляются
серверу обновлений. Он не имеет доступа к серверу синхронизации или его БД.

Публикация и ключ подписи управляются вне публичного репозитория. Обновление
становится доступно после публикации пакета поддерживаемой платформы,
контрольных сумм и подписанных метаданных.
Зеркало обновлений проверяет релизы каждые 15 минут. До появления обоих
подписанных списков нового релиза оно сохраняет предыдущую проверенную версию.

## Linux и iOS

С версии 0.9.4 копия, установленная из DEB или RPM (пакет `consolecrypt` в
`/opt/consolecrypt`), обновляется так же, как на других компьютерах: проверка
при запуске, «Проверить обновления» и «Скачать и установить». У Linux своя лента
`https://updates.consolecrypt.dev/linux-stable.json`, подписанная отдельным
ключом Ed25519; ключ Windows, macOS и Android её не подписывает, и наоборот.
Пакет проверяется по подписи и SHA-256, затем система спрашивает пароль
администратора (`pkexec`, polkit), и менеджер пакетов ставит его: `apt-get` (или
`dpkg`) для DEB, `dnf`, `zypper` или `rpm` для RPM. Профили, хранилища и системная
ключница сохраняются; новая версия открывается кнопкой «Перезапустить». Если
закрыть окно пароля, установка отменяется, а проверенный пакет остаётся для
повторной попытки. Без `pkexec` или при ошибке менеджера пакетов установите пакет
вручную со [страницы загрузок](https://consolecrypt.dev/download?lang=ru), как
описано в [инструкции Linux](BUILD_LINUX.md).

Версию 0.9.4 на Linux один раз установите вручную: более ранние версии ленту
Linux не читают. Копия, запущенная не из пакета (сборка из исходников,
скопированная папка), по-прежнему показывает только страницу загрузок.

На iOS настольный загрузчик отключён. Новый Simulator ZIP устанавливается
по [инструкции](BUILD_IOS.md); настоящий iPhone требует отдельной подписи.

## English

On Windows, macOS and Android, open **Settings → Updates** to enable/disable startup checks, check manually,
and download a newer version. Checking never installs anything or interrupts
SSH sessions. Installation requires confirmation and preserves profiles and vaults.

**Moving from the old update address:** as of 19 October 2026,
`updates.consolecrypt.evsikov.net` is unavailable. Builds using that address
cannot discover new releases through their in-app updater. Download the installer
from the [download page](https://consolecrypt.dev/download?lang=en), quit
ConsoleCrypt and update the existing installation. On macOS, replace the app in
Applications. Keep the app's data, vault and system keychain; do not uninstall
them. A manual update preserves them. New builds use `updates.consolecrypt.dev`.
Save a backup and recovery kit first.

Windows runs the per-user installer, Android uses the system installation
confirmation. On macOS, choose a new DMG filename or folder in the system
**Save and open** dialog, quit ConsoleCrypt, and replace the app in Applications.
Cancelling the save dialog leaves the verified download ready for retry;
existing files are never overwritten. Signed release metadata and SHA-256 verification protect
the installer; account credentials and vault data are never sent.
The update mirror checks every 15 minutes and retains the previous verified
release until both signed feeds for the new release are available.

If ConsoleCrypt 0.2.1 or an older in-app updater on macOS produced an app that cannot open,
download the latest DMG once through your browser and replace the app in Applications.
The old downloader set a sandbox no-user-consent quarantine mark. Version 0.2.2
uses a system-approved save location and keeps App Sandbox and Gatekeeper enabled.
A newer version downloaded by the old app can inherit the same restriction.
Downloading again with the old app repeats the problem even with a new installer;
the fixed downloader becomes available after the first successful launch of the
replacement app.
Do not delete your vault or Keychain.


From 0.9.4, a copy installed from the DEB or RPM (the `consolecrypt` package
in `/opt/consolecrypt`) updates like the other desktops: startup checks, Check
for updates and Download and install. Linux has its own feed,
`https://updates.consolecrypt.dev/linux-stable.json`, signed with a separate
Ed25519 key; the Windows/macOS/Android key cannot sign it and vice versa. The
package is checked against the signature and SHA-256, then the system asks for
the administrator password (`pkexec`, polkit) and the package manager installs
it: `apt-get` (or `dpkg`) for DEB, `dnf`, `zypper` or `rpm` for RPM. Profiles,
vaults and the system keyring are kept; Restart opens the new version. Closing
the password dialog cancels and keeps the verified package for a retry. Without
`pkexec`, or when the package manager fails, install the package manually from
the [download page](https://consolecrypt.dev/download?lang=en).

Install 0.9.4 on Linux manually once: earlier versions do not read the Linux
feed. A copy not run from the package (a source build or a copied folder) still
shows only the download page. Keep your profiles and system keyring. On iOS, install a new Simulator archive
manually; an actual iPhone still requires separate signing.
