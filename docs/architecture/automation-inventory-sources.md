# Внешний static inventory: нормализация и привязки

Рабочая часть E1, 10 ноября 2026. INI/YAML parser и нормализация находятся в
`runner/src/consolecrypt_runner/inventory.py`. Native preview, выбор файлов и
каталогов, явные привязки и исполнение подключены. INV-01 проверен в текущем
Windows/Linux/WSL backend scope; INV-02/03 и общая платформенная приёмка
остаются незавершёнными.

## Реализованная граница parser

Статические INI и YAML читаются как данные. Inventory scripts, сторонние
plugins, YAML constructors, anchors/aliases и пользовательский код не
запускаются. Переменные credentials, произвольные SSH options и служебные
`consolecrypt_*` запрещены. Это не поиск всех возможных секретов в task vars:
пользователь всё равно должен хранить чувствительные значения отдельно.

Адрес, порт и пользователь нормализуются в каждой записи хоста. Явные значения
хоста перекрывают группы, дочерние группы перекрывают родителей, `all`
учитывается и как неявный родитель. Эти правила соответствуют описанному
[порядку наследования Ansible](https://docs.ansible.com/projects/ansible/latest/inventory_guide/intro_inventory.html).

Для конфликтующих connection values соседних групп ConsoleCrypt требует
явного значения хоста. Алфавитный выбор и `ansible_group_priority` не являются
разрешением незаметно изменить маршрут: priority пока не поддерживается.
Обычные group vars сохраняются, connection vars удаляются из сериализованных
групп после разрешения в видимый host snapshot.

Внутренние declared-field markers не сериализуются. Они нужны, чтобы endpoint,
унаследованный при чтении одного файла, не стал явным override при последующем
объединении с другими источниками. Сырые defaults остаются внутри parser до
merge. Несовместимые явные endpoints, credentials bindings или переменные
одного alias блокируют объединение.

Граф групп проверяется топологически: циклы, неопределённые children и пути
глубже 32 отклоняются, в том числе когда общий child ранее уже был посещён.
Имена alias/group не должны совпадать; `_meta` зарезервировано. Лимиты parser:
5 МиБ текста, 250 000 YAML tokens, 10 000 hosts, 1 000 groups и 5 миллионов
операций разрешения наследования. Транспортные frames имеют отдельный лимит.
Целые переменные ограничены signed 64-bit для одинакового представления в
Python/Rust/Dart; более длинные идентификаторы следует задавать строками.

## Реализованный предпросмотр

Из проверенного snapshot выбирается от одного до восьми файлов/каталогов.
Native разворачивает каталог по metadata этого снимка, а не новым обходом
диска. Максимум 128 файлов общим размером 5 МиБ. Читаются статические INI,
YAML/JSON и INI без расширения; hidden/backup и обычные README пропускаются.
Executable-файлы не становятся inventory scripts.
Helper запускает parser в отдельном filesystem/PID/network namespace без
target socket и credentials. В helper передаётся точный список regular files.
Каждый путь проверяется без следования symlinks, SHA-256 исходных bytes
сверяется с native revision; UTF-8 BOM учитывается в исходном hash.

Нормализованный документ до 5 МиБ передаётся частями до 128 КиБ. Native
повторно проверяет offsets, полную длину/hash, схему и provenance; source не
может назначить Host ID или credential reference. Helper cache ограничен
32 МиБ в целом, 12 МиБ и восемью capture на revision, с резервом диска 128 МиБ.

AppCore возвращает preview ID до выполнения работы. Доступны максимум четыре
удерживаемых preview с TTL 10 минут, отмена и страницы до 50 nonsensitive
записей. Variables и credentials в страницы не попадают. Блокировка и смена
профиля закрывают прежние ID; preview не создаёт и не подключает target hosts.
Проверено настоящим AppCore→WSL сценарием, включая отмену/лимиты и lock fencing.
Native привязки к сохранённым Host ID реализованы: отдельный SQLCipher документ
с hash marker в каталоге и атомарным увеличением revision. Commit потребляет
один ready preview, заново открывает текущую папку проекта и проверяет snapshot.
Привязка запоминает source endpoint и решение match/use_saved_host; изменение
адреса/порта/пользователя источника требует нового явного выбора. Сама привязка
не меняет сохранённый хост, его маршрут или credentials и не добавляет цель.

Интеграция native prepare/accept подключена через inventory_ref. Полный
normalized snapshot передаётся chunk-ами и независимо сверяется по semantic
values и exact canonical hash; подписанный manifest по-прежнему содержит
inventory_digest. Приватный witness может быть до 5 МиБ, внутренние файлы
validation/execution — до 6 МиБ; общий IPC frame cap остаётся 1 МиБ.
Публичные review/history не содержат source variables. Большой artifact
проверен actual Windows→WSL transfer. Полный external-inventory Ansible job с
private value 1,2 МБ также выполнен на отдельных Ubuntu24 Runner/target:
подтверждены signed admission, новый retry ID, отсутствие variables в review,
независимый marker на target и отсутствие marker на Runner. Пользовательский
UI также проверен; это не означает закрытие INV-02/03 или всей матрицы.

## Каталоги переменных

В выбранных каталогах `group_vars` и `host_vars` читаются как статические
YAML/JSON mappings, включая файлы без расширения и каталоги отдельных целей.
Файлы одной цели применяются в лексическом порядке, host vars перекрывают
групповые defaults. Connection values разрешаются после чтения всех vars:
явный host vars может устранить неоднозначность соседних групп. Эти сценарии
сравнены с установленным Ansible на константном owned fixture в пустом HOME.
Поведение описывает публичный
[host_group_vars](https://docs.ansible.com/projects/ansible/latest/collections/ansible/builtin/host_group_vars_vars.html).

Неопределённый host/group в vars-файле вызывает ошибку, а не создаёт новую
цель и не теряется молча. Credentials, connection plugins и служебные
`consolecrypt_*` по-прежнему запрещены. Все vars-файлы входят в source hashes;
проверяется исходное содержимое, включая BOM. Ansible Vault/password handling
подключается отдельно в ANS-05 и не имитируется этим parser.

## Пользовательский сценарий

Диалог показывает относительные пути, выбранный Runner и фактически захваченные
файлы. Search и pagination закреплены, выводится до 50 записей на страницу;
поиск saved hosts и список записей строятся лениво. В интерфейс не передаются
полные variables. Переключение источников сбрасывает только несохранённый draft.

Перед сохранением native check показывает до 50 конфликтов: пропавшая запись,
изменённый source endpoint, отсутствующий saved host или несовпадение маршрута.
Planner читает только metadata/references, без CredentialResolver и соединений.
Пользователь перепривязывает запись, выбирает use_saved_host или убирает binding.
Check не потребляет preview и не выдаёт полномочий; commit повторяет обязательные
проверки самостоятельно. Clear требует подтверждения и не удаляет файлы/хосты.

В external-only режиме run dialog предлагает только явно связанные Host ID.
Mixed добавляет остальные saved hosts без автоматического выбора. Native gate
повторно строит exact targets и связывает inventory с manifest. Изменение host
folder hierarchy не требуется для project-only Ansible groups.

Тесты сравнивают эффективные connection values с установленным Ansible на
собственном константном inventory в пустом HOME/config. Это не разрешение
запускать upstream parser на произвольном проекте вне sandbox.

## Граница native gate

1. Выбранные файлы/каталог читаются из того же проверенного project snapshot,
   который затем используется для validation и исполнения. Preview не получает
   credentials и не выполняет inventory plugins.
2. UI показывает источники, aliases и итоговые nonsensitive endpoints. Каждый
   внешний alias явно связывается с сохранённым Host ID; совпадение имени или
   адреса само по себе не создаёт доверие. Несовпадения маршрута/пользователя
   требуют явного решения, а не подстановки параметров.
3. Native строит фактический target scope из выбранных Host ID и проверенных
   SSH sessions, включая host keys и маршруты со стороны Runner. Ни источник,
   ни модель не выбирает credentials или новый сетевой доступ.
4. Объединённый inventory, revision, параметры и exact targets связываются
   обычным validation proof и подписанным manifest. Невыбранные цели удаляются
   из runtime inventory и не получают credential channel.

Эта последовательность проверена actual AppCore→WSL и на отдельных Ubuntu24
Runner/target, включая directory inventory и host vars payload 1,2 МБ. UI32,
Windows core46, Linux core47 и AppCore15 проверки прошли; журнал в
`Tasks/AUTOMATION_MONITORING_STATUS.md` хранит имена точных прогонов и ограничения.
Standard JSON helper не означает уже поставленный CLI/collection plugin;
INV-09 остаётся отдельной задачей.
