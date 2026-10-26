# ConsoleCrypt 0.9.3

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. iOS проверяется на Simulator, но её выпуск пока отложен.
Исправления команд работают со всеми серверами синхронизации с командами (0.2.1 и новее);
сервер 0.2.4 дополнительно запрещает участникам менять чужие ключи команды, разрешает назначать администраторов только владельцу и отправляет приглашения только на подтверждённые адреса.

**Новое.**

- **Комплект восстановления** можно напечатать или сохранить в PDF.
- **Вкладки терминала** можно расположить колонкой у левого края; свёрнутая боковая
  панель запоминается.
- **Разделение терминала на панели:** «Разделить вправо» / «Разделить вниз»
  (Cmd/Ctrl+Shift+\ / Shift+−) открывают рядом ещё одну сессию к той же цели, до четырёх
  панелей во вкладке; Cmd/Ctrl+] переключает панели, разделители перетаскиваются.
- **Порядок вкладок терминала** меняется перетаскиванием (в полосе и в колонке), а в колонке
  ещё и клавишами Alt+↑ / Alt+↓.
- **Импорт ключа SSH из файла:** кнопка «Выбрать файл…» в окне импорта ключа.
- **Пароль к ключу SSH** при подключении спрашивается отдельным окном с именем ключа,
  а не как пароль сервера.
- **Импорт из CSV в SQLite и Cassandra/ScyllaDB.** Мастер импорта теперь работает и с
  этими базами. SQLite через `sqlite3` на хосте и Cassandra пишут без общей транзакции
  (в CQL их нет), поэтому вариант «одна транзакция на весь файл» для них не
  показывается. В Cassandra «добавлять» не перезаписывает существующие строки
  (`INSERT … IF NOT EXISTS`). «Создать таблицу из CSV…» доступно и для SQLite.
- **Конструктор таблиц для SQLite и Cassandra/ScyllaDB.** Таблицы создаются и меняются
  без ручного SQL и CQL. Изменения SQLite, которые не умеет `ALTER TABLE`, пересоздают
  таблицу одной транзакцией с сохранением строк и индексов; таблицу с триггерами или
  вычисляемыми столбцами конструктор не пересоздаёт. Для Cassandra задаются ключ
  партиции, кластерные столбцы с порядком, вторичные индексы, TTL и `gc_grace_seconds`.
- **История разговоров с ИИ.** Кнопка «История разговоров» в чате показывает разговоры
  этой сессии и сохранённые в хранилище (настройка «Хранить разговоры с ИИ в
  хранилище»); их можно продолжить или удалить. Продолженный разговор снова передаёт
  модели прежние ходы, со скрытыми, как и раньше, данными: раньше модель начинала его с
  пустого контекста.

- **Исходники сервера в «О программе».** Для синхронизированного профиля окно показывает
  ссылку на исходный код вашего сервера синхронизации (требование AGPL; сервер сообщает её
  в `/v1/meta`).

- **SQLite: стартовые сниппеты и импорт из строки.** Набор из 15 сниппетов SQLite (схема,
  индексы, внешние ключи, проверка целостности, размер файла, план запроса, ANALYZE,
  VACUUM). Подключения импортируются из строк `sqlite:///путь.db` и `cassandra://` /
  `scylla://`.

**Исправления.**

- **SFTP: восстановление правок на других хостах.** Подключение, открытое для
  восстановления несохранённых правок на хосте, которого нет в браузере, теперь
  общее для правок этого хоста и закрывается вместе с последней из них, а не живёт до
  выхода из приложения.
- **Правка файлов по SFTP: бэкапы на Windows и Linux.** Каталог рабочих копий помечается
  `CACHEDIR.TAG` и `.nobackup`, поэтому borg, restic, tar, Duplicity, Back In Time и
  другие программы резервного копирования, которые учитывают эти метки, его пропускают
  (на macOS он, как и раньше, исключён из Time Machine).
- **SQLite: VACUUM.** Подтверждённый `VACUUM` в разделе «Базы данных» больше не
  отказывает: раньше его блокировал запрет подключать к сессии другие базы.
- **Синхронизация удалённого хранилища.** Если хранилище удалено на сервере, синхронизация
  останавливается с понятной причиной («хранилище не найдено на сервере»), а не повторяет
  попытки бесконечно.

- **Синхронизация подключений Cassandra/ScyllaDB и SQLite через хост.** Пункт «Синхронизировать
  через хранилище» переносит такое подключение с паролем в хранилище, если все устройства
  аккаунта работают на 0.9.2 или новее (с сервером 0.2.4 приложение видит их версии и
  перечислит те, что нужно обновить). Файл SQLite на этом компьютере остаётся только здесь.
- **Напоминания при закрытом приложении.** Напоминания заметок и задач приходят системным
  уведомлением «Напоминание из ConsoleCrypt» — без текста заметки, центр уведомлений не
  шифруется. На Android время может сдвинуться на несколько минут, на Linux напоминания
  по-прежнему работают только при открытом приложении.
- **SFTP: перетаскивание в Finder и Проводник.** На macOS и Windows файлы из списка хоста
  перетаскиваются прямо в Finder или Проводник: файл скачивается во временную папку,
  передаётся системе и удаляется, ход виден в «Передачах». Папки — по-прежнему командой
  «Скачать». Перетаскивание внутри приложения и из Finder/Проводника работает как раньше
  (на Linux — тоже, но без перетаскивания наружу).
- **Локальный ИИ: своя модель GGUF.** «Добавить свою модель (GGUF)…» копирует выбранный
  файл в папку моделей со сверкой SHA-256 и делает его текущей моделью; «Удалить» убирает
  копию. Происхождение и лицензию своей модели проверяйте сами.
- **QR из Recovery Kit без ввода текста.** При восстановлении доступа QR-код комплекта
  можно отсканировать камерой телефона или прочитать с картинки или скриншота (на любой
  платформе). Распознавание идёт на устройстве открытым кодом, кадры никуда не уходят.
- **Общий доступ к RDP-хостам.** RDP-хостом можно поделиться с проверенным коллегой:
  адрес, порт, пользователь, домен и метки; заметки и сохранённый пароль — только по
  отметке. Сервер и протокол не меняются; клиенты до 0.9.3 такую карточку не показывают.
- **Импорт CSV в ClickHouse, MongoDB и Redis.** ClickHouse — пакетами `INSERT` (и
  «Создать таблицу из CSV…» с MergeTree); MongoDB — в коллекцию, с новыми полями по
  заголовкам, пропуском и обновлением по `_id`; Redis — в базу или папку ключей, строками
  (`SET`) или хешами (`HSET`).
- **Конструктор таблиц.** Комментарии с апострофом; в PostgreSQL — снятие identity,
  смена ALWAYS ↔ BY DEFAULT и UNLOGGED; в ClickHouse — переименование таблицы, снятие
  значения по умолчанию и комментария столбца.
- **Cassandra/ScyllaDB: все узлы кластера.** При прямом подключении запросы
  распределяются по всем узлам дата-центра, а при отказе узла переходят на другие;
  раньше сеанс работал через один узел. С ScyllaDB — по соединению на каждый шард
  через shard-aware порт. Через SSH, туннели и пробросы — по-прежнему через один узел.
- **Cassandra/ScyllaDB: трассировка запроса** вместо EXPLAIN, которого нет в CQL:
  запрос выполняется один раз с трассировкой, окно показывает шаги и задержки каждого
  узла деревом, как план у SQL. Только для непрод-подключений, с подтверждением.
- **Cassandra/ScyllaDB: правка ячеек.** Правка ячеек в таблице результатов снова
  доступна (раньше интерфейс её не пускал). Строки одной партиции меняются одним
  условным `BATCH` — все или ни одна; если правки затрагивают несколько партиций, окно
  проверки предупреждает, что вместе они не меняются. Колонки ключа не редактируются.
- **Kubernetes: «Поток ресурсов».** Заголовок больше не повторяется, поиск, фильтры и
  список не наезжают друг на друга, а выбор пространства имён и меток выглядит как
  остальные фильтры.

**Синхронизация.** С сервером 0.2.4 (протокол 1.13) удаление хранилища, запрошенное
ещё не доверенным устройством, откладывается на 7 дней; доверенное устройство показывает
предупреждение в «Синхронизации» и может сохранить хранилище.

**Безопасность.** Исправления по итогам ревью безопасности кода.

- **Команды: подтверждение участника.** Ключ команды получают ровно те ключи участника,
  по которым посчитан код на экране администратора. Если после сверки сервер сообщит
  другие ключи или роль, добавление откажет, и код нужно сверить заново. Раньше
  взломанный сервер мог подменить ключи между сверкой и подтверждением и получить
  доступ к хранилищу команды.
- **Команды: вступление.** Код, который приглашённый диктует администратору, теперь
  привязывает команду к ключу этого администратора. Команда откроется, только если
  участника добавил именно он; поддельную команду, собранную сервером, приложение
  не откроет.
- **Команды: ключ команды закрепляется.** Приложение запоминает ключ команды, с которым
  работало. Подменить его сервер больше не может: другой ключ той же эпохи или новый
  ключ, который не связан цепочкой с прежним, приводит к паузе команды, а не к записи
  новых данных под чужим ключом.
- **Команды: ключ подтверждает администратор.** С сервером 0.2.4 (протокол 1.12)
  владелец или администратор подписывает отпечаток ключа команды при её создании, при
  смене ключа и перед добавлением участника. Новый участник открывает команду, только
  если ключ совпадает с подписанным отпечатком, — подменить ключ сервер не может даже
  при самом первом открытии. Для команд, созданных раньше, отпечаток публикует первый
  администратор, открывший команду в 0.9.3.
- **Общий доступ: свои устройства.** Устройство, которое сервер выдаёт за ваше, больше
  не становится получателем секрета, подключения Kubernetes или базы данных: свои
  устройства получают общий доступ только через подтверждённое подключение. Поиск по
  адресу собственного аккаунта отклоняется.
- **SFTP на Windows.** При скачивании папки имена вида `C:файл`, `файл:поток`, `CON`,
  `NUL` или с точкой/пробелом в конце больше не принимаются: раньше сервер мог записать
  файл за пределы выбранной папки или снять у соседнего файла метку «скачано из
  интернета».
- **ИИ: оценка риска команд.** Команды, которые сами запускают программы или пишут
  файлы, больше не помечаются как «только чтение»: `env -S '…'`, `fd -x/-X`,
  `rg --pre`, скрипты `sed` с командами `e`/`w` и флагами `s///e`/`s///w`, `awk` с
  `| getline` или файлом программы, `git -c …`. Для них снова нужно отметить, что вы
  понимаете риск, перед «Выполнить».
- **Новое устройство: подтверждение без проверки.** Если сервер хоть раз сообщил
  этому устройству, что поддерживает проверку по коду пары, переключить его на старый
  способ без проверки он уже не может: такое подтверждение отклоняется. Одобряющее
  устройство должно быть версии 0.9.2 или новее.
- **Синхронизация: испорченные данные.** Если сервер присылает объект, который не
  проходит проверку подлинности, приложение показывает предупреждение и оставляет
  последнюю хорошую локальную копию, а не заменяет её нечитаемыми данными.
- **SSH-агент на Windows.** Канал встроенного SSH-агента теперь доступен только вашей
  учётной записи и системе: раньше по умолчанию к нему могли подключаться администраторы,
  а на чтение — любые пользователи.
- **Зависимости.** `crypto-bigint` 0.7.10 (RUSTSEC-2026-0387, неравномерное по
  времени выполнение операций, которые должны быть постоянными), `tokio` 1.54.1
  (RUSTSEC-2026-0386) и плановые обновления `hyper`, `uuid`, `tokio-rustls`, `xml` и
  других библиотек.

## English

This release targets macOS, Windows, Linux and Android. iOS is built and tested on the Simulator, but its release is still deferred.
The team fixes work with every sync server that has teams (0.2.1 and later); server 0.2.4
also stops members from changing other members' team keys, lets only the owner appoint
admins and sends invitations only to verified addresses.

**New.**

- The **Recovery Kit** can be printed or saved as PDF.
- **Terminal tabs** can sit in a column at the left edge; a folded sidebar is remembered.
- **Split terminal panes:** “Split right” / “Split down” (Cmd/Ctrl+Shift+\ / Shift+−) open
  another session to the same target beside the focused one, up to four panes per tab;
  Cmd/Ctrl+] moves between them and the dividers can be dragged.
- **Terminal tabs** are reordered by dragging (in the strip and the column), and with
  Alt+↑ / Alt+↓ in the column.
- **Import an SSH key from a file:** “Choose file…” in the import key dialog.
- **The SSH key passphrase** is asked at connect time in its own prompt naming the key,
  not as the server password.
- **CSV import into SQLite and Cassandra/ScyllaDB.** The import wizard now works with
  these databases too. SQLite through `sqlite3` on a host and Cassandra write without
  a shared transaction (CQL has none), so the whole-file transaction is not offered
  there. In Cassandra, insert does not overwrite existing rows (`INSERT … IF NOT EXISTS`).
  “Create table from CSV…” works for SQLite as well.
- **Table designer for SQLite and Cassandra/ScyllaDB.** Create and change tables without
  writing SQL or CQL. SQLite changes that `ALTER TABLE` cannot make rebuild the table in
  one transaction and keep its rows and indexes; a table with triggers or generated
  columns is never rebuilt. Cassandra tables get a partition key, clustering columns
  with their order, secondary indexes, TTL and `gc_grace_seconds`.
- **AI conversation history.** “Conversation history” in the chat lists this session’s
  conversations and those kept in the vault (“Keep AI conversations in the vault”); they
  can be continued or deleted. A continued conversation gives the model its earlier turns
  again, with the same data hidden as before; before, the model started it with an empty
  context.

- **Server source in About.** For a synced profile the dialog shows where your sync server
  offers its source code (AGPL; the server reports it in `/v1/meta`).

- **SQLite: starter snippets and import from a string.** A pack of 15 SQLite snippets (schema,
  indexes, foreign keys, integrity check, file size, query plan, ANALYZE, VACUUM).
  Connections import from `sqlite:///path.db` and `cassandra://` / `scylla://` strings.

**Fixes.**

- **SFTP: recovering edits on other hosts.** The connection opened to recover unsaved
  edits on a host the browser is not showing is now shared by that host's edits and
  closes with the last of them instead of living until the app quits.
- **Editing files over SFTP: backups on Windows and Linux.** The working-copy folder carries
  `CACHEDIR.TAG` and `.nobackup`, so borg, restic, tar, Duplicity, Back In Time and other
  backup tools that honour them skip it (on macOS it stays excluded from Time Machine).
- **SQLite: VACUUM.** An approved `VACUUM` in Databases no longer fails; the rule against
  attaching other databases to the session used to block it.
- **Syncing a deleted vault.** When the vault was deleted on the server, sync stops with a
  clear reason (“vault not found on server”) instead of retrying forever.

- **Syncing Cassandra/ScyllaDB connections and SQLite through a host.** “Sync with the vault”
  moves such a connection and its password into the vault once every device of the account
  runs 0.9.2 or later (with server 0.2.4 the app sees their versions and lists the ones to
  update). An SQLite file on this computer stays here.
- **Reminders with the app closed.** Note and task reminders arrive as a system
  notification “A reminder from ConsoleCrypt” without the note text, since the notification
  centre is not encrypted. On Android the time may drift by a few minutes; on Linux reminders
  still work only while the app is open.
- **SFTP: drag into Finder and Explorer.** On macOS and Windows, files from the host list
  drag straight into Finder or Explorer: the file is downloaded into a temporary folder,
  handed to the system and deleted, with progress in Transfers. Folders still go through
  Download. Dragging inside the app and from Finder/Explorer works as before (on Linux too,
  without dragging out).
- **Local AI: your own GGUF model.** “Add your own model (GGUF)…” copies the chosen file
  into the models folder with a SHA-256 check and makes it the current model; Remove deletes
  the copy. Check the origin and license of your own model yourself.
- **Recovery Kit QR without typing.** Recovery reads the kit's QR code with a phone camera
  or from a picture or screenshot (on any platform). Decoding runs on the device with open
  source code; frames go nowhere.
- **Sharing RDP hosts.** An RDP host can be shared with a verified colleague: address,
  port, user, domain and tags; notes and the saved password only when ticked. Neither the
  server nor the protocol changes; clients before 0.9.3 do not show such a card.
- **CSV import into ClickHouse, MongoDB and Redis.** ClickHouse in `INSERT` batches (and
  “Create table from CSV…” with MergeTree); MongoDB into a collection, with new fields from
  the headers, skip and update by `_id`; Redis into a database or key folder, as strings
  (`SET`) or hashes (`HSET`).
- **Table designer.** Comments with an apostrophe; in PostgreSQL dropping an identity,
  switching ALWAYS ↔ BY DEFAULT and UNLOGGED; in ClickHouse renaming the table and removing
  a column default or comment.
- **Cassandra/ScyllaDB: every cluster node.** Directly, requests spread over every node
  of the data center and move to other nodes when one fails; before, a session used one
  node; with ScyllaDB one connection per shard through the shard-aware port. Through SSH,
  tunnels and forwards it still uses one node.
- **Cassandra/ScyllaDB: query tracing** in place of EXPLAIN, which CQL lacks: the query
  runs once with tracing, and the window shows each node's steps and delays as a tree,
  like an SQL plan. Non-production connections only, after a confirmation.
- **Cassandra/ScyllaDB: cell editing.** Editing cells in the result grid works again
  (the interface used to refuse it). Rows of one partition change in one conditional
  `BATCH` — all or none; when the edits span several partitions, the review warns that
  they do not change together. Key columns are not editable.
- **Kubernetes: resource flow.** The title is no longer repeated, the search, filters and
  list no longer run into each other, and the namespace and label pickers look like the
  other filters.

**Sync.** With server 0.2.4 (protocol 1.13) a vault deletion asked by a device that is not
trusted yet waits 7 days; a trusted device shows a warning under Sync and can keep the vault.

**Security.** Fixes from the code security review.

- **Teams: confirming a member.** The team key goes only to the member keys the code on
  the admin's screen was computed from. If the server reports other keys or another role
  after the check, adding fails and the code has to be compared again. Before, a
  compromised server could swap the keys between the check and the confirmation and
  gain access to the team vault.
- **Teams: joining.** The code the invitee reads to the admin now ties the team to that
  admin's key. The team opens only if that admin adds the member; a fake team put
  together by the server is not opened.
- **Teams: the team key is pinned.** The app remembers the team key it has used. The
  server can no longer swap it: another key for the same epoch, or a new key that does
  not chain back to the previous one, pauses the team instead of writing new data
  under a foreign key.
- **Teams: an admin vouches for the key.** With server 0.2.4 (protocol 1.12) the owner or
  an admin signs the team key's fingerprint when the team is created, when the key is
  rotated and before a member is added. A new member opens the team only if the key
  matches the signed fingerprint — the server cannot swap it, not even on the very
  first open. For teams created earlier, the first admin who opens the team in 0.9.3
  publishes the fingerprint.
- **Sharing: your own devices.** A device the server presents as yours no longer
  becomes a recipient of a secret, a Kubernetes or database connection: your own devices
  get shared items only through a verified enrollment. Looking up your own account's
  address is refused.
- **SFTP on Windows.** Folder downloads no longer accept names such as `C:file`,
  `file:stream`, `CON`, `NUL` or names ending in a dot or space: a server could write a
  file outside the chosen folder or strip a sibling file's "downloaded from the
  internet" mark.
- **AI: command risk.** Commands that run programs or write files themselves are no
  longer marked read-only: `env -S '…'`, `fd -x/-X`, `rg --pre`, `sed` scripts with the
  `e`/`w` commands or the `s///e`/`s///w` flags, `awk` with `| getline` or a program
  file, `git -c …`. They again need the risk acknowledgement before Run.
- **New device: approval without a check.** Once the server told this device that it
  supports the pairing-code check, it can no longer switch it back to the unchecked way:
  such an approval is refused. The approving device must run 0.9.2 or later.
- **Sync: damaged data.** If the server sends an object that fails authentication, the
  app shows a warning and keeps the last good local copy instead of replacing it with
  unreadable data.
- **SSH agent on Windows.** The built-in SSH agent's pipe is now open only to your account
  and the system; before, the default descriptor let administrators in and gave every
  user read access.
- **Dependencies.** `crypto-bigint` 0.7.10 (RUSTSEC-2026-0387, timing variability in
  operations meant to be constant-time), `tokio` 1.54.1 (RUSTSEC-2026-0386) and routine
  updates of `hyper`, `uuid`, `tokio-rustls`, `xml` and other libraries.
