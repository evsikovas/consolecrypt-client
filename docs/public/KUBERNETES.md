# Kubernetes

## Русский

Раздел Kubernetes объединяет подключения к кластерам, ресурсы, логи, терминалы
Pod, туннели, Helm, GitOps и помощника ИИ. Основные операции работают через
API Kubernetes: устанавливать `kubectl`, `flux` или `helm` для просмотра,
логов, shell и port-forward не нужно.

### Интерфейс на компьютере

Слева — навигатор: список кластеров и сворачиваемые группы ресурсов
(нагрузки, сеть, конфигурация, хранилище, доступ, кластер, GitOps,
пользовательские ресурсы по API-группам и избранное). Кнопка со стрелкой
сворачивает навигатор, а главное меню приложения при работе с кластером
сворачивается автоматически.

Кластер открывается на обзоре: узлы, запущенные поды, пространства имён,
проблемы, загрузка CPU и памяти с графиками и состояния подов. Метрики берутся
из metrics-server. Если его нет, обзор предложит установить базовую версию в
`kube-system`; для локальных кластеров с самоподписанными сертификатами kubelet
включите `--kubelet-insecure-tls`. Для production-кластера сначала откройте
окно записи.

Карточка объекта открывается поверх таблицы. Логи и терминал — в нижней
панели, помощник ИИ и активные пробросы портов — в правой панели.
Правый клик по кластеру открывает меню: переименовать, свойства, пометить как
тестовый или продовый, цвет метки, поделиться и экспорт kubeconfig.

### Подключение

1. Откройте Kubernetes и импортируйте kubeconfig из файла или вставьте YAML.
   На компьютере также можно выбрать несколько файлов и просканировать папку.
2. Отметьте нужные контексты. Проверьте адрес API, пользователя, namespace и
   способ авторизации. Исходный kubeconfig не изменяется.
3. Для рабочего production-кластера включите «Прод». При необходимости
   настройте маршрут через сохранённый SSH-хост или прокси.
4. Подключитесь к контексту. Права пользователя в Kubernetes продолжают
   действовать: интерфейс не выдаёт дополнительные права.

Токены и закрытые ключи находятся в зашифрованном хранилище. Подключения
синхронизируются между устройствами через сервер ConsoleCrypt в зашифрованном
виде. Запросы к Kubernetes идут с устройства к кластеру по выбранному маршруту;
сервер синхронизации не пересылает этот трафик.

На всех платформах доступны token, client certificate и поддерживаемый OIDC.
Внешние exec-плагины авторизации доступны только на компьютере. Перед импортом
показывается команда, аргументы и найденный бинарный файл; запуск требует
отдельного подтверждения его SHA-256. После замены файла подтверждение
запрашивается снова. На Windows используйте `.exe`: оболочки `.cmd` не
запускаются. Команды, требующие интерактивного stdin, не выполняются скрыто.
На телефоне выберите token, client certificate или OIDC.

В совместном доступе можно передать контекст проверенному устройству коллеги.
По умолчанию учётные данные не включаются: получатель использует свои.
Передача ключей или токенов требует отдельного явного выбора. Экспорт
kubeconfig тоже выполняется отдельно и создаёт файл, который может содержать
секреты; храните его как файл доступа к кластеру.

### Ресурсы и защита production

Выберите namespace и тип ресурса. Список обновляется через watch; поиск,
фильтры и настройки колонок помогают сузить выбор. В карточке доступны
состояние, YAML, события и связанные объекты. Для поддерживаемых workloads
можно открыть историю rollout и сравнить ревизии.

Изменение YAML проходит через предварительный просмотр отличий и dry-run:
«Применить» сначала проверяет изменения на сервере. Применение, удаление,
масштабирование и restart требуют действия пользователя. Конфликт владения
полями не перезаписывается без отдельного подтверждения; при отказе показано,
какой менеджер полей (например, контроллер) владеет полем или какие поля
сервер отклонил. У запущенного Pod можно менять только образ,
`activeDeadlineSeconds`, добавлять tolerations, а также метки и аннотации —
остальное меняется через владельца (Deployment, StatefulSet и т.п.).
Secret скрывает значения по умолчанию; просмотр значений выполняется отдельно.
Документ с маркерами `[redacted]` нельзя применить: скрытые значения не
заменяются текстом масок.

«Прод» принудительно ограничивает изменения в ядре, включая запросы внешних
CLI через локальный прокси. На компьютере можно явно открыть временное окно
записи на 10 минут — оно подтверждается паролем хранилища; на телефоне окно
недоступно. Каждое изменение (применение YAML, удаление, перезапуск,
масштабирование, действия GitOps, откат Helm, массовые операции) тоже
подтверждается паролем хранилища, а не вводом имени ресурса или контекста.
Массовые изменения запрещены в production даже при открытом окне. Exec и port-forward имеют отдельные разрешения
production-контекста.

### Логи, shell и туннели

В карточке Pod выберите контейнер и откройте логи. Доступны поток, фильтр,
временная граница и предыдущий запуск контейнера. Workload позволяет собрать
логи связанных Pod. Сохранение на компьютере и системный обмен на телефоне
выполняются по запросу.

Shell открывается во вкладке терминала. Автоматический выбор проверяет
`/bin/bash`, `/bin/ash`, затем `/bin/sh`; можно выбрать команду явно.
Копирование, поиск и управление терминалом используют общий интерфейс
ConsoleCrypt.

По умолчанию port-forward создаёт локальный адрес `127.0.0.1` и показывает состояние в
туннелях. На компьютере вне production можно отдельно разрешить другой
IP-интерфейс после предупреждения; на телефоне и в production это недоступно.
Остановка туннеля закрывает локальный сокет. На Android активные
туннели поддерживаются foreground service с уведомлением и кнопкой остановки.
Сборка и проверка Kubernetes на iOS отложены. Текущий выпуск включает
macOS, Windows, Linux и Android. Блокировка хранилища закрывает сессии.

Запущенные пробросы Kubernetes и Docker собраны в общей правой панели
«Проброс портов»: состояние, локальный адрес с копированием, трафик и
остановка. Панель открывается сама после запуска проброса.

### Подключение к базе данных

У Pod со стандартным портом базы данных (PostgreSQL, MySQL, MariaDB,
ClickHouse, Redis, MongoDB) есть кнопка «Подключиться к БД». Если в разделе
«Базы данных» уже сохранено подключение с тем же маршрутом, оно сразу
открывается. Иначе открывается форма с маршрутом через Kubernetes: для Pod
из StatefulSet — к самому Pod (у него постоянное имя), иначе к единственному
Service, выбирающему Pod, или к его Deployment, чтобы подключение пережило
перезапуск подов. Имя базы и пользователь подставляются из переменных
окружения контейнера, если они заданы явно; значения из Secret не читаются.
Проброс порта восстанавливается при каждом подключении к базе, а в
production действуют разрешения port-forward этого контекста.

### Helm и GitOps

Helm показывает историю релиза, values, манифест и различия между ревизиями.
Все строковые values скрываются; поля с именами учётных данных скрываются
независимо от типа. Вычисленные values могут быть
неполными, если сохранённый Helm release не содержит defaults зависимостей.
Сравнение с кластером читает живые объекты и сопоставляет поля сохранённого
манифеста; это не результат серверного defaulting и не заключение контроллера
о drift. Ограничения RBAC, неполные результаты и скрытые поля обозначаются явно.
Rollback доступен на компьютере через отдельно разрешённый установленный
`helm`.

GitOps показывает Flux sources, Kustomizations, HelmReleases, уведомления,
image automation и Argo CD Applications, если соответствующие CRD доступны.
Обзор, зависимости и «Поток ресурсов» помогают перейти от источника к workload.
«Синхронизировать сейчас» (flux reconcile), «Синхронизировать вместе с
источником», «Приостановить/Возобновить синхронизацию» (flux suspend/resume) и
Argo sync/refresh проходят подтверждение и ограничения production; подсказки
и окно подтверждения объясняют, что сделает Flux. Приостановка может сопровождаться
причиной и локальным напоминанием; напоминание само ничего не возобновляет.

Необязательные уведомления Flux включаются для контекста и работают внутри
открытого приложения. История событий тоже включается отдельно, хранится
локально в зашифрованной базе до семи дней и не синхронизируется. Недостаток
прав или ограничение объёма данных отображается явно.

### Терминал контекста и ИИ

На компьютере терминал контекста запускает выбранную локальную оболочку.
Его временный `KUBECONFIG` указывает на локальный прокси с отзывным доступом:
реальные ключи кластера не записываются в этот файл. Вкладка остаётся привязана
к своему контексту, выбранный namespace задаётся по умолчанию. Установка
дополнительных CLI требует подтверждения команды и способа установки.
Для пакетного менеджера источник и версию определяют его настройки.

WSL требует `/bin/bash` и доступен, когда выбранный дистрибутив может обратиться к защищённому
loopback-прокси Windows. Если сеть WSL этого не позволяет, запуск отменяется
с объяснением; приложение не открывает прокси на внешних интерфейсах.

ИИ использует выбранного в настройках провайдера. Перед доступом к данным
кластера требуется согласие; внешнему провайдеру отправляются отобранные
и очищенные данные. Инструменты ИИ читают ресурсы, события и хвосты логов.
Команда или YAML остаются предложением: выполнение проходит обычное
подтверждение, оценку риска и ограничения production. Чат привязан к контексту. В отличие от локальной истории событий, история
ИИ может синхронизироваться с E2EE при включённой настройке синхронизации чатов.

## English

The Kubernetes workspace combines cluster connections, live resources, logs,
Pod terminals, tunnels, Helm, GitOps and AI assistance. Browsing, logs, shell
and port forwarding use the Kubernetes API directly and require no installed
`kubectl`, `flux` or `helm`.

### Desktop layout

The navigator on the left lists clusters and collapsible resource groups
(workloads, network, configuration, storage, access, cluster, GitOps, custom
resources by API group and favourites). The arrow button collapses it, and the
app sidebar collapses automatically while a cluster is open.

A cluster opens on its overview: nodes, running Pods, namespaces, problems,
CPU and memory with charts, and Pod states. Metrics come from metrics-server.
When it is missing, the overview offers to install a basic one into
`kube-system`; turn on `--kubelet-insecure-tls` for local clusters whose
kubelets use self-signed certificates. Open a write window first on a
production cluster.

Object details open over the table. Logs and terminals live in the bottom
panel; the AI assistant and active port forwards live in the right panel.
Right-click a cluster for rename, properties, test or production marker,
colour label, sharing and kubeconfig export.

### Connect and share

Import a kubeconfig file or paste YAML, select contexts and review each API
address, user, namespace and authentication method. Desktop also supports
multiple files and folder scanning. The source configuration stays unchanged.
Mark production contexts and select a saved SSH or proxy route when needed.
Existing Kubernetes RBAC permissions continue to apply.

Tokens and private keys are stored in the encrypted vault. Connections sync
between your devices as encrypted objects. Kubernetes requests travel from
the device to the cluster through the selected route; the ConsoleCrypt sync
server does not forward that traffic.

Token, client certificate and supported OIDC authentication are available on
all platforms. External exec authentication plugins require desktop. Import
shows the command, arguments and resolved executable; execution separately
requires approval of its SHA-256. A changed executable requires new approval.
Windows accepts `.exe`, not `.cmd` wrappers. Plugins needing interactive stdin
are not silently executed. Use token, certificate or OIDC authentication on
mobile.

Share a context with a verified colleague’s device through Shared access.
Credentials are excluded by default, allowing recipients to supply their own.
Including tokens or keys requires explicit selection. Exporting a kubeconfig
is also explicit and can produce a file containing credentials.

### Browse and change resources

Select a namespace and resource kind. Watch keeps the list current; use search,
filters and column settings to narrow it down. Details include status, YAML,
events and related objects. Supported workloads expose rollout history and
revision comparison.

YAML editing includes a diff and dry-run: Apply validates on the server
first. Applying, deleting, scaling and restarting require user action; field
ownership conflicts are not overridden without separate approval, and a
refusal names the field manager (for example, a controller) that owns a field
or the fields the server rejected. A running Pod only accepts changes to its
image, `activeDeadlineSeconds`, added tolerations, labels and annotations;
change the rest through its owner (Deployment, StatefulSet and so on). Secret values are hidden until explicitly revealed. Documents containing
`[redacted]` markers cannot be applied, preventing masks from replacing values.

Production restrictions are enforced by the native core, including external
CLI requests through the local proxy. Desktop allows a 10-minute write window
confirmed with the vault passphrase; mobile does not. Every change (YAML apply,
delete, restart, scale, GitOps actions, Helm rollback, bulk actions) is also
confirmed with the vault passphrase instead of typing the resource or context
name. Bulk mutations remain prohibited in production even during a write window. Exec and port forwarding have separate
production permissions.

### Logs, shell and port forwarding

Choose a container in Pod details to view streaming logs, filter output, set
a time boundary or inspect the previous container run. Workload logs combine
related Pods. Export uses file saving on desktop and system sharing on mobile.

Pod shell opens in a terminal tab. Automatic selection tries `/bin/bash`,
`/bin/ash` and `/bin/sh`, or you can choose a command explicitly.

Port forwarding defaults to `127.0.0.1` and appears among tunnels. Desktop
non-production contexts can explicitly expose another IP interface after a
warning; mobile and production cannot. Stopping a
tunnel closes its listener. Android keeps active forwards in a foreground
service with a notification and stop action. The iOS build and Kubernetes
testing are deferred; this release targets macOS, Windows, Linux and Android.
Locking the vault closes sessions.

Running Kubernetes and Docker forwards share the right-hand “Port forwards”
panel with state, the local address and a copy button, traffic and Stop. The
panel opens by itself when a forward starts.

### Connect to a database

A Pod with a standard database port (PostgreSQL, MySQL, MariaDB, ClickHouse,
Redis, MongoDB) has a “Connect to database” button. A connection with the same
route already saved in Databases opens at once. Otherwise a form opens with a
Kubernetes route: to the Pod itself for a StatefulSet Pod (its name is
stable), otherwise to the only Service selecting the Pod or to its Deployment,
so the connection survives Pod restarts. The database name and user come from
literal container environment variables; values from Secrets are not read.
The forward is restored on every database connect, and production contexts
keep their port-forward permissions.

### Helm and GitOps

Helm exposes release history, values, manifests and revision differences.
All string values are masked, as are credential-named fields of other types. Computed values may be incomplete when a stored
Helm release lacks dependency defaults. Live comparison reads cluster objects
and compares the saved manifest’s fields; it is not server-side defaulting or
a controller drift verdict. RBAC limitations, partial results and masked fields are identified explicitly. Rollback requires desktop and a separately approved installed `helm`.

GitOps discovers available Flux sources, Kustomizations, HelmReleases,
notifications, image automation and Argo CD Applications. Overview,
dependencies and resource flow connect sources to workloads. Sync now
(flux reconcile), Sync with source, Pause/Resume sync (flux suspend/resume) and
Argo sync/refresh require confirmation and obey production restrictions; tooltips
and the confirmation explain what Flux will do. Suspension can include a reason and a local
reminder; reminders never resume resources automatically.

Optional Flux alerts operate within the running app. Optional event history
is kept in the local encrypted database for up to seven days and is not
synced. Missing permissions and data limits are reported explicitly.

### Context terminal and AI

A desktop context terminal runs your selected local shell with a temporary
`KUBECONFIG` pointing to a revocable local proxy. Actual cluster keys are not
written into that file. Each tab keeps its original context with the selected namespace as its default.
Additional CLI installation requires approval of the command and installation
method. Package sources and versions follow the package manager’s configuration.
WSL requires `/bin/bash` and the selected distribution to reach the protected Windows loopback
proxy; otherwise launch is refused without exposing an external listener.

AI uses the provider selected in settings and asks for consent before accessing
cluster data. External providers receive selected, sanitized data. AI tools
read resources, events and log tails. Suggested commands and YAML require the
normal user approval, risk checks and production policy. Chats stay bound to
their cluster context. Unlike local event history, AI history can sync with E2EE
when conversation sync is enabled.
