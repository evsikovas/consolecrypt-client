# Docker

## Русский

Раздел Docker показывает контейнеры, тома и compose-проекты движка на этом
компьютере или на сохранённом SSH-хосте. В нём логи, терминал внутри
контейнера, сведения inspect, проброс портов, подключение к базе данных из
контейнера и правка compose-файлов. Клиент обращается к Docker API сам:
для локального движка — через его сокет, для SSH-хоста — через SSH-подключение
этого хоста. Раздел доступен на macOS, Windows, Linux и Android; сборка и
проверка iOS отложены.

### Хосты и права

Откройте Docker в боковом меню. В списке — «Этот компьютер» и сохранённые
SSH-хосты; поиск сужает список. Нажмите «Подключиться»: в шапке появятся
версия движка, ОС и архитектура, число запущенных контейнеров. Кнопка со
стрелкой отключается и возвращает к выбору хоста.

Если у SSH-пользователя нет прав на сокет Docker, это видно сразу: приложение
объясняет причину и показывает команду `sudo usermod -aG docker <пользователь>`
с кнопкой копирования. Выполните её на хосте сами — приложение её не
запускает — и подключитесь снова: новая группа действует в новых SSH-сессиях.
Если SSH-сервер запрещает проброс сокета Docker и на хосте нет docker CLI,
включите `AllowStreamLocalForwarding yes` в `sshd_config` или установите
docker CLI. Если Docker не запущен, приложение так и сообщит.

Действия выполняются с правами пользователя Docker на хосте; приложение не
повышает права и не использует `sudo`. Смена профиля и блокировка хранилища
закрывают сессию Docker.

### Контейнеры

Контейнеры сгруппированы по compose-проектам: сначала проекты с запущенными
контейнерами, в конце — отдельные контейнеры. Заголовок группы показывает,
сколько контейнеров запущено, и сворачивает группу. Фильтр ищет по имени и
образу, переключатель «Все / Запущенные / Остановленные» сужает список.

В строке контейнера: состояние, проверка здоровья, образ, короткий ID, статус
и порты. Кнопки запускают, останавливают и перезапускают контейнер, открывают
логи и терминал. В меню «…» — приостановить и возобновить, принудительно
завершить, удалить, пробросить порт, подключиться к БД, сведения и
копирование ID. Принудительное завершение и удаление требуют подтверждения;
именованные тома при удалении контейнера сохраняются.

### Сведения, логи и терминал

Нажмите на контейнер — сведения откроются справа от списка (на телефоне —
отдельным экраном).

- **Обзор**: образ, команда, время создания и запуска, перезапуски, проект и
  сервис compose, порты, сети с IP-адресами, монтирования (тип, источник,
  путь в контейнере, режим), метки и переменные окружения.
- **Логи**: поток новых строк, stderr выделен цветом. Пауза сохраняет новые
  строки до продолжения, отметки времени включаются отдельно, поиск
  показывает число совпадений. Показанные строки можно скопировать или
  очистить с экрана; хранятся только последние строки.
- **JSON**: ответ inspect со скрытыми секретами и кнопкой копирования.

«Терминал» открывает оболочку внутри контейнера во вкладке терминала.
Автоматический выбор пробует `bash`, `ash` и `sh`; оболочку можно указать
явно.

### Секреты

Без пароля хранилища раздел Docker не показывает и не копирует значения,
похожие на секреты. Вместо них — `••••••` (длина значения не раскрывается):

- значения переменных и меток, имя которых похоже на секрет: `PASSWORD`,
  `TOKEN`, `SECRET`, `KEY`, `DSN`, `CERT`, `SESSION`, `COOKIE`, `SALT`,
  `SIGNATURE` и т. п. (`*_FILE`, `*_PATH` — это пути, они видны);
- пароль в адресе вида `схема://пользователь:пароль@хост` — пользователь, хост
  и путь остаются;
- значения флагов командной строки: `--password`, `--token`, `--secret`,
  `--requirepass`, `--api-key`, `--key`, у MySQL и MariaDB — `-p<пароль>`,
  у `redis-cli` — `-a`;
- закрытые ключи PEM целиком и токены известных форматов (GitHub, GitLab,
  Slack, `sk-…`, ключи доступа AWS, JWT).

Ссылки на переменные (`${POSTGRES_PASSWORD}`) не скрываются: это не сам
секрет.

Где действует: сведения о контейнере (команда, аргументы, метки, переменные),
вкладка JSON и просмотр compose-файлов. В переменных окружения «Показать
секреты» открывает значения; на вкладке JSON «Показать полностью» открывает
весь inspect. Обе кнопки запрашивают пароль хранилища; полный JSON копируется
как секрет и автоматически удаляется из буфера обмена. Раскрытые данные
забываются при закрытии карточки, обновлении, блокировке хранилища и смене
профиля.

Логи показываются как есть: это данные приложения. Если в копируемых строках
(кнопкой или выделением) есть секрет, текст копируется как секрет и
автоматически удаляется из буфера обмена.

Терминал контейнера даёт полный доступ, в том числе к переменным окружения
(`env`). При первом открытии терминала в профиле об этом напоминает подсказка.

Пароль хранилища защищает от случайного показа секретов на экране и от
подглядывания. От человека за незаблокированным компьютером он не защищает:
для этого включите автоблокировку хранилища в настройках.

### Тома и файлы

Вид «Тома» группирует тома по compose-проектам и показывает контейнеры,
которые их используют; неиспользуемые тома помечены отдельно. Путь тома или
монтирования открывается в SFTP этого SSH-хоста (файл монтирования — с
выделением в папке). Для локального Docker путь открывается в файловом
менеджере; папка-пакет macOS (например, `.app`) только выделяется в Finder и
никогда не запускается. Docker Desktop хранит тома внутри своей виртуальной машины, поэтому
их папки с компьютера не открываются — приложение это поясняет.

### Проброс портов

«Пробросить порт» связывает локальный порт с портом контейнера; по умолчанию
предложены порты контейнера, а локальный порт `0` выбирает свободный. Для
SSH-хоста проброс идёт через SSH-подключение, локальный адрес —
`127.0.0.1`. Запущенные пробросы Docker и Kubernetes собраны в общей правой
панели «Проброс портов»: состояние, адрес с копированием, число подключений,
трафик и остановка. Панель открывается сама после запуска проброса.

### Подключение к базе данных

У контейнера со стандартным портом базы данных (PostgreSQL, MySQL, MariaDB,
ClickHouse, Redis, MongoDB) есть кнопка «Подключиться к БД» — в строке, в
меню и в сведениях. Если подключение с таким маршрутом уже сохранено, оно
сразу откроется в разделе «Базы данных». Иначе откроется форма нового
подключения с движком, адресом и маршрутом, а также базой и пользователем,
если они есть в окружении контейнера; останется ввести пароль.

- Опубликованный порт сохраняется как адрес на Docker-хосте: на этом
  компьютере — напрямую, на SSH-хосте — через этот хост.
- Неопубликованный порт удалённого контейнера доступен по IP контейнера через
  SSH-хост.
- Неопубликованный порт локального контейнера получает маршрут «Контейнер
  Docker»: проброс восстанавливается при каждом подключении, контейнер ищется
  по имени.

Подключение с удалённого хоста по умолчанию предлагается как «Прод»; форма
позволяет это изменить.

### Compose-проекты

Нажмите «…» в заголовке compose-группы или щёлкните по нему правой кнопкой
мыши:

- **Запустить все** запускает только остановленные контейнеры проекта.
- **Остановить все** и **Перезапустить все** показывают, сколько контейнеров
  затронет действие, и ждут подтверждения.
- **Показать YAML** открывает файлы проекта вкладками с номерами строк и
  подсветкой; вверху — папка проекта и файлы окружения. Секреты в
  `environment`, `labels`, `command`, `entrypoint`, `healthcheck.test`,
  пароли в адресах и ключи скрыты, номера строк сохраняются. «Показать
  полностью» после пароля хранилища показывает исходный текст. Файл, который
  пользователь не может прочитать, помечен с объяснением. Копирование идёт
  через буфер обмена с автоочисткой.
- **Изменить YAML** требует пароль хранилища: compose-файлы могут содержать
  пароли и определяют, что запущено на хосте. В режиме правки виден исходный
  текст. Разрешение на правку действует ограниченное время.

«Сохранить и применить» сначала проверяет конфигурацию на хосте командой
`docker compose config` (или `docker-compose config`, если стоит отдельная
утилита); при ошибке ничего не сохраняется. Затем рядом с
каждым изменённым файлом остаётся резервная копия
`<файл>.bak-ГГГГММДД-ЧЧММСС`, файл записывается, и проект обновляется
командой `docker compose up -d`; её вывод и код завершения видны в окне.
«Сохранить без применения» оставляет запущенные контейнеры на прежней
конфигурации. Если файл изменился на хосте после открытия, сохранение
отменяется: скопируйте свои правки и откройте файл заново. Без прав на запись
файл не меняется. Если на хосте нет утилиты compose, изменения не
сохраняются, потому что их нельзя проверить.

Сохранение не доверяет именам файлов. Временный файл (со случайным именем) и
резервная копия создаются эксклюзивно, без перехода по символическим ссылкам,
и записываются через один открытый файл — права тоже выставляются через него.
Перед проверкой, после неё и вокруг переименования приложение сверяет, что это
всё ещё записанный им файл (на этом компьютере — по inode, по SSH — по
SHA-256 содержимого). Если файл подменили, сохранение прерывается, а
подменённый оригинал восстанавливается из резервной копии. Если каталог
проекта доступен на запись другим пользователям, перед сохранением появится
предупреждение, и сохранение продолжится только после подтверждения.

Просмотр и правка YAML работают для проектов, контейнеры которых хранят метки
compose с путями к файлам. Старые версии compose и другие инструменты таких
меток не ставят.

## English

The Docker section shows the containers, volumes and compose projects of the
engine on this computer or on a saved SSH host: logs, a terminal inside the
container, inspect details, port forwarding, connecting to a container’s
database and editing compose files. The client talks to the Docker API itself:
through the socket for a local engine and through the host’s SSH connection
for a remote one. The section is available on macOS, Windows, Linux and
Android; the iOS build and testing are deferred.

### Hosts and permissions

Open Docker in the sidebar. The list shows This computer and your saved SSH
hosts; search narrows it. Choose Connect to see the engine version, OS and
architecture and the number of running containers. The back arrow disconnects
and returns to the host list.

If the SSH user cannot access the Docker socket, you see it right away with
the command `sudo usermod -aG docker <user>` and a copy button. Run it on the
host yourself — the app never does — and reconnect; the new group applies to
new SSH sessions. If the SSH server forbids socket forwarding and the host has
no docker CLI, set `AllowStreamLocalForwarding yes` in `sshd_config` or install
the docker CLI. A stopped Docker daemon is reported as such.

Actions run with the Docker user’s rights on the host; the app never elevates
privileges or uses `sudo`. Switching profiles or locking the vault closes the
Docker session.

### Containers

Containers are grouped by compose project, projects with running containers
first and standalone containers last. A group header shows how many are
running and collapses the group. The filter matches names and images; the
All / Running / Stopped switch narrows the list.

Each row shows the state, health, image, short ID, status and ports. Buttons
start, stop and restart the container and open its logs and terminal. The “…”
menu adds pause and resume, kill, remove, port forwarding, connecting to a
database, details and Copy ID. Kill and Remove ask for confirmation; named
volumes survive removal.

### Details, logs and terminal

Click a container to open its details next to the list (a separate screen on
phones).

- **Overview**: image, command, created and started times, restarts, compose
  project and service, ports, networks with IP addresses, mounts (type,
  source, destination, mode), labels and environment variables.
- **Logs**: a live stream with stderr in its own colour. Pause keeps new lines
  until you resume, timestamps are optional and search shows the match count.
  Copy or clear the shown lines; only the most recent lines are kept.
- **JSON**: the inspect output with secrets hidden, with a copy button.

Terminal opens a shell inside the container in a terminal tab. Automatic
detection tries `bash`, `ash` and `sh`, or choose the shell explicitly.

### Secrets

Without the vault passphrase the Docker section neither shows nor copies
values that look secret. They read `••••••` (the length is not revealed):

- values of variables and labels whose name looks secret: `PASSWORD`,
  `TOKEN`, `SECRET`, `KEY`, `DSN`, `CERT`, `SESSION`, `COOKIE`, `SALT`,
  `SIGNATURE` and the like (`*_FILE` and `*_PATH` are paths and stay
  visible);
- the password of an address like `scheme://user:password@host` — user, host
  and path stay;
- values of command-line flags: `--password`, `--token`, `--secret`,
  `--requirepass`, `--api-key`, `--key`, MySQL's and MariaDB's
  `-p<password>`, `redis-cli`'s `-a`;
- PEM private keys as a whole and tokens of known formats (GitHub, GitLab,
  Slack, `sk-…`, AWS access keys, JWTs).

Variable references (`${POSTGRES_PASSWORD}`) are not hidden: they are not the
secret itself.

This applies to container details (command, arguments, labels, environment),
the JSON tab and the compose file view. In the environment, “Show secrets”
reveals the values; on the JSON tab, “Show in full” reveals the whole
inspect. Both ask for the vault passphrase; the full JSON is copied as a
secret and clears from the clipboard automatically. Revealed data is
forgotten when the details close or reload, when the vault locks and when the
profile changes.

Logs are shown as they are: they are the application's data. When copied
lines (with the button or a selection) contain a secret, the text is copied as
a secret and clears from the clipboard automatically.

A container terminal gives full access, environment variables (`env`)
included. The first terminal opened in a profile says so once.

The vault passphrase protects secrets from being shown by accident and from
shoulder surfing. It does not protect against someone at an unlocked
computer: turn on the vault's auto-lock in Settings for that.

### Volumes and files

The Volumes view groups volumes by compose project with the containers that
use them; unused volumes are marked. A volume or mount path opens in SFTP on
that SSH host (a mounted file is selected in its folder), or in the file
manager for local Docker; a macOS bundle folder (such as `.app`) is only
selected in Finder, never launched. Docker Desktop keeps volumes inside its virtual
machine, so their folders cannot be opened from the computer; the app says so.

### Port forwarding

“Forward port” links a local port to a container port; the container’s ports
are suggested and local port `0` picks a free one. On an SSH host the forward
runs through the SSH connection; the local address is `127.0.0.1`. Running
Docker and Kubernetes forwards share the right-hand “Port forwards” panel with
state, the address and a copy button, connections, traffic and Stop. The panel
opens by itself when a forward starts.

### Connecting to a database

A container with a standard database port (PostgreSQL, MySQL, MariaDB,
ClickHouse, Redis, MongoDB) has a “Connect to database” button in its row, its
menu and its details. A saved connection with the same route opens in
Databases at once. Otherwise a new connection form appears with the engine,
address and route filled in, plus the database and user when the container
environment has them; only the password is left.

- A published port is saved as an address on the Docker host: directly on
  this computer, through the host on an SSH host.
- An unpublished port of a remote container is reached at the container IP
  through the SSH host.
- An unpublished port of a local container gets a Docker container route: the
  forward is restored on every connect and the container is found by name.

A connection from a remote host is suggested as production; the form lets you
change that.

### Compose projects

Choose “…” on a compose group header or right-click it:

- **Start all** starts only the stopped containers of the project.
- **Stop all** and **Restart all** say how many containers are affected and
  wait for confirmation.
- **View YAML** opens the project files as tabs with line numbers and
  highlighting, with the project folder and environment files at the top.
  Secrets in `environment`, `labels`, `command`, `entrypoint`,
  `healthcheck.test`, passwords in addresses and keys are hidden; line numbers
  stay the same. “Show in full” shows the original text after the vault
  passphrase. A file the user cannot read is marked with an explanation.
  Copying goes through the self-clearing clipboard.
- **Edit YAML** needs the vault passphrase: compose files can contain
  passwords and decide what runs on the host. The editor shows the original
  text. Editing permission lasts a limited time.

“Save and apply” first validates the configuration on the host with
`docker compose config` (or `docker-compose config` when only the standalone
tool is installed); on errors nothing is saved. Then a backup
`<file>.bak-YYYYMMDD-HHMMSS` is kept next to each changed file, the file is
written and the project is updated with `docker compose up -d`, whose output
and exit code appear in the window. “Save without applying” leaves the running
containers on the previous configuration. If a file changed on the host after
it was opened, saving is refused: copy your edits and open the file again.
Without write permission the file stays unchanged. Without a compose tool on
the host nothing is saved, because the change cannot be validated.

Saving never trusts a file name. The temporary file (with a random name) and
the backup are created exclusively without following symbolic links and are
written through one open file, permissions included. Before and after the
validation and around the rename the app checks that it is still the file it
wrote (by inode on this computer, by the SHA-256 of the content over SSH). If
a file was swapped, the save stops and a replaced original is restored from
the backup. If the project directory is writable by other users, a warning
appears first and the save continues only after you confirm.

Viewing and editing YAML needs containers that carry the compose labels with
file paths; older compose versions and other tools do not set them.
