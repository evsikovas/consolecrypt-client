# Обновления ConsoleCrypt

С версии 0.1.12 в настройках есть раздел **Обновления**:

- **Проверять обновления при запуске** — сохранённый на этом устройстве
  переключатель. Когда он выключен, запросов при запуске нет.
- **Проверить обновления** — работает независимо от переключателя.
- **Скачать и установить** — появляется для новой версии. Показывает ход
  скачивания и запускает установщик после проверки файла.

Проверка при запуске не устанавливает ничего сама и не прерывает SSH-сеансы.
Перед установкой есть подтверждение. Профили, хранилища и настройки сохраняются.

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
[сайта через браузер](https://consolecrypt.evsikov.net/download?lang=ru)
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

Публикация и ключ подписи управляются вне публичного репозитория. Новый релиз
становится доступен после публикации пакетов всех платформ и списка SHA-256.
Сервис проверяет релизы каждые пять минут.

## English

Open **Settings → Updates** to enable/disable startup checks, check manually,
and download a newer version. Checking never installs anything or interrupts
SSH sessions. Installation requires confirmation and preserves profiles and vaults.
Windows runs the per-user installer, Android uses the system installation
confirmation. On macOS, choose a new DMG filename or folder in the system
**Save and open** dialog, quit ConsoleCrypt, and replace the app in Applications.
Cancelling the save dialog leaves the verified download ready for retry;
existing files are never overwritten. Signed release metadata and SHA-256 verification protect
the installer; account credentials and vault data are never sent.

If ConsoleCrypt 0.2.1 or an older in-app updater on macOS produced an app that cannot open,
download the latest DMG once through your browser and replace the app in Applications.
The old downloader set a sandbox no-user-consent quarantine mark. Version 0.2.2
uses a system-approved save location and keeps App Sandbox and Gatekeeper enabled.
A newer version downloaded by the old app can inherit the same restriction.
Downloading again with the old app repeats the problem even with a new installer;
the fixed downloader becomes available after the first successful launch of the
replacement app.
Do not delete your vault or Keychain.
