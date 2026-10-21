# ConsoleCrypt 0.8.5

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

**Базы данных — новый раздел.** PostgreSQL, MySQL и MariaDB, ClickHouse, Redis
и MongoDB. Подключения хранятся в зашифрованном хранилище, запросы идут
напрямую с устройства.

- Подключения: форма с проверкой связи, импорт строки подключения и `.pgpass`,
  цвет метки, группы и теги, вид плитками или списком.
- Маршрут к базе: напрямую, через SSH-хост, jump-профиль или прокси, через
  сохранённый SSH-туннель, проброс пода Kubernetes или контейнер Docker.
  Туннель и проброс поднимаются сами при подключении.
- Подключения с пометкой «прод» работают только на чтение; права роли можно
  проверить. В остальных запись включается явно.
- Рабочая область в духе классических SQL-клиентов. Слева структура с папками
  (таблицы, представления, функции; колонки, индексы, ограничения, триггеры),
  раскрытая таблица выделена рамкой. Справа вкладки запросов, редактор с
  номерами строк, подсветкой, автодополнением у курсора, форматированием и
  поиском с заменой.
- Выполнение выделенного или текущего оператора (⌘↵ / Ctrl+Enter) и всего
  скрипта (⌥X / Alt+X). Переменные в запросах, проверка и подтверждение
  изменяющих запросов.
- Результаты — компактная таблица с номерами строк, сортировкой, фильтром,
  закреплением и скрытием колонок; на телефоне — карточки записей.
- EXPLAIN и EXPLAIN ANALYZE в виде дерева плана со временем, строками и
  стоимостью.
- Правка ячеек в результатах одной таблицы с просмотром SQL перед применением.
- Экспорт и копирование результатов, история запросов с избранным, сниппеты.
- ИИ-помощник запросов: составить, объяснить, исправить и ускорить запрос.
  Схема и строки уходят в модель только с вашего согласия.
- Redis: дерево ключей через SCAN и подсказка вместо KEYS. MongoDB: команды в
  привычном синтаксисе оболочки.
- Kubernetes: кнопка «Подключиться к БД» у подов со стандартными портами баз
  данных.

**Docker — новый раздел.** Движок на этом компьютере или на сохранённом
SSH-хосте; если у пользователя нет прав на сокет Docker, это видно сразу.

- Контейнеры сгруппированы по compose-проектам, запущенные сверху, фильтр по
  состоянию и поиск.
- Логи, терминал внутри контейнера, сведения: порты, сети, монтирования,
  метки и JSON.
- Пароли в переменных окружения скрыты; показать и скопировать их можно после
  ввода пароля хранилища.
- Тома с проектами и контейнерами, которые их используют. Пути томов и
  монтирований открываются в SFTP, а для локального Docker — в файловом
  менеджере.
- Проброс порта через SSH-туннель. Пробросы Docker и Kubernetes собраны в одной
  правой панели.
- Подключение к базе данных из контейнера одной кнопкой.
- Меню compose-проекта: запустить, остановить или перезапустить всё,
  просмотреть YAML. Правка — после пароля хранилища: изменения проверяются,
  рядом сохраняется резервная копия, затем проект перезапускается.

**Интерфейс.**

- Выпадающие списки в стиле стекла с компактным шрифтом. Меню, подсказки и
  диалоги окрашиваются в цвет фона из настроек.
- Индикатор ИИ зелёный, только пока встроенная модель действительно запущена.
- SFTP показывает «нет доступа» для папок без прав вместо пустого списка.
- Обновление предлагается, даже если новее только номер сборки.
- Установщик для macOS стал меньше.

Подробности: [руководство по базам данных](DATABASES.md) и
[руководство по Docker](DOCKER.md).

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.

**Databases — a new section.** PostgreSQL, MySQL and MariaDB, ClickHouse,
Redis and MongoDB. Connections live in the encrypted vault; queries go
straight from the device.

- Connections: a form with a connection test, import of a connection string
  and `.pgpass`, label colour, groups and tags, card or list view.
- Route to the database: direct, through an SSH host, a jump profile or a
  proxy, a saved SSH tunnel, a Kubernetes pod forward or a Docker container.
  Tunnels and forwards come back on their own when you connect.
- Connections marked as production are read-only; role privileges can be
  checked. Elsewhere writes are enabled explicitly.
- A workspace in the spirit of classic SQL clients. On the left, the structure
  with folders (tables, views, functions; columns, indexes, constraints,
  triggers), with the expanded table framed. On the right, query tabs and an
  editor with line numbers, highlighting, completion at the caret, formatting
  and find and replace.
- Run the selection or the current statement (⌘↵ / Ctrl+Enter) or the whole
  script (⌥X / Alt+X). Query variables, review and confirmation of changing
  statements.
- Results are a compact grid with row numbers, sorting, filtering, pinned and
  hidden columns; record cards on phones.
- EXPLAIN and EXPLAIN ANALYZE as a plan tree with time, rows and cost.
- Cell editing in single-table results, with the SQL shown before it applies.
- Export and copy results, query history with favourites, snippets.
- A query assistant writes, explains, fixes and speeds up queries. Schema and
  rows reach the model only with your consent.
- Redis: a key tree through SCAN and a hint instead of KEYS. MongoDB: commands
  in the familiar shell syntax.
- Kubernetes: a "Connect to database" button on pods with standard database
  ports.

**Docker — a new section.** An engine on this computer or on a saved SSH
host; missing permissions on the Docker socket are reported right away.

- Containers are grouped by compose project, running ones first, with a state
  filter and search.
- Logs, a terminal inside the container, details: ports, networks, mounts,
  labels and JSON.
- Passwords in environment variables are hidden; they can be shown and copied
  after entering the vault passphrase.
- Volumes with the projects and containers that use them. Volume and mount
  paths open in SFTP, or in the file manager for local Docker.
- Port forwarding through an SSH tunnel. Docker and Kubernetes forwards share
  one right-hand panel.
- One button connects to a container's database.
- The compose project menu starts, stops or restarts everything and shows the
  YAML. Editing needs the vault passphrase: changes are validated, a backup is
  kept next to the file, then the project is redeployed.

**Interface.**

- Glass-style drop-down lists with compact type. Menus, tooltips and dialogs
  take the background colour from Settings.
- The AI indicator is green only while the built-in model is actually running.
- SFTP shows "no access" for unreadable folders instead of an empty list.
- An update is offered even when only the build number is newer.
- The macOS installer is smaller.

See the [Databases guide](DATABASES.md) and the [Docker guide](DOCKER.md).
