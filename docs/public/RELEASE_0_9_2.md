# ConsoleCrypt 0.9.2

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.
Новые функции общего доступа и проверки ключей работают с сервером синхронизации 0.2.2.

**Новые базы данных.**

- **SQLite** — файлы на этом компьютере, а также на SSH-хосте, в Docker-контейнере или
  поде Kubernetes через программу `sqlite3` (без копирования файла). По умолчанию только
  чтение. Дерево: таблицы с колонками «имя (ТИП)», ключами, внешними ключами, ссылками и
  триггерами; представления, индексы, последовательности, триггеры таблиц и типы данных.
  DDL, план запроса, экспорт, правка ячеек.
- **Apache Cassandra и ScyllaDB (CQL)** — структура keyspace, таблиц и пользовательских
  типов, DDL, постраничные результаты, правка ячеек по полному первичному ключу, экспорт,
  режим «только чтение» для прода; все маршруты (SSH, туннель, Kubernetes, Docker).
  Через туннель запросы идут через один узел кластера.
- Подключения SQLite и Cassandra в этом выпуске хранятся только на этом устройстве:
  не синхронизируются и не передаются в общий доступ.

**Работа с базами данных.**

- **Конструктор таблиц** для PostgreSQL, MySQL/MariaDB и ClickHouse, создание коллекций
  MongoDB: столбцы, ключи, внешние ключи, индексы, проверки и параметры движка. Изменения
  копятся в черновике и видны в дереве; «Сохранить изменения» показывает итоговый SQL и
  выполняет его после подтверждения (в PostgreSQL — одной транзакцией).
- **Панель результата:** «Обновить» с автообновлением, «Сохранить» (с «Показать SQL») и
  «Отменить», добавление, дублирование и удаление строк, переход по строкам, экспорт и
  импорт, размер порции и «Загрузить все строки», строка состояния. Изменения строк
  копятся до «Сохранить» и выполняются одной транзакцией по ключу. Виды «Таблица»,
  «Текст» и «Запись»; значки типов и ключей в заголовках.
- **Импорт CSV** в таблицы PostgreSQL, MySQL и MariaDB: предпросмотр, кодировки,
  автоопределение разделителя, сопоставление столбцов с проверкой типов, вставка,
  пропуск или обновление по ключу, отчёт об ошибках, прогресс и отмена.
- Кнопка **«Сниппеты»** (⌘J / Ctrl+J) вставляет сниппет под текстом редактора.
- ClickHouse: проверка «только чтение» учитывает регистр имён функций, как сам ClickHouse.

**Безопасность.**

- **Ключ хранилища под контролем устройства.** Устройство запоминает отпечаток ключа
  хранилища и не принимает от сервера другой ключ: при несовпадении показывается
  предупреждение, синхронизация останавливается.
- **Одобрение нового устройства:** оба устройства показывают общий код пары, а подпись
  одобрения охватывает ключ хранилища. Со старым сервером ключ помечается
  «не подтверждён» до ввода пароля хранилища.
- **Проверка коллег в общем доступе:** ключи коллег закрепляются и проверяются кодом
  безопасности или QR-кодом; при смене ключей новый доступ блокируется. Секреты,
  контексты Kubernetes и подключения к БД по умолчанию выдаются только проверенным
  получателям. Код добавления своих устройств вводится с экрана другого устройства.
- **Подтверждение с Touch ID, Windows Hello или отпечатком** вместо пароля хранилища,
  если на устройстве включена разблокировка через ОС. Смена пароля, комплект
  восстановления, резервные копии, одобрение устройств и безопасность аккаунта
  по-прежнему требуют пароль. Настройка «Подтверждать действия биометрией».
- **Буфер обмена:** скопированные секреты не попадают в историю буфера Windows и
  облачный буфер, в универсальный буфер обмена macOS и менеджеры буфера Linux, а на
  Android удаляются по сроку даже из свёрнутого приложения. Выделение в терминале и
  логах тоже не попадает в историю буфера.

**Синхронизация** (сервер 0.2.2).

- Клиент соблюдает лимиты сервера: при ограничении запросов статус показывает
  «Сервер временно ограничил синхронизацию, продолжим в ЧЧ:ММ» вместо ошибки.
- Если превышен предел объектов хранилища, непринятые изменения остаются на устройстве,
  а страница «Синхронизация» предлагает «Повторить» после очистки.
- Понятные сообщения о пределе устройств и о слишком большом числе новых устройств
  за сутки.

**Заметки.**

- Кнопка «Сохранить» (⌘S / Ctrl+S) и статус сохранения.
- Заметки хоста — из карточки хоста (без подключения) и выбором хоста в панели.
- Заметка, объект которой удалён или недоступен, остаётся с пометкой «Не привязана»,
  её можно привязать заново. Заметки к объектам команд — личные.
- Повторяющиеся ошибки показываются одним уведомлением.

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.
New sharing and key-verification features work with sync server 0.2.2.

**New databases.**

- **SQLite** — files on this computer, or on an SSH host, in a Docker container or a
  Kubernetes Pod via the `sqlite3` program (no file copying). Read-only by default. The
  tree shows tables with "name (TYPE)" columns, keys, foreign keys, references and
  triggers, plus views, indexes, sequences, table triggers and data types. DDL, query
  plans, export and cell editing.
- **Apache Cassandra and ScyllaDB (CQL)** — keyspace, table and user-type tree, DDL,
  paged results, cell editing by full primary key, export, read-only production, every
  route (SSH, tunnel, Kubernetes, Docker). Through a tunnel, queries use one coordinator.
- In this release SQLite and Cassandra connections stay on this device: they are not
  synced and cannot be shared.

**Working with databases.**

- **Table designer** for PostgreSQL, MySQL/MariaDB and ClickHouse, and MongoDB
  collections: columns, keys, foreign keys, indexes, checks and engine options. Changes
  stay in a draft shown in the tree; Save changes shows the resulting SQL and runs it
  after confirmation (in one transaction on PostgreSQL).
- **Result panel:** refresh and auto-refresh, Save (with Show SQL) and Discard, add,
  duplicate and delete rows, row navigation, export and import, fetch size and Fetch
  all, and a status line. Row changes stay pending until Save and run in one transaction
  by key. Table, Text and Record views; type and key icons in headers.
- **CSV import** into PostgreSQL, MySQL and MariaDB tables: live preview, encodings,
  delimiter detection, column mapping with type checks, insert, skip or upsert by key,
  an error report, progress and cancel.
- A **Snippets** button (⌘J / Ctrl+J) inserts a snippet below the editor text.
- ClickHouse: the read-only check matches function names case-sensitively, as
  ClickHouse does.

**Security.**

- **The vault key is pinned on the device.** Each device remembers its vault key
  fingerprint and refuses a different key from the server: you get a warning and sync
  stops.
- **New-device approval** shows one pairing code on both devices, and the signed
  approval covers the vault key. With an older server the key stays "not confirmed"
  until you enter the vault passphrase.
- **Verified colleagues in sharing:** colleagues' keys are pinned and verified with a
  safety number or QR code; changed keys block new access. Secrets, Kubernetes contexts
  and database connections go only to verified recipients by default. Own-device
  enrollment codes are typed in from the other device's screen.
- **Confirm with Touch ID, Windows Hello or your fingerprint** instead of the vault
  passphrase when device unlock is on. Changing the passphrase, the Recovery Kit,
  backups, approving devices and account security still need the passphrase. Toggle
  "Confirm actions with biometrics".
- **Clipboard:** copied secrets stay out of Windows clipboard history and the cloud
  clipboard, macOS Universal Clipboard and Linux clipboard managers, and on Android they
  are cleared on time even when the app is in the background. Terminal and log
  selections also stay out of clipboard history.

**Sync** (server 0.2.2).

- The client respects server limits: when requests are limited, the status shows "The
  server temporarily limited sync; resuming at HH:MM" instead of an error.
- When a vault reaches its object limit, the changes the server didn't accept stay on
  the device and the Sync page offers Retry after cleanup.
- Clear messages for the device limit and for too many new devices in a day.

**Notes.**

- A Save button (⌘S / Ctrl+S) with save status.
- Host notes from the host card (without connecting) or by choosing a host in the panel.
- A note whose object is gone stays, marked Not linked, and can be attached again.
  Notes about team objects are personal.
- Repeated errors show one notice.

See the [Databases guide](DATABASES.md), the [Sharing guide](SHARING.md) and the
[Clipboard guide](CLIPBOARD.md).
