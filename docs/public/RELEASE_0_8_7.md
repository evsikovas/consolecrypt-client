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

See the [Databases guide](DATABASES.md) and the [Kubernetes guide](KUBERNETES.md).
