# ConsoleCrypt 0.9.0

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

**Корпоративный вход в сервер синхронизации.** Сервер синхронизации 0.2.0 и
новее может пускать в аккаунты только через сервер авторизации организации.
Приложение само узнаёт режим сервера после ввода адреса и показывает нужную
форму.

- **Вход через SSO (OIDC).** Кнопка «Войти через {название}» открывает
  страницу входа организации в системном браузере; e-mail можно указать
  заранее. Пока идёт вход, виден статус ожидания и кнопка «Отмена». На
  компьютере браузер возвращается в приложение через локальный адрес
  `127.0.0.1`, на Android — через Custom Tabs, на iOS — через системное окно
  входа. Используются PKCE и проверка `state`; код входа одноразовый.
- **Вход через Active Directory.** Поля «Логин (email или DOMAIN\user)» и
  «Пароль» с подписью «Учётная запись вашей организации». Пароль отправляется
  один раз по TLS, не сохраняется и стирается из поля после каждой попытки.
- В корпоративном режиме нет регистрации, пароля аккаунта и «Забыли пароль».
  Новый пользователь после входа проходит обычное создание хранилища.

**Видно, где используется корпоративный сервер.** На экране входа:
«Вход через корпоративный сервер авторизации {название}. Пароль организации
не открывает хранилище — для этого нужен пароль хранилища.» В «Настройках»,
«Синхронизации» и «Устройствах» — строка «Авторизация: корпоративный сервер —
{название} (OIDC | Active Directory)»; на обычном сервере — «Email и пароль
аккаунта».

**Приложение помнит, как вы входили.** Для каждого сервера на устройстве
сохраняются способ входа и логин (не пароль). При следующем подключении способ
выбран заранее, логин подставлен, видна подсказка «В прошлый раз вы входили
через … как …» с кнопкой «Забыть», а недавние серверы показаны с меткой
«Email», «SSO: …» или «AD». Если сервер сменил режим входа, приложение
предупреждает об этом. В настройках видно, как и когда это устройство вошло.

**Доступ отключён администратором.** Когда организация отключает учётную
запись, сервер отзывает сессии, а приложение блокирует профиль и показывает
«Доступ отключён администратором». Данные на устройстве остаются
зашифрованными; профиль можно удалить с устройства или войти снова, если
доступ вернули. Ошибки корпоративного входа (домен не допущен, нет группы,
нужен второй фактор, учётная запись уже связана, служба входа недоступна,
слишком много попыток) показываются понятным текстом.

**Что не меняется.** Шифрование, хранилища, устройства и общий доступ
работают как раньше: корпоративный вход заменяет только пароль аккаунта.
После входа устройство — обычное новое устройство, хранилище открывает пароль
хранилища, ключ восстановления или одобрение. Со старыми серверами и
серверами в стандартном режиме всё работает как прежде.

На iOS добавлен системный обработчик входа через браузер; он же используется
для входа OIDC в Kubernetes.

**Заметки, задачи и напоминания.** В правой панели рядом со «Сниппетами» и
«ИИ» появился инструмент «Заметки» (также в палитре команд и в шапке на
телефоне).

- Заметки с Markdown, метками и закреплением. Переключатель «Задача» добавляет
  срок; просроченные задачи выделены, а число просроченных задач и
  сработавших напоминаний видно на значке «Заметок».
- Напоминания: «Через час», «Сегодня в 18:00», «Завтра в 09:00», «Через неделю»
  или своё время. Пока приложение открыто, в срок появляется уведомление с
  кнопкой «Открыть»; в меню заметки — «Напомнить через час». Системных
  уведомлений при закрытом приложении пока нет.
- Заметки к объектам. Вкладка «Объект» показывает заметки к тому, что открыто
  сейчас: SSH-хосту активной вкладки терминала, RDP-хосту, хосту SFTP,
  открытому подключению к базе данных, ресурсу или контексту Kubernetes,
  выбранному Docker-контейнеру. Вкладка «Все» показывает все заметки и к чему
  они привязаны, «Задачи» — только задачи.
- Текст из поля «Заметки» карточки хоста можно перенести в обычную заметку
  кнопкой «Перенести в заметки».
- Заметки шифруются на устройстве и синхронизируются сквозным шифрованием, как
  остальные данные хранилища. Удаление хоста или базы заметки не удаляет — у
  них появляется пометка «Объект не найден».

**Общие заметки** (сервер синхронизации 0.2.0 с включённым
`CC_SHARED_NOTES_ENABLED`).

- У общего хоста, подключения к базе данных или контекста Kubernetes во
  вкладке «Объект» появляется блок «Общие заметки». Их видят все участники
  объекта; писать может любой участник, изменить — только автор, удалить —
  автор или владелец. Заметка к Docker-контейнеру общего SSH-хоста или к
  ресурсу общего контекста видна только у этого контейнера или ресурса.
- Ключ общих заметок выдаёт и подписывает устройство владельца, каждую заметку
  подписывает устройство автора — сервер не может подменить ключ или прочитать
  текст. Пока владелец ни разу не открыл панель заметок для объекта, участники
  видят сообщение об ожидании. После исключения участника устройство владельца
  при следующем открытии панели перешифровывает заметки новым ключом.
- Связь объекта с общим доступом запоминается при публикации и при сохранении
  личной копии. Для объектов, опубликованных до 0.9.0 (кроме скопированных
  хостов), опубликуйте или скопируйте объект заново, чтобы у него появились
  общие заметки.

### Также в выпуске: изменения 0.8.7

Версия 0.8.7 отдельно не публиковалась; её изменения входят в 0.9.0.

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

**Corporate sign-in to the sync server.** Sync server 0.2.0 and later can admit
accounts only through the organisation's authorization server. After you enter
the server address, the app detects the server's mode and shows the matching
form.

- **SSO sign-in (OIDC).** "Sign in with {name}" opens the organisation's
  sign-in page in the system browser; the e-mail can be entered beforehand.
  While signing in, the app shows a waiting state with Cancel. On desktops the
  browser returns to the app through a local `127.0.0.1` address, on Android
  through Custom Tabs, on iOS through the system sign-in sheet. PKCE and a
  `state` check are used; the sign-in code is single-use.
- **Active Directory sign-in.** "Login (email or DOMAIN\user)" and "Password"
  under "Your organisation's account". The password is sent once over TLS,
  never stored and cleared from the field after every attempt.
- Corporate mode has no registration, account password or "Forgot password".
  A new user goes through the usual vault setup after signing in.

**The corporate server is always named.** The sign-in screen says "Sign-in
through the corporate authorization server {name}. Your organisation's
password does not open the vault — the vault passphrase does." Settings, Sync
and Devices show "Authorization: Corporate server — {name} (OIDC | Active
Directory)"; on a standard server it reads "E-mail and account password".

**The app remembers how you signed in.** For every server this device keeps
the sign-in method and login (never the password). Next time the method is
preselected, the login is filled in, a hint "Last time you signed in with …
as …" offers Forget, and recent servers carry an "Email", "SSO: …" or "AD"
badge. If the server changed its sign-in mode, the app warns about it.
Settings show how and when this device signed in.

**Access disabled by the administrator.** When the organisation disables the
account, the server revokes its sessions, and the app locks the profile and
shows "Access disabled by the administrator". Data on the device stays
encrypted; the profile can be deleted from the device, or you can sign in
again once access is restored. Corporate sign-in errors (domain not allowed,
group missing, second factor required, account already linked, sign-in
service unavailable, too many attempts) are explained in plain words.

**What stays the same.** Encryption, vaults, devices and sharing work as
before: corporate sign-in replaces only the account password. After signing
in the device is an ordinary new device; the vault opens with the vault
passphrase, the Recovery Key or approval. Older servers and servers in
standard mode work as before.

iOS gains a system browser sign-in handler, also used for Kubernetes OIDC.

**Notes, tasks and reminders.** The right workspace panel gains a "Notes"
tool next to Snippets and AI (also in the command palette and the phone
header).

- Notes with Markdown, tags and pinning. The "Task" switch adds a due time;
  overdue tasks are highlighted, and the Notes icon shows how many tasks are
  overdue and reminders went off.
- Reminders: in an hour, today at 18:00, tomorrow at 09:00, in a week or a
  time of your choice. While the app is open a notice with "Open" appears on
  time; the note menu offers "Remind in an hour". System notifications while
  the app is closed are not available yet.
- Notes about objects. The "Object" tab shows notes about what is open now:
  the SSH host of the active terminal tab, the RDP host, the SFTP host, the
  open database connection, the Kubernetes resource or context, the selected
  Docker container. "All" lists every note and what it is attached to, "Tasks"
  only tasks.
- The host card's free-text notes move into a regular note with "Move to
  notes".
- Notes are encrypted on the device and synced end-to-end encrypted like the
  rest of the vault. Deleting a host or database keeps its notes, marked
  "Object not found".

**Shared notes** (sync server 0.2.0 with `CC_SHARED_NOTES_ENABLED` on).

- A shared host, database connection or Kubernetes context shows "Shared
  notes" in the "Object" tab. Every member reads them; any member writes, only
  the author edits, the author or the owner deletes. A note about a Docker
  container of a shared SSH host or a resource of a shared context stays with
  that container or resource.
- The owner device issues and signs the notes key and the author device signs
  every note, so the server can neither substitute the key nor read the text.
  Members wait until the owner has opened the notes panel for the object once.
  After a member is removed, the owner's device moves the notes to a new key
  the next time the panel opens.
- The link between an object and its share is remembered when publishing it
  and when saving a personal copy. For objects published before 0.9.0 (other
  than copied hosts), publish or copy the object again to get shared notes.

### Also in this release: the 0.8.7 changes

Version 0.8.7 was not published on its own; its changes ship in 0.9.0.

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

See the [Account and sync guide](SYNC_ACCOUNT.md), the [Databases guide](DATABASES.md) and the [Kubernetes guide](KUBERNETES.md).
