# Базы данных / Databases

## Русский

Раздел «Базы данных» работает с PostgreSQL, MySQL и MariaDB, ClickHouse, Redis,
MongoDB и файлами SQLite. В нём структура базы, редактор запросов с вкладками и историей,
таблица результатов, EXPLAIN, правка ячеек, копирование и экспорт, а также
ИИ-помощник. Драйверы встроены в приложение, поэтому `psql`, `mysql`,
`clickhouse-client`, `redis-cli` и `mongosh` устанавливать не нужно. Раздел
доступен на macOS, Windows, Linux и Android; сборка и проверка iOS отложены.

### Подключения

1. Откройте «Базы данных» в боковом меню и нажмите «Новое подключение к БД».
2. Укажите имя, движок, адрес, порт, базу (для Redis — номер базы),
   пользователя и пароль. Пароль можно сохранить в хранилище или вводить при
   каждом открытии: введённый так пароль используется один раз и не
   сохраняется.
3. «Защита TLS»: «Проверять сертификат и имя сервера» (по умолчанию),
   «Требовать шифрование», «Предпочитать шифрование» или «Отключить TLS». Для
   локального адреса и проброса порта режим сам переключается на
   «Предпочитать шифрование». Через любой маршрут TLS проверяет исходное имя
   сервера базы.
   - «Предпочитать шифрование» — TLS, если сервер его поддерживает; без TLS
     только через туннель (SSH-хост, jump-профиль, SSH-туннель, проброс
     Kubernetes или Docker) или на этом компьютере (адрес loopback после
     разрешения имени). Напрямую или через прокси к другому хосту режим
     работает как «Требовать»: если сервер не поддерживает TLS, подключение
     завершается ошибкой «Сервер не поддерживает TLS», и пароль не уходит.
     Старые подключения с этим режимом к удалённой базе без TLS получат эту
     ошибку — выберите «Отключить TLS» явно, если это допустимо.
   - «Требовать шифрование» — шифрование без проверки сертификата. В
     PostgreSQL пароль тогда используется только через SCRAM-SHA-256-PLUS
     (channel binding): подменивший сертификат перехватчик не получит пароль.
     Если сервер не умеет SCRAM-PLUS (старые версии, `md5`/`password` в
     `pg_hba.conf`), появится ошибка и переключатель «Разрешить вход без
     привязки к TLS (менее безопасно)», по умолчанию выключен. MySQL/MariaDB
     такой привязки не имеют: от активного перехватчика пароль защищает только
     проверка сертификата.
   - Без TLS по сети PostgreSQL не отвечает на запрос пароля открытым текстом
     (`AuthenticationCleartextPassword`); через туннель и на этом компьютере
     он разрешён. MySQL/MariaDB никогда не включают `mysql_clear_password`.
     ClickHouse, Redis и MongoDB не переходят на открытый текст ни в каком
     режиме, кроме «Отключить TLS».
   Фактическая защита видна в шапке рабочей области и на карточке
   подключения: «TLS (проверен)», «TLS (без проверки сертификата)», «Без TLS —
   защищено туннелем» или «Без TLS».
4. Выберите маршрут (см. ниже) и при необходимости отметьте «Прод — всегда
   только чтение».
5. «Проверить подключение» показывает версию сервера до сохранения.

В блоке «Группа» задаются группа, метки и цвет. Цвет виден на плитке
подключения и в шапке рабочей области. Список подключений показывается
плитками или компактным списком — переключатель справа от поиска, как в
разделе «Хосты»; выбор запоминается на этом устройстве. Поиск находит
подключение по имени, адресу, базе, движку и меткам, метки над списком
работают как фильтры.

Подключение можно импортировать из строки (`postgres://`, `mysql://`,
`clickhouse://`, `redis://`, `mongodb://`, `mongodb+srv://`) или из файла
`.pgpass`. Перед сохранением показывается список найденных подключений: для
каждого можно включить «Прод», выбрать маршрут и решить, сохранять ли пароль.
Строки с подстановками `*` и локальными сокетами пропускаются.

Подключения и пароли хранятся в зашифрованном хранилище и синхронизируются
через сервер ConsoleCrypt в зашифрованном виде. Запросы идут с устройства к
базе по выбранному маршруту; сервер синхронизации их не видит.

### Маршруты

- **Напрямую** — к адресу и порту базы.
- **SSH-хост** или **профиль SSH-переходов** — через сохранённые хосты
  ConsoleCrypt, включая их jump-цепочки.
- **SSH-туннель** — через сохранённый туннель локального проброса. Адрес и
  порт берутся из цели туннеля; если туннель не запущен, он стартует при
  подключении — всегда на 127.0.0.1, даже если в туннеле указан сетевой
  адрес, чтобы база не стала доступна другим компьютерам. Если в форме указан
  localhost с портом сохранённого туннеля, форма предложит использовать этот
  туннель.
- **Kubernetes** — проброс порта к Pod, Service, Deployment или StatefulSet
  выбранного контекста. Для Service и Deployment выбирается готовый Pod,
  поэтому подключение переживает перезапуск подов.
- **Контейнер Docker** — проброс порта в контейнер на этом компьютере или на
  сохранённом SSH-хосте. Контейнер ищется по имени, так что пересозданный
  контейнер тоже подойдёт.

Пробросы Kubernetes и Docker восстанавливаются при каждом подключении, а уже
запущенные используются повторно. Значок рядом с именем подключения
показывает маршрут.

Подключиться можно и из других разделов: кнопка «Подключиться к БД» есть у
подов Kubernetes и контейнеров Docker со стандартными портами баз данных. Если
подключение с таким маршрутом уже сохранено, оно открывается сразу; иначе
открывается форма с заполненными движком, портом и маршрутом.

### SQLite

SQLite — это файл, а не сервер. В форме подключения выберите движок «SQLite»,
затем «Где находится файл»:

- **Этот компьютер** (macOS, Windows, Linux) — путь к файлу или кнопка
  «Выбрать файл…». Новый пустой файл создаётся только кнопкой «Создать новую
  базу…»; подключение к несуществующему файлу не создаёт его, а сообщает,
  что файл не найден. Перед открытием проверяется заголовок: другой файл
  (или зашифрованная база) не открывается. Поддерживается режим WAL.
- **SSH-хост**, **Docker-контейнер** или **Под Kubernetes** — запросы
  выполняет программа `sqlite3` (версии 3.16 или новее) на хосте, в
  контейнере или поде; файл никуда не копируется. Запрос передаётся через
  стандартный ввод, а не в командной строке. Если `sqlite3` нет, приложение
  так и скажет. Для продакшн-контекста Kubernetes нужно разрешение на
  выполнение команд в его настройках.

«Только чтение» включено по умолчанию: файл открывается только для чтения, и
ни один запрос не может его изменить. Без этого флажка действуют обычные
правила: на непродовом подключении изменения проходят проверку, на продовом
нужно временное окно записи. Даже разрешённое чтение выполняется с
`PRAGMA query_only`; `ATTACH`/`DETACH`, `VACUUM INTO`, `load_extension`,
`readfile`/`writefile` и изменяющие PRAGMA (ключи шифрования, `query_only`,
`writable_schema`) запрещены; удалённая программа запускается с `-readonly`
и `-safe`.

Структура: файл → «Таблицы», «Представления», «Индексы», «Последовательности»
(счётчики AUTOINCREMENT), «Триггеры таблиц» и «Типы данных»; таблица →
«Колонки» («имя (ТИП)»), «Ключи», «Внешние ключи», «Индексы», «Ссылки»
(внешние ключи других таблиц на эту) и «Триггеры». Доступны DDL, сведения об
объекте (размер файла, оценка строк из `sqlite_stat1`, STRICT, WITHOUT
ROWID), план `EXPLAIN QUERY PLAN` деревом, подсказки, экспорт (в т. ч.
INSERT с литералами SQLite) и правка ячеек по первичному ключу или `rowid`.

Подключения SQLite хранятся **только на этом устройстве** (значок «Только на
этом устройстве»): они не синхронизируются, не попадают в хранилище и
резервные копии и не передаются другим — версии приложения до 0.9.1 не умеют
их читать. История их запросов тоже остаётся локальной.

### Режим «Прод»

Прод-подключение открывается только на чтение, и защита у него в два слоя:

- **На сервере.** PostgreSQL и MySQL/MariaDB выполняют каждый запрос в
  транзакции только для чтения. ClickHouse получает `readonly=2` в каждом
  запросе. У Redis серверного переключателя нет. Для MongoDB проверяются роли
  пользователя.
- **В приложении.** Каждый запрос разбирается до отправки. Изменяющие команды и
  попытки снять защиту (`SET transaction_read_only`, `COPY … TO PROGRAM`,
  `INTO OUTFILE`, `FLUSHALL`, `$out`, `$merge` и подобные) не отправляются.

Когда изменить прод всё же нужно, кнопка **«Разблокировать прод»** в шапке
рабочей области открывает временное окно записи. Нужно ввести пароль хранилища
(он проверяется в ядре приложения) и выбрать срок: 5, 15 или 30 минут. Пока окно открыто, в шапке горит красная метка «Запись в
прод до ЧЧ:ММ» с обратным отсчётом и кнопкой «Заблокировать». Каждый
изменяющий запрос по-прежнему показывается на проверку, и для него нужно
ввести пароль хранилища; ячейки результата можно редактировать. Окно
принадлежит только этому сеансу и закрывается само по истечении срока, по
кнопке «Заблокировать», при отключении, блокировке хранилища или смене
профиля. Серверная защита снимается только для разрешённых изменений: чтения
по-прежнему идут в транзакциях только для чтения. Роль только для чтения
сервер не пропустит и в открытом окне — после разблокировки приложение
проверяет права и предупреждает, если роль не может писать. Изменения,
сделанные в окне, отмечаются в истории запросов. На телефонах окно записи
недоступно.

Кнопка «Проверить права» в шапке рабочей области сообщает, подтверждена ли
защита на сервере и есть ли у роли права записи. Для прода лучше использовать роль только для
чтения: в PostgreSQL — `SELECT` или `pg_read_all_data`, в MySQL — `SELECT`, в
ClickHouse — профиль `readonly=1`, в Redis — ACL `+@read -@write`, в MongoDB —
роль `read`.

Непрод-подключение тоже открывается в режиме «Только чтение». Кнопка с замком
в шапке разрешает запись до закрытия подключения. Изменяющий запрос перед
выполнением показывается в окне проверки целиком, и выполнить его можно только
после ввода пароля хранилища. Это касается и опасных запросов (`DROP`,
`TRUNCATE`, `UPDATE` или `DELETE` без `WHERE`, `FLUSHALL`, удаление
коллекции), и правок ячеек: в проверке правок видны сами новые значения.
Команда Redis `KEYS` не выполняется никогда: вместо неё предлагается такой же
`SCAN`, который можно добавить в редактор.

### Общий доступ

Если сервер синхронизации поддерживает общий доступ к подключениям баз данных
(сервер 0.1.15 и новее с включённой функцией), в меню подключения появляется
**Поделиться**. Перед отправкой показывается, какие поля уйдут коллеге:
движок, адрес, порт, база, пользователь, TLS, лимиты и режим «Прод». Группа,
история запросов, настройки ИИ и разрешения сеанса не передаются. Пароль и ключ
клиента передаются только при явной отметке и только если сервер поддерживает
общие секреты.

Маршрут через SSH-хост, профиль переходов, прокси или туннель не передаётся:
получатель выбирает свой. Для проброса Kubernetes сохраняются пространство
имён и нагрузка, а контекст получатель выбирает свой; для Docker сохраняется
имя контейнера, а движок выбирает получатель. Подключение с таким маршрутом
нельзя сохранить как прямое.

Полученные подключения показываются в разделе с меткой **Общее**, свои
опубликованные — с меткой **Вы поделились**. **Подробнее** открывает принятие,
сохранение личной копии, участников, отзыв доступа и прекращение общего
доступа. Копия прод-подключения остаётся прод-подключением только для чтения.
Отзыв доступа не отменяет уже скопированный пароль — смените его на сервере
базы данных.

### Рабочая область

Нажмите на подключение: слева откроется структура, справа — вкладки
запросов, редактор и результаты. Границы панелей перетаскиваются, кнопки в
шапке скрывают структуру и открывают историю и ИИ-помощника.

Структура загружается по мере раскрытия и раскладывает объекты по папкам.
Раскрытая таблица обведена рамкой, первичные и внешние ключи отмечены
значками. Поиск фильтрует уже загруженные объекты, соседняя кнопка
сворачивает всё дерево.

- **PostgreSQL**: схемы → таблицы, представления, материализованные
  представления, последовательности, функции и процедуры → колонки (тип,
  NULL, значение по умолчанию, ключи), индексы, ограничения, триггеры.
- **MySQL/MariaDB**: базы → таблицы, представления, процедуры и функции →
  колонки, индексы, триггеры.
- **ClickHouse**: базы → таблицы (движок), представления, словари → колонки
  (ключ сортировки), skip-индексы и проекции.
- **Redis**: `db0`…`db15` → папки ключей по разделителю `:` → ключи с типом и
  TTL. Ключи читаются порциями через `SCAN`.
- **MongoDB**: базы → коллекции → индексы.

Двойной щелчок по таблице, представлению, коллекции или ключу Redis и пункт
«Первые 100 строк» сразу показывают данные внизу. Для этого есть одна
вкладка просмотра («items — данные»): следующий объект открывается в ней же,
новые вкладки не плодятся. Если изменить текст этой вкладки, она становится
обычной и больше не перезаписывается.

В меню объекта: «Первые 100 строк», «Показать DDL» (для Redis — описание
ключа, для MongoDB — параметры коллекции и индексы), «Вставить имя» и
«Сведения об объекте». DDL показывается блоком кода с номерами строк и
подсветкой, каждый оператор (`CREATE TABLE`, каждый `CREATE INDEX`) отделён
чертой. Окно подстраивается под ширину экрана и по высоте не больше кода.
Длинные строки по умолчанию переносятся с отступом, номер стоит на первой
строке; кнопка «Переносить строки» выключает перенос, и тогда код
прокручивается вбок (настройка запоминается). Кнопки «Копировать» и «Открыть
в редакторе». Для коллекций MongoDB есть «Схема по выборке документов»: типы
полей и доля документов, в которых поле встречается.

«Сведения об объекте» показывают факты из системного каталога, только чтением
и с параметрами вместо подстановки имён: для таблиц и представлений
PostgreSQL — схему, владельца, вид, оценку числа строк, размеры (всего,
данные, индексы, TOAST), число колонок, индексов, ограничений и триггеров,
табличное пространство, комментарий, ключ секционирования и последние
VACUUM/ANALYZE; для MySQL/MariaDB — движок, формат строк, оценку строк,
размеры данных и индексов, AUTO_INCREMENT, сопоставление, время создания и
изменения; для ClickHouse — движок, строки, размер, части, ключи
секционирования, сортировки и выборки, TTL; для ключа Redis — тип, TTL,
кодировку, длину и память, для базы Redis — число ключей. Оценки помечены
«≈», размеры показаны в КиБ/МиБ, кнопка «Копировать» копирует всё окно. Для
MongoDB описание показывается как JSON. Пункт скрыт у объектов, для которых
сведений нет.

Сгенерированный текст — DDL из «Открыть в редакторе», запрос из истории,
черновик помощника, предложенный `SCAN` — добавляется в текущую вкладку
ниже уже написанного, через пустую строку; курсор встаёт в начало вставки,
ничего не выполняется. Если открыта вкладка просмотра, текст уходит в
последнюю вкладку редактора; новая вкладка создаётся, только если других нет.

### Редактор и выполнение

Редактор нумерует строки, подсвечивает синтаксис, дополняет ключевые слова,
таблицы и колонки у курсора (в том числе по псевдониму: `o.` после
`FROM orders o`), форматирует запрос, комментирует строки и ищет с заменой.
MongoDB принимает команды в привычном синтаксисе оболочки, например
`db.orders.find({ status: "paid" })`.

| Действие | macOS | Windows / Linux |
| --- | --- | --- |
| Выполнить выделение или оператор под курсором | `⌘↵` | `Ctrl+Enter` |
| Выполнить весь скрипт | `⌥X` или `⇧⌘↵` | `Alt+X` или `Ctrl+Shift+Enter` |
| Дополнить у курсора | `⌃Space` | `Ctrl+Space` |
| Форматировать | `⇧⌘F` | `Ctrl+Shift+F` |
| Закомментировать строки | `⌘/` | `Ctrl+/` |
| Поиск и замена | `⌘F` | `Ctrl+F` |
| Отменить запрос | `⌘.` | `Ctrl+.` |
| EXPLAIN | `⌘E` | `Ctrl+E` |
| История | `⌘H` | `Ctrl+H` |
| ИИ-помощник | `⌘I` | `Ctrl+I` |

Без выделения выполняется оператор на строке курсора, как в классических
SQL-клиентах: операторы разделяются `;` (вне строк и комментариев) и пустыми
строками; с любой строки многострочного оператора выполняется он целиком;
если на строке два оператора, выбирается тот, что под курсором; на пустой
строке — ближайший оператор выше (если его нет — ниже). В Redis и MongoDB
правило то же.

Весь скрипт выполняется по одному оператору и останавливается на первой
ошибке. Переменные `{{name}}` заполняются в отдельном окне и передаются
драйверу как параметры, а не вставляются в текст.

EXPLAIN показывает план деревом: у каждого шага строки, стоимость и полоса —
доля самого шага; кнопка «JSON» открывает исходный план. «Выполнить с
ANALYZE» добавляет реальное время, но выполняет запрос, поэтому доступно
только для непрод-подключений и требует подтверждения.

### Результаты, копирование и экспорт

Результат — компактная таблица с номерами строк: сортировка и фильтр по
колонке, закрепление и скрытие колонок, просмотр полного значения. Сетка
сначала загружает часть строк (по умолчанию 1000, лимит задаётся в
«Ограничениях» подключения), остальное подгружается кнопкой «Загрузить ещё».
На телефоне результат показывается карточками записей. Значения колонок с
именами вроде `password`, `token`, `secret` скрыты, а при копировании буфер
обмена очищается автоматически.

Щелчок по ячейке выделяет её рамкой (`⌘C` / `Ctrl+C` копирует значение),
щелчок по номеру строки выбирает строку, Shift+щелчок расширяет выбор,
⌘/Ctrl+щелчок добавляет или убирает строку, Esc снимает выбор. В строке
состояния видно «Выбрано строк: N»; копирование, экспорт и ИИ-помощник
работают с выбранными строками, если они есть.

Копировать можно ячейку, строку, выделение или весь загруженный результат в
CSV, TSV, JSON, NDJSON, Markdown или как SQL `INSERT`. «Экспорт результата»
заново выполняет запрос и записывает все строки без лимита сетки, с
прогрессом и отменой. Экспортируются только запросы на чтение: изменяющий
запрос для экспорта не повторяется.

В CSV и TSV флажок «Экранировать формулы (для Excel)» (включён по умолчанию)
добавляет апостроф перед текстом, который начинается с `=`, `+`, `-`, `@`,
табуляции или возврата каретки, чтобы табличный редактор не выполнил его как
формулу; числа из числовых столбцов не меняются. Строки в SQL `INSERT`
экранируются удвоением кавычки (`''`); для MySQL, MariaDB и ClickHouse
обратная косая черта удваивается, для PostgreSQL текст с ней записывается как
`E'…'`, так что файл читается одинаково при любых настройках экранирования.

### Панель результата

Слева от результата — переключатель вида: «Таблица» (сетка), «Текст»
(выровненная текстовая таблица моноширинным шрифтом, её можно выделить и
скопировать; показывает до 2000 строк, NULL — как `[NULL]`, скрытые значения —
точками) и «Запись» (одна строка формой «поле → значение»). Выбранный вид
запоминается для вкладки. В заголовках колонок — значок типа (число, текст,
дата и время, да/нет, JSON, двоичные данные) и ключ у ключевых колонок; NULL
показан приглушённым `[NULL]`.

Под результатом — панель инструментов:

- «Обновить» (F5, ⌘R / Ctrl+R) заново выполняет последний читающий запрос
  вкладки после новой проверки; меню рядом включает автообновление (выкл.,
  5 с, 10 с, 30 с, 1 мин, 5 мин). Автообновление работает только для
  читающих запросов (в том числе на проде) и пропускает такт, пока есть
  несохранённые изменения, запрос ещё выполняется или окно скрыто.
- «Сохранить» (⌘S / Ctrl+S) применяет несохранённые изменения, в меню —
  «Показать SQL»; «Отменить изменения» (⌥⌘Z / Ctrl+Alt+Z) сбрасывает их.
- Строки: «Редактировать текущую строку» (⇧↵, открывает вид «Запись»),
  «Добавить строку» (⌥⌘N / Ctrl+Alt+N), «Дублировать строку» (⌘D / Ctrl+D),
  «Удалить строку» (⌘⌫ / Ctrl+Delete; с выделением — все выбранные строки).
- Переход: первая (⌘↑ / Ctrl+Home), предыдущая (⌥↑ / Alt+↑), следующая
  (⌥↓ / Alt+↓) и последняя (⌘↓ / Ctrl+End) строка; в сетке текущую строку
  двигают и стрелки ↑/↓.
- «Экспорт данных…» (⇧⌘E / Ctrl+Shift+E), «Импорт данных…» (CSV в таблицу
  результата, см. ниже).
- Шестерёнка и поле «Строк за раз» задают размер следующей порции (1–1000,
  запоминается для подключения); «Загрузить следующую порцию» и «Загрузить
  все строки» (в пределах лимита памяти результата).

Строка состояния: сколько строк получено, время выполнения, дата и время
окончания, «есть ещё», если строки остались на сервере, число выбранных
строк и несохранённых изменений.

### Изменение строк

На компьютере в сессии с разрешённой записью (непрод-подключение или прод с
открытым окном записи) строки результата простого `SELECT` из одной таблицы
можно менять, добавлять и удалять. Строка определяется первичным ключом, а
если его нет — первым уникальным ключом из NOT NULL-колонок (адрес строки
вроде `ctid` нестабилен и не используется). Без такого ключа, для ClickHouse,
Redis и MongoDB, а также на телефоне кнопки изменения неактивны, а подсказка
объясняет почему.

- Двойной щелчок по ячейке (или Enter/F2) — правка на месте: Enter оставляет
  значение в списке изменений, Esc отменяет, Tab переходит к следующей
  ячейке. Тип берётся из колонки и проверяется при вводе, NULL — отдельный
  переключатель. Изменённые ячейки подсвечены жёлтым.
- Новая строка подсвечена зелёным, номер строки — «+». Колонки, которые
  заполнит сервер (значение по умолчанию, identity, auto-increment), показаны
  как `[ПО УМОЛЧАНИЮ]`, остальные пустые — `[NULL]`. Обязательные колонки
  (NOT NULL без значения по умолчанию) нужно заполнить: иначе «Сохранить»
  подсветит их и ничего не отправит. Вычисляемые колонки не записываются.
- «Дублировать» копирует строку без ключевых, вычисляемых и скрытых значений.
- Удаляемая строка зачёркнута и подсвечена красным до сохранения; повторное
  «Удалить» снимает отметку, новая строка удаляется сразу.

До «Сохранить» ничего не выполняется. Ядро строит из изменений
параметризованные `DELETE`, `UPDATE` и `INSERT` (в этом порядке) по ключу,
а проверка показывает те же операторы со значениями-литералами и требует
пароль хранилища; защита прода не обходится. После подтверждения операторы
выполняются одной транзакцией, и каждый должен изменить ровно одну строку —
иначе откатывается всё. Затем результат обновляется. «Показать SQL»
открывает операторы в виде кода: их можно скопировать или добавить в текущую
вкладку редактора. Если при повторном запуске, обновлении, смене набора
результатов, закрытии вкладки или выходе из подключения есть несохранённые
изменения, приложение спрашивает: сохранить, отменить или остаться.

### Импорт из CSV

Для PostgreSQL, MySQL и MariaDB на компьютере данные из CSV можно загрузить в
таблицу: «Импорт данных…» на панели результата (в таблицу результата, если
это запрос к одной таблице с ключом) или «Импорт из CSV…» в меню таблицы в
дереве. Нужна разрешённая запись (на проде — открытое окно записи). Мастер:

1. **Файл** — CSV, TSV или текст до 200 МБ; файл читается потоком и никуда,
   кроме этой базы, не передаётся.
2. **Параметры чтения** с живым просмотром первых 100 строк: кодировка (UTF-8
   по умолчанию, BOM распознаётся; Windows-1251, Latin-1), разделитель
   (определяется автоматически: `,` `;` TAB `|`), символ кавычки, строка
   заголовка, пропуск строк в начале, что считать NULL (пустое поле, `NULL`,
   `\N`), десятичный разделитель и порядок дня, месяца и года в датах.
3. **Столбцы** — каждый столбец файла попадает в столбец таблицы (по имени
   без учёта регистра) или пропускается; первые 1000 строк проверяются на
   соответствие типам, проблемные значения показаны примерами, а
   обязательные столбцы без значений — предупреждением.
4. **Параметры импорта** — добавлять; добавлять, пропуская существующие
   ключи; добавлять или обновлять по ключу (PostgreSQL `ON CONFLICT`,
   MySQL `ON DUPLICATE KEY UPDATE`); размер пакета; при ошибочной строке —
   остановить импорт или пропустить её и записать отчёт
   `<файл>.errors.csv`; одна транзакция на весь файл или по пакету;
   «Сначала удалить все строки таблицы» (выключено по умолчанию; это
   `DELETE FROM` в той же транзакции, и он виден при проверке).
5. **Проверка и запуск** — как любое изменение: операторы с одной строкой
   параметров, пароль хранилища, защита прода. Затем идёт загрузка
   параметризованными многострочными `INSERT` с прогрессом (строк в секунду,
   оставшееся время) и отменой — открытая транзакция откатывается. В конце —
   итог: импортировано, пропущено, с ошибками, и ссылка на отчёт.

Значения, похожие на формулы, импортируются как есть (экранирование формул —
задача экспорта). Пока идёт импорт, другие запросы этого подключения ждут.
ClickHouse, MongoDB и Redis импорт из CSV пока не поддерживают.

В меню схемы (PostgreSQL) или базы (MySQL) есть «Создать таблицу из CSV…»:
по первым строкам файла строится простой `CREATE TABLE` (целые, десятичные,
даты, дата и время, да/нет, иначе текст), который можно скопировать или
открыть в редакторе и выполнить через обычную проверку.

### История

История запросов ведётся по каждому подключению: текст, время,
длительность, число строк и ошибка. Значения параметров и данные результатов
в неё не попадают. Фильтры: все запросы, успешные, с ошибками, избранное. По
умолчанию история хранится только на этом устройстве в зашифрованном виде; её
можно включить в зашифрованную синхронизацию или отключить. Запрос из истории
можно сохранить как сниппет: переменные `{{name}}` станут переменными
сниппета.

### Сниппеты для баз данных

В разделе «Сниппеты» → «Базовые наборы» есть готовые наборы диагностических
запросов: «SQL · PostgreSQL» (20), «SQL · MySQL/MariaDB» (20),
«SQL · ClickHouse» (20), «Redis» (15) и «MongoDB» (15). Они добавляются в
хранилище только по нажатию кнопки, после чего их можно править и удалять как
обычные сниппеты. В наборах: активные и долгие запросы, дерево блокировок,
размеры таблиц и баз, использование индексов, мёртвые строки и autovacuum,
репликация и слоты, изменённые настройки, тяжёлые запросы
(`pg_stat_statements`, дайджесты `performance_schema`, `system.query_log`),
куски, слияния и мутации ClickHouse, память и медленные команды Redis,
`currentOp`, статистика и профилировщик MongoDB. Почти все запросы только
читают; `EXPLAIN ANALYZE` (выполняет запрос), отмена запроса и `KILL`
помечены как изменяющие, завершение сессии PostgreSQL — как разрушительное, и
перед запуском требуют подтверждения. В описании каждого сниппета указаны
клиент (psql, mysql, clickhouse-client, redis-cli, mongosh) и требования к
версии или расширению.

Тип и диалект сниппета совпадают с теми, что использует история: PostgreSQL —
`postgresql`, MySQL/MariaDB и ClickHouse — `sql` с диалектом `mysql` /
`clickhouse`, Redis — `redis_cli`, MongoDB — `shell` с `mongosh`. Поэтому
сниппеты подходят и для SSH-терминала: запустите в сессии psql, mysql,
clickhouse-client, redis-cli или mongosh и выберите сниппет — после заполнения
переменных и подтверждения риска он уйдёт в активную вкладку как текст (не
через `sh -c`); многострочный запрос отправляется одной вставкой в режиме
bracketed paste, если клиент его включил. Новую SSH-сессию для такого сниппета
приложение не открывает. Команды администрирования сервера (например
`CLIENT LIST`, `SLOWLOG`, `db.currentOp()`, `rs.status()`) рабочая область
«Базы данных» не разрешает, поэтому они помечены как «только для терминала».

### ИИ-помощник

Помощник работает с провайдерами из «Настройки → ИИ»: встроенной моделью,
локальным сервером или внешним API. Он умеет:

- составить запрос по описанию;
- объяснить или исправить запрос;
- оптимизировать запрос по плану EXPLAIN;
- перевести запрос в другой диалект;
- сделать из запроса сниппет с переменными;
- предложить типовые запросы к таблице;
- объяснить выбранные строки результата.

Перед каждым запросом к модели открывается окно «Проверьте контекст ИИ»; по
умолчанию все пункты выключены:

- **Метаданные схемы** — имена таблиц и колонок с типами, без данных.
- **Выбранные строки** — только те, что вы отметили в результате.
- **Выборка строк** и **проверочный запрос** — модель может посмотреть до 20
  строк таблицы или выполнить один запрос на чтение (до 50 строк, 10 секунд).
  Запись этим путём невозможна даже на непрод.

Без согласия модель не получает ни схему, ни данные. Пароли, строки
подключения и адреса серверов не отправляются никогда. Колонки с секретами
скрываются. Для внешних провайдеров с профилем приватности «Строгий» имена
таблиц и колонок заменяются псевдонимами, а в ответе восстанавливаются.
Согласие можно отозвать кнопкой в панели помощника.

Ответ — черновик с пояснением и списком переданного контекста. «Принять
черновик» добавляет его в текущую вкладку редактора; сам помощник ничего не
выполняет — запрос запускаете вы, с обычными проверками. Для
прод-подключений черновики DDL только показываются.

## English

The Databases section works with PostgreSQL, MySQL and MariaDB, ClickHouse,
Redis, MongoDB and SQLite files. It offers the structure tree, a tabbed query editor with
history, a result grid, EXPLAIN, cell editing, copy and export, and an AI
assistant. Drivers are built into the app; no external command-line clients
are needed. The section is available on macOS, Windows, Linux and Android;
the iOS build and testing are deferred.

### Connections

Open Databases in the sidebar and choose “New database connection”. Enter a
name, engine, host, port, database (an index for Redis), username and
password. Save the password in the vault or type it on every open; a typed
password is used once and never stored. TLS defaults to verifying the
certificate and hostname; a local address or port forward switches to
“Prefer encryption” automatically. Through any route, TLS verifies the
original database hostname.

“Prefer encryption” uses TLS when the server offers it and goes without TLS
only through a tunnel (SSH host, jump profile, SSH tunnel, Kubernetes or Docker
forward) or on this computer (a loopback address after name resolution).
Direct or proxied connections to another host behave as “Require”: a server
without TLS fails with “The server does not support TLS” and the password is
never sent; older saved connections of this kind need “Disable TLS” chosen
explicitly. “Require encryption” encrypts without verifying the certificate;
PostgreSQL then uses the password only through SCRAM-SHA-256-PLUS (channel
binding), so an interceptor with a forged certificate cannot obtain it. Servers
without SCRAM-PLUS (older versions, `md5`/`password` in `pg_hba.conf`) get an
explanation and the “Allow sign-in without TLS binding (less secure)” switch,
off by default. MySQL/MariaDB have no such binding: only certificate
verification protects their password from an active interceptor. Without TLS
over the network PostgreSQL refuses cleartext password requests (allowed
through a tunnel or on this computer); MySQL/MariaDB never enable
`mysql_clear_password`; ClickHouse, Redis and MongoDB never fall back to
plaintext unless TLS is disabled. The workspace header and the connection card
show the actual protection: “TLS (verified)”, “TLS (certificate not
verified)”, “No TLS — protected by a tunnel” or “No TLS”. “Test connection” shows the server version before
saving.

Groups, tags and a colour label are set under “Group”; the colour appears on
the card and in the workspace header. The list switches between cards and a
compact list with the same toggle as Hosts, remembered on this device. Search
matches names, hosts, databases, engines and tags, and tags act as filters.

Import a connection string (`postgres://`, `mysql://`, `clickhouse://`,
`redis://`, `mongodb://`, `mongodb+srv://`) or a `.pgpass` file and review
each entry — production flag, route, whether to keep the password — before
saving. Connections and passwords live in the encrypted vault and sync
end-to-end encrypted. Queries go from the device to the database; the sync
server never sees them.

### Routes

- **Direct** to the database host and port.
- **SSH host** or **SSH jump profile** from your saved inventory, including
  jump chains.
- **SSH tunnel**: a saved local-forward tunnel, started on connect when it is
  not running — always on 127.0.0.1, even when the tunnel names a network
  bind address, so the database is never exposed to other computers. The
  form offers a matching tunnel when you type localhost and its port.
- **Kubernetes**: a port forward to a Pod, Service, Deployment or
  StatefulSet. Services and Deployments use a ready Pod, so the connection
  survives Pod restarts.
- **Docker container** on this computer or a saved SSH host, found by name so
  a re-created container still works.

Kubernetes and Docker forwards are restored on every connect, and running
ones are reused. Kubernetes Pods and Docker containers with a standard
database port have a “Connect to database” button that opens a saved
connection with the same route or a prefilled form.

### SQLite

SQLite is a file, not a server. Choose the “SQLite” engine, then where the
file is:

- **This computer** (macOS, Windows, Linux): type the path or use “Choose
  file…”. A new empty file is created only with “Create new database…”;
  connecting to a missing file reports it instead of creating one. The header
  is checked before opening, so another file (or an encrypted database) is
  refused. WAL mode is supported.
- **SSH host**, **Docker container** or **Kubernetes Pod**: the `sqlite3`
  program (3.16 or newer) there runs the queries; the file is not copied.
  Queries travel on standard input, not on the command line. A missing
  `sqlite3` is reported as such. A production Kubernetes context must allow
  running commands.

“Read only” is on by default: the file opens read-only and no query can
change it. Without it the usual rules apply: changes are reviewed on
non-production connections and need a temporary write window on production
ones. Every read still runs under `PRAGMA query_only`; `ATTACH`/`DETACH`,
`VACUUM INTO`, `load_extension`, `readfile`/`writefile` and changing pragmas
(encryption keys, `query_only`, `writable_schema`) are refused, and the remote
program runs with `-readonly` and `-safe`.

Structure: the file → Tables, Views, Indexes, Sequences (AUTOINCREMENT
counters), Table triggers and Data types; a table → Columns (“name (TYPE)”),
Keys, Foreign keys, Indexes, References (foreign keys of other tables that
point here) and Triggers. DDL, object details (file size, row estimate from
`sqlite_stat1`, STRICT, WITHOUT ROWID), an `EXPLAIN QUERY PLAN` tree,
completion, export (including INSERT with SQLite literals) and cell editing by
primary key or `rowid` work as for the other engines.

SQLite connections are kept **on this device only** (“This device only”
badge): they are not synced, stored in the vault or its backups, or shared,
because app versions up to 0.9.1 cannot read them. Their query history stays
local too.

### Production

Production connections open read-only in two layers: the server (read-only
transactions in PostgreSQL and MySQL/MariaDB, `readonly=2` in ClickHouse, role
checks in MongoDB) and the native query guard, which blocks changing
statements and attempts to lift the protection.

When production must be changed, **Unlock production** in the workspace
header opens a temporary write window: enter the vault passphrase (checked by
the app core) and pick 5, 15 or 30 minutes. While it
is open the header shows a red “Production writes until HH:MM” badge with a
countdown and a **Lock** button. Every changing statement is still shown for
review and needs the vault passphrase; result cells can be edited. The
window belongs to that session only and closes when the time runs out, on
Lock, on disconnect, vault lock or profile change. Server-side protection is
lifted only for the permitted changes — reads still run in read-only
transactions — and a read-only role still refuses: the app checks privileges
after unlocking and warns when the role cannot write. Changes made in the
window are marked in query history. Phones never get a write window.
“Check privileges” reports
whether server-side protection is verified and whether the role can write.
Prefer read-only roles: `SELECT` or `pg_read_all_data` in PostgreSQL, `SELECT`
in MySQL, a `readonly=1` profile in ClickHouse, ACL `+@read -@write` in Redis
and the `read` role in MongoDB.

Non-production connections also open read-only; the lock button allows writes
until the connection closes. Changing statements are shown in full for review
and run only after the vault passphrase is entered. This covers dangerous ones
(`DROP`, `TRUNCATE`, `UPDATE` or `DELETE` without `WHERE`, `FLUSHALL`,
dropping a collection) and cell edits, whose review shows the new values.
Redis `KEYS` never runs; the equivalent `SCAN` is offered for the editor.

### Sharing

When the sync server supports database connection sharing (server 0.1.15 or
later with the feature enabled), the connection menu offers **Share**. The
preview lists exactly what a colleague receives: engine, address, port,
database, user, TLS, limits and the production marker. Groups, query history,
AI settings and session permissions are never shared. The password and client
key are included only when explicitly selected and only if the server also
supports shared secrets.

A route through an SSH host, jump profile, proxy or tunnel is not shared: the
recipient selects their own. A Kubernetes forward keeps its namespace and
workload while the recipient selects their own context; a Docker forward keeps
the container name while the recipient selects the engine. Such a connection
cannot be saved as direct.

Received connections are listed with a **Shared** badge and your published
ones with **Shared by you**. **Details** opens acceptance, saving a personal
copy, members, revoking access and stopping sharing. A copy of a production
connection stays read-only production. Revoking access does not invalidate an
already copied password; change it on the database server.

### Workspace

The structure tree loads lazily and sorts objects into folders (tables,
views, functions, sequences, dictionaries; columns, indexes, constraints,
triggers, projections). The expanded table is framed and keys carry icons.
Object menus offer the first 100 rows, DDL (a numbered, highlighted code
block per statement, with Copy and Open in editor; the dialog follows the
window width and is only as tall as the code; long lines wrap with an
indent by default, and "Wrap lines" switches to sideways scrolling,
remembered on this device), the quoted identifier and
details; MongoDB collections can infer a schema from a sample. Redis keys are
grouped by `:` and read with `SCAN`.

Object details are read-only catalogue facts, looked up with bound names:
for PostgreSQL tables and views the schema, owner, kind, estimated rows,
sizes (total, data, indexes, TOAST), column/index/constraint/trigger counts,
tablespace, comment, partition key and the last VACUUM/ANALYZE; for
MySQL/MariaDB the engine, row format, estimated rows, data and index sizes,
AUTO_INCREMENT, collation and create/update times; for ClickHouse the engine,
rows, size, parts, partition/sorting/sampling keys and TTL; for a Redis key
its type, TTL, encoding, length and memory, and for a Redis database its key
count. Estimates are marked "≈", sizes are shown in KiB/MiB and Copy copies
the whole dialog. MongoDB descriptions are shown as JSON. The item is hidden
for objects without details.

Double-clicking a table, view, collection or Redis key, or choosing the first
100 rows, shows the data below right away in a single preview tab ("items —
data"): the next object reuses it instead of adding tabs. Editing the preview
tab's text turns it into an ordinary tab that is never overwritten. Generated
text — DDL from Open in editor, a history entry, an AI draft, the suggested
`SCAN` — is added to the current tab below the existing text after a blank
line, with the caret at its start, and never runs. While the preview tab is
shown it goes to the last editor tab; a new tab is created only when there is
none.

The editor numbers lines, highlights syntax, completes keywords, tables and
columns at the caret (including aliases), formats, toggles comments and finds
and replaces. MongoDB accepts familiar shell syntax such as
`db.orders.find({ status: "paid" })`. Shortcuts use ⌘ on macOS and Ctrl on
Windows and Linux: Enter runs the selection or the statement on the caret's
line (statements end at `;` and at blank lines; any line of a multi-line
statement runs all of it; on a blank line the nearest statement above runs,
else the one below), `⌥X` /
`Alt+X` (or Shift+Enter with the modifier) runs the whole script, Space
completes (`⌃Space` on macOS), Shift+F formats, `/` comments, F finds, `.`
cancels, E explains, H opens history and I opens the AI assistant. Scripts
stop at the first error. `{{variables}}` are bound as parameters.

EXPLAIN draws the plan as a tree with rows, cost and each step’s own share;
JSON shows the raw plan. “Execute with ANALYZE” adds timings but runs the
query, so it is limited to non-production connections and asks first.

### Results, editing, history and export

Results are a compact grid with row numbers, sorting, filtering, pinned and
hidden columns and full-value view; record cards on phones. The first page
(1,000 rows by default, set under Limits) loads at once, the rest with “Load
more”. Sensitive-looking columns are masked and copied values clear from the
clipboard automatically. Copy a cell, row, selection or the loaded result as
CSV, TSV, JSON, NDJSON, Markdown or `INSERT`; file export re-runs read-only
queries and streams every row with progress and cancellation.

For CSV and TSV, “Escape formulas (for Excel)” (on by default) prefixes text
starting with `=`, `+`, `-`, `@`, TAB or CR with an apostrophe so a
spreadsheet does not run it as a formula; numbers from numeric columns are
left as they are. `INSERT` string literals double the quote (`''`); MySQL,
MariaDB and ClickHouse literals also double backslashes, and PostgreSQL text
containing a backslash is written as `E'…'`, so the file reads back the same
whatever the server's escape settings.

A click focuses a cell (⌘C / Ctrl+C copies its value); a click on the row
number selects the row, Shift+click extends the selection and ⌘/Ctrl+click
toggles a row, Esc clears it. The status line counts selected rows, and copy,
export and the AI assistant use the selection when there is one.

A switcher on the left of the result shows it as a **Table** (the grid),
**Text** (an aligned, selectable and copyable monospace table of up to 2,000
rows; NULL as `[NULL]`, masked values as dots) or a **Record** (one row as a
field → value form). The view is remembered per tab. Column headers carry a
type glyph (number, text, date/time, true/false, JSON, binary) and a key on
key columns; NULL shows as a dim `[NULL]`.

The toolbar under the result has:

- **Refresh** (F5, ⌘R / Ctrl+R) runs the tab's last read query again after a
  fresh review; its menu turns on auto-refresh (off, 5 s, 10 s, 30 s, 1 min,
  5 min). Auto-refresh only repeats read queries (production included) and
  skips a beat while changes are pending, a query runs or the window is hidden.
- **Save** (⌘S / Ctrl+S) applies the pending changes; its menu has **Show
  SQL**. **Discard changes** (⌥⌘Z / Ctrl+Alt+Z) drops them.
- Rows: **Edit the current row** (⇧↵, opens the Record view), **Add a row**
  (⌥⌘N / Ctrl+Alt+N), **Duplicate the row** (⌘D / Ctrl+D) and **Delete the
  row** (⌘⌫ / Ctrl+Delete; every selected row when there is a selection).
- Navigation: first (⌘↑ / Ctrl+Home), previous (⌥↑ / Alt+↑), next (⌥↓ /
  Alt+↓) and last (⌘↓ / Ctrl+End) row; ↑/↓ also move the current row in the grid.
- **Export data…** (⇧⌘E / Ctrl+Shift+E) and **Import data…** (CSV into the
  result's table, see below).
- The gear and the **Rows per fetch** field set the next page size (1–1,000,
  remembered per connection); **Load the next rows** and **Fetch all rows**
  (within the result memory limit).

The status line shows the rows fetched, the elapsed time, the date and time
the query finished, “more available” when rows are left on the server, and
the selected rows and pending changes.

On desktop, sessions with writes allowed (non-production, or production in
its write window) can change, add and delete rows of a simple single-table
`SELECT`. Rows are identified by the primary key or, without one, by the
first unique key of NOT NULL columns (physical row addresses such as `ctid`
are not stable and never used). Without such a key, for ClickHouse, Redis and
MongoDB, and on phones the editing actions are off and their tooltip says why.

- Double-click a cell (or Enter/F2) to edit it in place: Enter keeps the
  value, Esc cancels, Tab moves on. The value is checked and bound with the
  column's type; NULL is an explicit toggle. Changed cells are yellow.
- A new row is green with “+” as its number. Columns the server fills
  (default, identity, auto-increment) read `[DEFAULT]`, other empty ones
  `[NULL]`. Required columns (NOT NULL without a default) must be filled in —
  Save highlights them and sends nothing. Generated columns are never written.
- **Duplicate** copies a row without key, generated and masked values.
- A row to delete is struck through in red until saved; Delete again unmarks
  it, and a new row is simply removed.

Nothing runs before Save. The core turns the changes into parameterized
`DELETE`, `UPDATE` and `INSERT` statements (in that order) by key; the review
shows them with the values as literals and asks for the vault passphrase, and
the production guard still applies. They run in one transaction in which each
statement must change exactly one row, otherwise everything rolls back; then
the result refreshes. **Show SQL** opens the statements as code to copy or add
to the current editor tab. Running again, refreshing, switching result sets,
closing the tab or leaving the connection with pending changes asks whether to
save, discard or stay.

**CSV import.** On desktop, PostgreSQL, MySQL and MariaDB tables can be
loaded from CSV: **Import data…** on the result toolbar (into the result's
table when it is a single keyed table) or **Import from CSV…** on a table in
the tree. Writes must be allowed (production: an open write window). The
wizard has five steps: the **file** (CSV, TSV or text up to 200 MB, read as a
stream and sent nowhere but this database); **reading settings** with a live
preview of the first 100 rows — encoding (UTF-8 by default with BOM
detection, Windows-1251, Latin-1), delimiter (detected: `,` `;` TAB `|`),
quote character, header row, lines to skip, what NULL is (empty field,
`NULL`, `\N`), decimal separator and date order; **columns** — each file
column goes to a table column (matched by name, ignoring case) or is
skipped, and the first 1,000 rows are checked against the column types with
sample bad values and a warning for required columns without values;
**options** — insert, insert skipping existing keys, or insert-or-update by
key (PostgreSQL `ON CONFLICT`, MySQL `ON DUPLICATE KEY UPDATE`), batch size,
stop on a bad row or skip it into `<file>.errors.csv`, one transaction for
the whole file or one per batch, and “Delete all rows of the table first”
(off by default; a `DELETE FROM` in the same transaction, shown in the
review); and the **review and run** — the usual review with the vault
passphrase and the production guard, then parameterized multi-row `INSERT`
batches with progress (rows per second, time left) and Cancel, which rolls
the open transaction back, ending with imported / skipped / failed counts
and the report. Formula-looking values are imported as they are (escaping
them is an export concern). Other queries of the connection wait while an
import runs. ClickHouse, MongoDB and Redis do not import CSV yet.
**Create table from CSV…** on a schema (PostgreSQL) or database (MySQL)
builds a simple `CREATE TABLE` typed from the file's first rows (whole
numbers, decimals, dates, timestamps, true/false, else text) to copy or open
in the editor and run through the usual review.

History is kept per connection — text, time, duration, rows and error, never
parameter values or data — with filters for successful, failed and favourite
queries. It is local and encrypted by default, can join encrypted sync or be
turned off, and any entry can become a snippet.

### Database snippets

Snippets → Starter packages offers diagnostic query packages: “SQL ·
PostgreSQL” (20), “SQL · MySQL/MariaDB” (20), “SQL · ClickHouse” (20),
“Redis” (15) and “MongoDB” (15). They are added to the vault only when you
press the button and are ordinary editable snippets afterwards. They cover
active and long-running queries, the lock tree, table and database sizes,
index usage, dead tuples and autovacuum, replication and slots, non-default
settings, top queries (`pg_stat_statements`, `performance_schema` digests,
`system.query_log`), ClickHouse parts, merges and mutations, Redis memory and
slow log, and MongoDB `currentOp`, stats and profiler. Almost all of them only
read; `EXPLAIN ANALYZE` (it executes the query), cancelling a query and `KILL`
are marked modifying and terminating a PostgreSQL session destructive, so they
need confirmation. Each description names the client (psql, mysql,
clickhouse-client, redis-cli, mongosh) and any version or extension
requirement.

Snippet type and dialect match what the history saves: PostgreSQL →
`postgresql`, MySQL/MariaDB and ClickHouse → `sql` with the `mysql` /
`clickhouse` dialect, Redis → `redis_cli`, MongoDB → `shell` with `mongosh`.
The same snippets work in an SSH terminal: start psql, mysql,
clickhouse-client, redis-cli or mongosh in the session and pick a snippet —
after the variables and the risk confirmation it is typed into the active tab
as text (never through `sh -c`), and a multi-line query is sent as one
bracketed paste when the client enabled it. The app never opens a new SSH
session for such a snippet. Server admin commands (for example `CLIENT LIST`,
`SLOWLOG`, `db.currentOp()`, `rs.status()`) are not allowed in the Databases
workspace and are tagged terminal-only.

### AI assistant

The assistant uses the providers from Settings → AI to compose, explain, fix,
optimize, translate and parameterize queries, suggest examples and explain
selected rows. Schema metadata, selected rows, row samples (up to 20) and one
read-only probe (up to 50 rows) are shared only with per-request consent;
everything starts off. Passwords, connection strings and server addresses are
never sent. Strict privacy pseudonymizes identifiers for external providers.
Drafts are added to the current editor tab and never run automatically; production DDL
drafts are shown only.
