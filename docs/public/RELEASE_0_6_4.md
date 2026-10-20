# ConsoleCrypt 0.6.4

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

**Kubernetes на компьютере переработан.** Слева навигатор с кластерами и
сворачиваемыми группами ресурсов (нагрузки, сеть, конфигурация, хранилище,
доступ, кластер, GitOps, пользовательские ресурсы по API-группам и избранное).
Первый экран кластера — живой обзор: узлы, запущенные поды, пространства имён,
проблемы, загрузка CPU и памяти с графиками и состояния подов. Если в кластере
нет metrics-server, базовую версию можно установить из обзора (для локальных
кластеров есть режим `--kubelet-insecure-tls`). Карточка объекта открывается
поверх таблицы, ИИ-помощник кластера — в правой панели, нижняя панель логов и
терминала не сжимает таблицу.

- Меню кластера по правому клику: переименовать, свойства, пометить как
  тестовый или продовый, цвет метки, поделиться через сервер синхронизации и
  экспорт kubeconfig.
- Окно записи в прод-кластере подтверждается паролем хранилища.
- Правка YAML сама проверяет изменения на сервере. Поды, созданные
  контроллерами, теперь редактируются; при отказе показано, какой контроллер
  владеет полем или какие поля сервер отклонил.
- Таблица и обзор показывают настоящий статус подов (Running, Completed,
  CrashLoopBackOff); у объектов без статуса (Service, ConfigMap) колонки нет.
- GitOps: обзор больше не падает, граф «Поток ресурсов» строится слева направо
  с учётом циклов Flux.
- Пробросы портов — отдельный инструмент правой панели; история событий —
  карточками с раскрывающимся текстом.
- ИИ Kubernetes работает со встроенной локальной моделью и сокращает контекст
  для небольших моделей.

**Терминал и подключения.**

- Шрифт терминала выбирается из установленных моноширинных шрифтов.
- Меню вкладки: переименовать, дублировать, закрыть другие и справа.
- Фоновая вкладка с новым выводом подсвечивается, в боковом меню —
  отдельный счётчик таких вкладок.
- Подпись вкладки: имя из списка, адрес или имя компьютера, либо ничего.
- Импорт хостов из `~/.ssh/config` (Include, маски Host, ProxyJump и ключи
  из IdentityFile).
- Горячие клавиши переназначаются в настройках, с предупреждением о
  конфликтах и сбросом к стандартным.

**Интерфейс.** Ответы ИИ отображают Markdown. Внизу бокового меню видно, какой
ИИ отвечает: встроенная модель, локальный сервер или API. Защищённые диалоги
окрашены в цвета темы; страницы настроек, синхронизации и резервных копий
растягиваются на всю ширину.

Подробности: [руководство Kubernetes](KUBERNETES.md).

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.

**Kubernetes on desktop is redesigned.** A navigator on the left lists
clusters and collapsible resource groups (workloads, network, configuration,
storage, access, cluster, GitOps, custom resources by API group and
favourites). A cluster opens on a live overview: nodes, running Pods,
namespaces, problems, CPU and memory with charts, and Pod states. When the
cluster has no metrics-server, a basic one can be installed from the overview
(with `--kubelet-insecure-tls` for local clusters). Object details open over
the table, the cluster AI assistant lives in the right panel, and the bottom
logs and terminal panel no longer squeezes the table.

- Cluster context menu: rename, properties, mark as test or production,
  colour label, share through the sync server and export kubeconfig.
- A production write window is confirmed with the vault passphrase.
- YAML editing validates on the server by itself. Pods created by controllers
  can now be edited; a refusal names the field manager that owns a field or
  the fields the server rejected.
- Tables and the overview show the real Pod status (Running, Completed,
  CrashLoopBackOff); kinds without a status (Service, ConfigMap) have no status
  column.
- GitOps: the overview no longer crashes, and the resource flow graph reads
  left to right, handling Flux bootstrap cycles.
- Port forwards are a tool of the right panel; the event history shows cards
  with expandable messages.
- Kubernetes AI works with the built-in local model and trims context for
  small models.

**Terminal and connections.**

- The terminal font is chosen from installed monospace fonts.
- The tab menu can rename, duplicate, close others and close tabs to the right.
- A background tab with new output is highlighted, with a separate count in
  the sidebar.
- The tab subtitle shows the list name, the address or computer name, or
  nothing.
- Hosts can be imported from `~/.ssh/config` (Include, Host patterns,
  ProxyJump and IdentityFile keys).
- Keyboard shortcuts can be remapped in Settings, with conflict warnings and a
  reset to defaults.

**Interface.** AI answers render Markdown. The bottom of the sidebar shows
which AI answers: the built-in model, a local server or an API. Secure dialogs
follow the theme colours; the settings, sync and backups pages use the full
width.

See the [Kubernetes guide](KUBERNETES.md) for details.
