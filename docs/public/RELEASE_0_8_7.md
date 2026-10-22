# ConsoleCrypt 0.8.7

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

**Подтверждение паролем хранилища.** Опасные действия больше не нужно
подтверждать вводом имени подключения, контекста или ресурса — везде
спрашивается пароль хранилища. Неверный пароль ничего не выполняет, поле
очищается после каждой попытки, Enter подтверждает.

- Базы данных: изменяющие и опасные запросы, правки ячеек и «Разблокировать
  прод» (теперь только пароль и срок).
- Kubernetes: применение YAML, создание и удаление ресурсов, перезапуск,
  масштабирование, cordon/drain, debug-контейнер, действия GitOps, откат Helm,
  массовые операции, очистка истории событий и удаление диалогов ИИ.

**Базы данных.**

- Двойной щелчок по таблице или «Первые 100 строк» сразу показывают данные
  внизу — в одной вкладке просмотра, которая переиспользуется, а не плодит
  новые. Изменённая вручную вкладка просмотра становится обычной.
- Сгенерированный SQL (DDL «Открыть в редакторе», история, черновик ИИ,
  предложенный `SCAN`) добавляется в текущую вкладку ниже написанного, а не
  в новую; ничего не выполняется.
- «Сведения об объекте» работают для PostgreSQL, MySQL/MariaDB, ClickHouse и
  Redis: владелец, оценка числа строк, размеры данных и индексов, число
  колонок и индексов, ключи секционирования и сортировки, последние VACUUM и
  ANALYZE, для ключей Redis — тип, TTL, длина и память. Окно со сведениями
  сгруппировано по разделам, размеры в КиБ/МиБ, оценки помечены «≈», есть
  кнопка «Копировать». Пункт меню скрыт там, где сведений нет.
- Ошибки в рабочей области показываются стеклянным уведомлением, а не белой
  полосой; в контекстном меню подсвечивается только один пункт.
- Проверка правок ячеек показывает сами значения — строки в кавычках, `NULL`
  для пустых, длинные значения сокращены — вместо `{{p0}}`. Выполняется
  по-прежнему запрос с параметрами.
- Запрос в окне проверки показывается блоком кода, как DDL: моноширинный
  шрифт, подсветка, номера строк, можно выделять и копировать.
- Окно DDL подстраивается под ширину экрана и не оставляет пустого места под
  коротким кодом. Длинные строки переносятся с отступом; если выключить
  «Переносить строки», код прокручивается вбок. Выбор запоминается.
- Базовые наборы сниппетов: SQL · PostgreSQL (20), SQL · MySQL/MariaDB (20),
  SQL · ClickHouse (20), Redis (15) и MongoDB (15) — активные и долгие запросы,
  блокировки, размеры, индексы, репликация, тяжёлые запросы. Добавляются в
  «Сниппеты → Стартовые наборы». Их же можно запускать в psql, mysql,
  clickhouse-client, redis-cli и mongosh внутри SSH-сессии: многострочный
  запрос уходит одной вставкой; изменяющие команды требуют подтверждения.
- Экспорт в CSV/TSV экранирует формулы для табличных редакторов (флажок
  «Экранировать формулы (для Excel)», включён по умолчанию); SQL-экспорт
  правильно экранирует кавычки и обратную косую черту для каждой базы.
- Сохранённый туннель с сетевым адресом для подключения к базе запускается
  только на 127.0.0.1.

**Общий доступ** (сервер синхронизации 0.1.17).

- Удалить общий элемент может только владелец; удалённое можно восстановить в
  течение 30 дней: «Я поделился → Недавно удалённые».
- Получатель может выйти из общего доступа — в карточке элемента или во
  входящем приглашении; сохранённые личные копии остаются. Владелец видит, кто
  вышел.
- Повреждённая общая запись больше не ломает весь список: она скрывается, а
  приложение сообщает, сколько записей не удалось прочитать.
- Адрес почты при поиске получателя больше не передаётся в адресе запроса.

**Безопасность.**

- SFTP: «Открыть» больше не запускает загруженные файлы, способные выполнить
  код (скрипты, ярлыки, .exe, .command, .desktop и т. п.) — они открываются как
  текст в редакторе после предупреждения. Загруженные файлы помечаются как
  полученные из интернета (Windows) и карантином (macOS).
- Новая парольная фраза хранилища — не короче 12 символов; существующие
  хранилища открываются как прежде. Ключ шифруется Argon2id не слабее 64 МиБ;
  старые параметры обновляются при следующей разблокировке фразой.
- Резервные копии записываются через безопасно созданный временный файл;
  чтение копий и ответов сервера ограничено по размеру.
- Автоочистка буфера обмена настраивается от 30 секунд до 1 дня и срабатывает
  и после перезапуска, не трогая чужое содержимое.

**Windows.**

- В выборе шрифта терминала — только установленные моноширинные шрифты,
  включая установленные для пользователя (например, JetBrainsMono Nerd Font);
  имена «… Nerd Font Mono» и «… NFM» взаимозаменяемы, а если шрифт не
  установлен, настройки подскажут. Linux тоже показывает установленные шрифты.
- Окно скрывается от скриншотов, записи и демонстрации экрана, пока открыт
  пароль, ключ или слова восстановления. «Настройки → Безопасность»: «Никогда»,
  «Когда видны секреты» или «Всегда».

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.

**Confirm with the vault passphrase.** Dangerous actions no longer ask you to
type a connection, context or resource name — they ask for the vault
passphrase everywhere. A wrong passphrase runs nothing, the field is cleared
after every attempt and Enter confirms.

- Databases: changing and dangerous queries, cell edits and "Unlock
  production" (now just the passphrase and the duration).
- Kubernetes: YAML apply, creating and deleting resources, restart, scale,
  cordon/drain, debug containers, GitOps actions, Helm rollback, bulk actions,
  clearing event history and deleting AI conversations.

**Databases.**

- Double-clicking a table or choosing the first 100 rows shows the data below
  right away, in one preview tab that is reused instead of adding new tabs.
  Editing the preview tab turns it into an ordinary tab.
- Generated SQL (DDL Open in editor, history, AI drafts, the suggested `SCAN`)
  is added to the current tab below the existing text instead of a new tab;
  nothing runs.
- Object details work for PostgreSQL, MySQL/MariaDB, ClickHouse and Redis:
  owner, estimated rows, data and index sizes, column and index counts,
  partitioning and sorting keys, last VACUUM and ANALYZE, and for Redis keys
  the type, TTL, length and memory. The dialog groups them in sections, shows
  sizes in KiB/MiB, marks estimates with "≈" and has a Copy button. The menu
  item is hidden where there is nothing to show.
- Errors in the workspace appear as a glass notification instead of a white
  bar, and the context menu highlights only one item.
- The cell edit review shows the actual values — quoted strings, `NULL` for
  empty cells, long values shortened — instead of `{{p0}}`. What runs is still
  the parameterized query.
- The review shows the query as a code block like the DDL view: monospace,
  highlighted, with line numbers, selectable and copyable.
- The DDL dialog follows the window width and leaves no empty space under
  short code. Long lines wrap with an indent; "Wrap lines" switches to
  horizontal scrolling, and the choice is remembered.
- Database starter snippets: SQL · PostgreSQL (20), SQL · MySQL/MariaDB (20),
  SQL · ClickHouse (20), Redis (15) and MongoDB (15) — active and long-running
  queries, locks, sizes, indexes, replication and top queries. Add them from
  Snippets → Starter packages. They also run inside psql, mysql,
  clickhouse-client, redis-cli and mongosh in an SSH session: a multi-line
  query is sent as one paste; changing commands need confirmation.
- CSV/TSV export escapes spreadsheet formulas ("Escape formulas (for Excel)",
  on by default); SQL export escapes quotes and backslashes correctly for each
  database.
- A saved tunnel with a network bind address starts on 127.0.0.1 only when a
  database connection uses it.

**Sharing** (sync server 0.1.17).

- Only the owner can delete a shared item; deleted items can be restored for
  30 days from Shared by me → Recently deleted.
- Recipients can leave a shared item from the item or an incoming invitation;
  saved personal copies stay. Owners see who left.
- One malformed shared record no longer breaks the shared list: it is hidden
  and the app shows how many entries could not be read.
- Recipient search no longer puts the e-mail address into the request URL.

**Security.**

- SFTP: Open no longer runs downloaded files that can execute code (scripts,
  shortcuts, .exe, .command, .desktop, etc.) — they open as text in an editor
  after a notice. Downloads are marked as coming from the Internet (Windows)
  and quarantined (macOS).
- New vault passphrases need at least 12 characters; existing vaults open as
  before. Keys are wrapped with Argon2id of at least 64 MiB; older parameters
  are upgraded at the next passphrase unlock.
- Backups are written through a safely created temporary file; backup and
  server-response reads are size-bounded.
- Clipboard auto-clear is configurable from 30 seconds to 1 day and also
  completes after an app restart, never clearing foreign content.

**Windows.**

- The terminal font picker lists the monospaced fonts actually installed,
  including per-user fonts such as JetBrainsMono Nerd Font; "… Nerd Font Mono"
  and "… NFM" names are interchangeable, and Settings tells you when the chosen
  font isn't installed. Linux lists installed fonts too.
- The window is hidden from screenshots, recordings and screen sharing while a
  password, key or recovery words are shown. Settings → Security: Never, While
  secrets are shown, or Always.

See the [Databases guide](DATABASES.md) and the [Kubernetes guide](KUBERNETES.md).
