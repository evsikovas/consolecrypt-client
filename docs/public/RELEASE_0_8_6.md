# ConsoleCrypt 0.8.6

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

**Общий доступ к подключениям баз данных.** Сохранённым подключением теперь
можно поделиться с проверенным устройством коллеги — так же, как хостами и
контекстами Kubernetes. Нужен сервер синхронизации 0.1.15 или новее с
включённой функцией; на `sync.consolecrypt.dev` она включена. Со старым
сервером пункт «Поделиться» не показывается.

- «Поделиться» в меню подключения в разделе «Базы данных». Перед отправкой
  видно, какие поля получит коллега: движок, адрес, порт, база, пользователь,
  TLS, лимиты и пометка «прод».
- Группа, история запросов, настройки ИИ и разрешения сеанса не передаются.
  Пароль и ключ клиента уходят только по явной отметке и только если сервер
  поддерживает общие секреты.
- Маршрут через SSH-хост, jump-профиль, прокси или туннель не передаётся:
  получатель выбирает свой. У проброса Kubernetes сохраняются пространство
  имён и нагрузка, у Docker — имя контейнера. Такое подключение нельзя
  сохранить как прямое.
- Общие подключения видны в разделе «Базы данных» с меткой «Общее» (свои —
  «Вы поделились»). Оттуда можно принять подключение, сохранить личную копию,
  управлять участниками и отозвать доступ.
- Копия прод-подключения остаётся прод-подключением и открывается только на чтение.
- Подключения можно добавлять в общие коллекции.

Отзыв доступа не отменяет уже скопированный пароль — смените его на сервере
базы данных.

**Важно после обновления.**

- Разблокировка устройством (Touch ID, Windows Hello, отпечаток) выключена,
  пока вы один раз не откроете хранилище паролем. Затем приложение перешифрует
  локальную базу и предложит включить её снова — теперь ключ выдаёт сама
  система только после биометрии.
- На Windows обновление с прошлой версии через баннер может не сработать —
  установите эту версию вручную с сайта. Дальше обновления через баннер
  работают: приложение закрывается, установщик обновляет и запускает его снова.

**Базы данных.**

- «Разблокировать прод» в верхней панели: запись в прод-базу на 5, 15 или 30
  минут после пароля хранилища и ввода имени подключения. Каждое изменение
  по-прежнему подтверждается, по истечении времени запись снова закрыта,
  такие запросы помечены в истории.
- Двойной клик по ячейке — правка прямо в таблице (Enter, Esc, Tab), если
  запись разрешена. Тип значения берётся из колонки, выбирать его не нужно.
- ⌘↵ / Ctrl+Enter без выделения выполняет оператор на строке курсора, в том
  числе многострочный.
- DDL таблицы — с подсветкой, номерами строк, кнопками «Копировать» и
  «Открыть в редакторе».
- Клик выделяет ячейку; строки выделяются кликом по номеру, с Shift или ⌘.
  Копирование, экспорт и ИИ работают с выделенным.
- PostgreSQL больше не переходит без предупреждения на соединение без TLS:
  без шифрования — только через туннель или на этом компьютере; пароль
  привязывается к TLS-соединению. В рабочей области виден статус шифрования.

**Безопасность.**

- Защита от входа с нового устройства (сервер синхронизации 0.1.16): письмо о
  каждом новом устройстве со ссылкой «Это не я», уведомление на ваших
  устройствах, пометка «не подтверждено». Новое устройство 14 дней не может
  отзывать доверенные, выдача данных для разблокировки ограничена.
- Docker: пароли скрыты не только в переменных, но и в адресах, командах,
  метках, JSON и просмотре compose; полный текст — после пароля хранилища.
  Сохранение compose-файла защищено от подмены файлов.
- Обновлены библиотеки TLS, зависимости проверяются автоматически.

**Kubernetes и Windows.**

- Кнопки Flux называются понятно — «Синхронизировать сейчас»,
  «Приостановить/Возобновить синхронизацию» — и объясняют, что делают.
- Пути Windows показываются обычными, без префикса `\\?\`.

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.

**Sharing database connections.** A saved connection can now be shared with a
verified colleague's device, like hosts and Kubernetes contexts. It needs sync
server 0.1.15 or later with the feature enabled; `sync.consolecrypt.dev` has
it on. With an older server the Share action is hidden.

- Share from the connection menu in Databases. The preview shows exactly what
  the colleague receives: engine, address, port, database, user, TLS, limits
  and the production marker.
- Groups, query history, AI settings and session permissions are never
  shared. The password and client key are included only when explicitly
  selected and only if the server supports shared secrets.
- A route through an SSH host, jump profile, proxy or tunnel is not shared:
  the recipient picks their own. A Kubernetes forward keeps its namespace and
  workload, a Docker forward its container name. Such a connection cannot be
  saved as direct.
- Shared connections appear in Databases with a "Shared" badge ("Shared by
  you" for your own). From there you accept, save a personal copy, manage
  members and revoke access.
- A copy of a production connection stays a production connection and opens read-only.
- Connections can be added to shared collections.

Revoking access does not invalidate an already copied password; change it on
the database server.

**After updating.**

- Device unlock (Touch ID, Windows Hello, fingerprint) is off until you open
  the vault with its passphrase once. The app then re-encrypts the local
  database and offers to turn it back on — the key is now released only by the
  operating system after biometric confirmation.
- On Windows, updating from the previous version through the banner may fail —
  install this version manually from the website. From now on the banner
  works: the app closes, the installer updates it and starts it again.

**Databases.**

- "Unlock production" in the header allows writes to a production database for
  5, 15 or 30 minutes after the vault passphrase and typing the connection
  name. Every change is still reviewed, writes close again when time runs out,
  and such queries are marked in history.
- Double-click a cell to edit it in the grid (Enter, Esc, Tab) when writes are
  allowed. The value type comes from the column.
- ⌘↵ / Ctrl+Enter without a selection runs the statement on the caret's line,
  including multi-line statements.
- Table DDL is highlighted, with line numbers, Copy and Open in editor.
- A click selects a cell; rows are selected by their number, with Shift or ⌘.
  Copy, export and AI use the selection.
- PostgreSQL no longer silently drops to an unencrypted connection: plaintext
  only through a tunnel or on this computer, and the password is bound to the
  TLS connection. The workspace shows the encryption status.

**Security.**

- Protection against sign-ins from new devices (sync server 0.1.16): an email
  for every new device with a "This wasn't me" link, a notice on your
  devices and a "not confirmed" marker. A new device cannot revoke trusted
  ones for 14 days, and unlock material is rate-limited.
- Docker: secrets are hidden not only in variables but also in URLs, commands,
  labels, JSON and the compose view; full text needs the vault passphrase.
  Saving compose files is protected against file swaps.
- TLS libraries are updated and dependencies are checked automatically.

**Kubernetes and Windows.**

- Flux actions have plain names — Sync now, Pause/Resume sync — and explain
  what they do.
- Windows paths are shown without the `\\?\` prefix.

See the [Databases guide](DATABASES.md) and the [Sharing guide](SHARING.md).
