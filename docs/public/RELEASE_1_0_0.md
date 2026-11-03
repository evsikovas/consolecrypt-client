# ConsoleCrypt 1.0.0

## Русский

Менеджер паролей KDBX встроен в рабочее пространство ConsoleCrypt: локальные базы,
WebDAV, генератор и поиск через палитру команд. Выпуск для Windows, macOS,
Linux DEB/RPM, Android и iOS Simulator на Mac. Simulator — не установщик для iPhone.

- **Пароли:** создание и открытие KDBX 3/4 с мастер-паролем и ключевым файлом,
  папки, карточки, дополнительные поля, редактирование, перенос и корзина.
  Изменения сохраняются автоматически; ПКМ на компьютере и меню записи на
  телефоне позволяют открыть, переместить или удалить запись с подтверждением.
- **WebDAV:** полный HTTPS-путь к KDBX, защищённые подключения в зашифрованной
  синхронизации ConsoleCrypt. Пароль подключения хранится отдельным секретом.
  Контроль версий файла защищает от перезаписи чужих изменений; открытие можно отменить.
- **Генератор:** 8–128 символов, выбор наборов, исключение похожих символов,
  оценка энтропии, вставка в редактор и общий защищённый буфер обмена.
- **Палитра:** `Ctrl+K` / `⌘K` → `пароли:` → поиск карточки → выбор поля для
  копирования. Поиск локальный; секреты не передаются ИИ и не вставляются в терминал автоматически.
- **Быстрое открытие KDBX:** отдельный локальный конверт под системной биометрией
  или PIN устройства; на Windows/macOS также FIDO2 с PRF. Android использует
  системный Keystore и запрос подтверждения. Мастер-пароль остаётся доступен.
- **Телефоны:** адаптивная компоновка, сенсорные меню, устойчивое хранение
  импортированных KDBX и экспорт через системный диалог. Локальный файл на
  телефоне — копия в приложении; WebDAV редактируется непосредственно на сервере.
- **Туннели:** галочка «Сохранить туннель» при пробросе Docker/Kubernetes,
  повторный запуск и остановка из раздела «Туннели». Эти параметры локальны для
  профиля; существующие SSH-туннели продолжают синхронизироваться.
- **Локальный ИИ для Linux:** установка проверенного CPU-runtime и тех же GGUF-моделей,
  выбор модели, рекомендации по ОЗУ, отмена/возобновление загрузки и запуск без облака.
  Пакеты DEB/RPM включают зависимости OpenMP/OpenSSL; веса загружаются отдельно по выбору пользователя.
- **Рабочее пространство:** просмотр пароля в редакторе подключения,
  отдельные логин/пароль/домен RDP, команды в совместном доступе, копирование
  адреса/пользователя хоста, улучшенные поля ввода и вкладки терминала.
  На Windows доступны `Ctrl+V`, `Ctrl+Shift+V` и `Shift+Insert`.

Версии сервера и сайта независимы от клиента. Для KDBX/WebDAV не добавляются
новые HTTP endpoints или миграции сервера: он хранит прежние зашифрованные объекты.
Перед настройкой WebDAV обновите все устройства до 1.0.0: старый клиент может
не сохранить новые поля при изменении общих настроек.

[Менеджер паролей](KEEPASS.md) · [Поддержка платформ](PLATFORM_SUPPORT.md) ·
[Совместимость сервера](https://github.com/evsikovas/consolecrypt-server/blob/main/docs/public/COMPATIBILITY_1_0.md) ·
[Руководство со скриншотами](https://consolecrypt.dev/guide)

## English

KDBX password management is now part of the ConsoleCrypt workspace: local
databases, WebDAV, a generator and command-palette search. Packages target
Windows, macOS, Linux DEB/RPM, Android and iOS Simulator on Mac. The Simulator
archive is not an iPhone installer.

- **Passwords:** create/open KDBX 3/4 with a master password and key file;
  folders, cards, custom fields, editing, moving and the recycle bin.
  Changes save automatically. Desktop right-click and the phone entry menu
  offer opening, moving and deletion with confirmation.
- **WebDAV:** a full HTTPS KDBX URL and connection settings in encrypted
  ConsoleCrypt sync; authentication is a separate encrypted credential.
  File-version checks prevent overwriting another client's changes; opening can be cancelled.
- **Generator:** 8–128 characters, selectable character classes, similar-character
  exclusion, entropy estimate, editor insertion and the shared secure clipboard.
- **Palette:** `Ctrl+K` / `⌘K` → `passwords:` → find a card → select a field
  to copy. Search stays local, secrets are not sent to AI and never execute in a terminal.
- **Quick KDBX unlock:** a separate device-local envelope protected by system
  biometrics/device PIN; Windows/macOS also support FIDO2 with PRF. Android
  uses its Keystore and system prompt. The master password remains available.
- **Phones:** adaptive layout, touch menus, durable imported databases and
  system-picker export. A local mobile database is an in-app copy; WebDAV
  writes directly to the remote file.
- **Tunnels:** save Docker/Kubernetes forwards and restart/stop them from
  Tunnels. These presets stay local to the profile; existing SSH tunnel
  definitions still sync.
- **Local AI on Linux:** a verified CPU runtime, the same GGUF model catalogue,
  RAM guidance, cancellable/resumable downloads and inference without a cloud.
  DEB/RPM declare OpenMP/OpenSSL dependencies; models remain optional downloads.
- **Workspace:** reveal a connection password while editing, separate RDP
  username/password/domain fields, teams in sharing, copy host addresses and
  usernames, improved input fields and terminal tabs. Windows supports
  `Ctrl+V`, `Ctrl+Shift+V` and `Shift+Insert`.

Server and website versions are independent. KDBX/WebDAV needs no new server
HTTP endpoints or migrations: the existing encrypted-object transport is used.
Upgrade all devices to 1.0.0 before adding WebDAV connections: an older client
may drop the new fields when updating shared settings.

[Passwords](KEEPASS.md) · [Platform support](PLATFORM_SUPPORT.md) ·
[Server compatibility](https://github.com/evsikovas/consolecrypt-server/blob/main/docs/public/COMPATIBILITY_1_0.md) ·
[Illustrated guide](https://consolecrypt.dev/guide?lang=en)
