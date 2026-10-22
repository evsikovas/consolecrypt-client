# Базы данных / Databases

## Русский

Раздел «Базы данных» работает с PostgreSQL, MySQL и MariaDB, ClickHouse, Redis
и MongoDB. В нём структура базы, редактор запросов с вкладками и историей,
таблица результатов, EXPLAIN, правка ячеек, конструктор таблиц, копирование и
экспорт, а также ИИ-помощник. Драйверы встроены в приложение, поэтому `psql`, `mysql`,
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

### Редактирование ячеек

На компьютере в сессии с разрешённой записью (непрод-подключение или прод с
открытым окном записи) можно менять ячейки результата простого `SELECT` из
одной таблицы с первичным ключом. Двойной щелчок по ячейке открывает
редактирование прямо в таблице: Enter сохраняет значение в списке изменений,
Esc отменяет, Tab переходит к следующей ячейке. Тип значения берётся из
описания колонки в результате и проверяется при вводе; NULL задаётся
отдельным переключателем. Изменённые ячейки подсвечиваются. «Проверить
изменения» показывает сгенерированные `UPDATE … WHERE <первичный ключ>`
(значения передаются параметрами); после подтверждения они выполняются одной
транзакцией. Если какое-то изменение не нашло свою строку, откатываются все.
Без права записи двойной щелчок открывает просмотр значения.

### Конструктор таблиц

Таблицу можно создать или изменить без ручного SQL. «Создать таблицу…» есть в
меню схемы (PostgreSQL), базы данных (MySQL/MariaDB, ClickHouse) и папки
«Таблицы», а также кнопкой в заголовке рабочей области; для MongoDB пункт
называется «Создать коллекцию…» и доступен у базы данных подключения.
«Изменить структуру…» в меню таблицы загружает её текущую структуру. Для Redis
конструктора нет.

Конструктор открывается вкладкой рядом с вкладками редактора: имя, схема или
база данных, комментарий и разделы «Столбцы», «Ключи», «Внешние ключи»,
«Индексы», «Проверки», «Параметры» и «DDL». В таблице столбцов задаются имя,
тип (список типов движка с поиском или свой тип), длина и масштаб, NOT NULL,
значение по умолчанию (с готовыми вариантами вроде `now()`,
`CURRENT_TIMESTAMP`, `gen_random_uuid()`), автоинкремент (identity или
`serial` в PostgreSQL, `AUTO_INCREMENT` в MySQL), первичный ключ, уникальность
и комментарий; столбцы перетаскиваются, дублируются и удаляются. Обычный
текст в поле значения по умолчанию становится строкой (`active` →
`'active'`), выражения и числа остаются как есть. Внешние ключи выбирают
таблицу и столбцы из структуры базы и действия ON DELETE/ON UPDATE; у
индексов PostgreSQL есть методы btree/hash/gin/gist/brin/spgist. Для
ClickHouse вместо NOT NULL используется `Nullable(T)`, есть
`LowCardinality(T)`, движок (семейство MergeTree, Memory, Log), обязательный
ключ сортировки ORDER BY, PRIMARY KEY и PARTITION BY; внешних ключей,
уникальности и проверок там нет. Для MongoDB поля с BSON-типом и
обязательностью превращаются в валидатор `$jsonSchema`, доступны capped,
time series, уровень и действие проверки, collation и индексы.

Пока таблица проектируется, на сервере ничего не выполняется: изменения
копятся в черновике. Вкладка, новые и изменённые строки помечаются, новая
таблица и её столбцы сразу видны в дереве зелёным, изменённые — оранжевым
(с точкой-меткой); щелчок по ним возвращает к конструктору. Есть отмена
последнего шага и отмена всех изменений. Проверка идёт на лету: пустые и
повторяющиеся имена, неизвестные столбцы в ключах, типы без нужной длины,
ORDER BY для MergeTree, AUTO_INCREMENT без ключа и т. п.; идентификаторы
всегда берутся в кавычки, поэтому зарезервированные слова и любые символы
безопасны. Раздел «DDL» показывает итоговый скрипт; его можно скопировать или
дописать в текущую вкладку редактора.

«Сохранить изменения» (⌘S / Ctrl+S) открывает окно «‹таблица› — сохранить
изменения» с полным скриптом (подсветка, номера строк, перенос строк),
кнопками «Открыть в редакторе», «Копировать», «Отмена» и «Выполнить». Это и
есть подтверждение: окно предупреждает об удалении данных или перестройке
таблицы, просит парольную фразу хранилища, а выполнение проходит обычную
проверку запроса и защиту продакшена (для прод-подключения нужно открыть окно
записи). В PostgreSQL скрипт выполняется одной транзакцией: при ошибке не
меняется ничего. В MySQL/MariaDB, ClickHouse и MongoDB DDL не транзакционный —
окно предупреждает об этом, и при ошибке показывает, какие операторы уже
применены. После успеха дерево обновляется, а конструктор продолжает работать
с таблицей на сервере. При закрытии вкладки с несохранёнными изменениями
конструктор спрашивает «Сохранить / Не сохранять / Отмена».

При изменении существующей таблицы конструктор сравнивает черновик со
структурой на сервере и строит ALTER: добавление, удаление и переименование
столбцов, смена типа, NULL и значения по умолчанию, ключи, индексы и
проверки. Ограничения: параметры, задаваемые при создании (движок, ключ
сортировки ClickHouse, секционирование), здесь не меняются; в ClickHouse
нельзя переименовать таблицу и убрать значение по умолчанию; в PostgreSQL
нельзя снять identity; комментарий с апострофом пока не поддерживается;
TTL и SETTINGS ClickHouse, а также UNLOGGED в PostgreSQL добавляются
вручную в редакторе.

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
Redis and MongoDB. It offers the structure tree, a tabbed query editor with
history, a result grid, EXPLAIN, cell editing, a table designer, copy and
export, and an AI assistant. Drivers are built into the app; no external command-line clients
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

On desktop, sessions with writes allowed (non-production, or production in
its write window) can edit cells of a simple single-table `SELECT` with a
primary key: double-click a cell to edit it in place, Enter keeps the value,
Esc cancels and Tab moves to the next cell. The value is checked and bound
with the column's type from the result; NULL is an explicit toggle. Pending
edits are highlighted, reviewed as parameterized `UPDATE` statements and
applied in one transaction that rolls back unless every change hit exactly
one row. Without write access a double-click opens the value viewer.

History is kept per connection — text, time, duration, rows and error, never
parameter values or data — with filters for successful, failed and favourite
queries. It is local and encrypted by default, can join encrypted sync or be
turned off, and any entry can become a snippet.

### Table designer

Tables can be created and changed without writing SQL. «Create table…» is in
the menu of a schema (PostgreSQL), a database (MySQL/MariaDB, ClickHouse) and
the Tables folder, and in the workspace header; for MongoDB it is «Create
collection…» on the connection's database. «Edit structure…» on a table loads
its current structure. Redis has no designer.

The designer opens as a tab next to the editor tabs: name, schema or
database, comment, and the sections Columns, Keys, Foreign keys, Indexes,
Checks, Options and DDL. Columns have a name, a type (the engine's type list
with search, or any type typed in), length and scale, NOT NULL, a default
(with presets such as `now()`, `CURRENT_TIMESTAMP`, `gen_random_uuid()`),
auto increment (identity or `serial` in PostgreSQL, `AUTO_INCREMENT` in
MySQL), primary key, unique and comment; rows can be dragged, duplicated and
removed. Plain text in the default field becomes a string (`active` →
`'active'`); expressions and numbers stay as written. Foreign keys pick the
referenced table and columns from the database structure plus ON DELETE/ON
UPDATE; PostgreSQL indexes offer btree/hash/gin/gist/brin/spgist. ClickHouse
uses `Nullable(T)` instead of NOT NULL, offers `LowCardinality(T)`, the table
engine (MergeTree family, Memory, Log), the required ORDER BY sorting key,
PRIMARY KEY and PARTITION BY, and hides foreign keys, unique keys and checks.
For MongoDB, fields with a BSON type and «required» become a `$jsonSchema`
validator; capped and time-series collections, validation level/action,
collation and indexes are available.

Nothing runs on the server while you design: changes stay in a draft. The
tab, new and changed rows are marked; a new table and its columns appear in
the tree right away in green, changed ones in orange (with a dot marker), and
clicking them returns to the designer. Undo and «Revert all» are there.
Validation runs as you type: empty or duplicate names, unknown key columns,
types that need a length, ORDER BY for MergeTree, AUTO_INCREMENT without a
key and more. Identifiers are always quoted, so reserved words and any
characters are safe. The DDL section shows the resulting script; copy it or
append it to the current editor tab.

«Save changes» (⌘S / Ctrl+S) opens «‹table› — save changes» with the whole
script (highlighting, line numbers, wrap switch) and «Open in editor»,
«Copy», «Cancel» and «Run». This dialog is the confirmation: it warns when
data is dropped or the table is rewritten, asks for the vault passphrase, and
the run goes through the usual query review and production guard (a
production connection needs an open write window). PostgreSQL applies the
script in one transaction, so a failure changes nothing. DDL in
MySQL/MariaDB, ClickHouse and MongoDB is not transactional: the dialog says
so and, on failure, shows which statements were already applied. After a
successful run the tree refreshes and the designer keeps working on the
table on the server. Closing a tab with unsaved changes asks «Save / Don't
save / Cancel».

When editing an existing table the designer compares the draft with the
server structure and generates ALTER statements: added, dropped and renamed
columns, type, NULL and default changes, keys, indexes and checks. Limits:
options set at creation (engine, ClickHouse sorting key, partitioning) are
not changed here; ClickHouse cannot rename the table or remove a default;
PostgreSQL identity cannot be removed; comments with an apostrophe are not
supported yet; ClickHouse TTL and SETTINGS and PostgreSQL UNLOGGED are added
in the editor.

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
