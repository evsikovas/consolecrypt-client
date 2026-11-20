# ADR: нативный канал целей интерактивного Runner

10 ноября 2026. Реализован и проверен на изолированном стенде; полная
приёмка E0/E1 и сертификация платформ остаются незавершёнными.

## Решение

Приватные SSH-ключи остаются в Rust-ядре. Helper получает подписанный
JobManifest, снимок проекта и Unix socket конкретного задания. Ansible
connection plugin передаёт ограниченные exec/SFTP операции через этот socket.
Это не SSH agent: запросов на подпись socket не принимает. Stdout проекта
никогда не разбирается как control IPC.

Нативный JobTargetScope независимо проверяет Job ID, digest подписанного
manifest, Host ID, монотонный TTL, отмену, параллелизм и лимиты. Он содержит
только заранее аутентифицированные SSH-сессии, без общего credential resolver.
После подтверждения нельзя выбрать другое назначение или credential.

Для WSL TCP к первой SSH-точке открывает отдельный процесс явно выбранных
distribution/user. Private stdin/stdout переносит SSH ciphertext. Проверка
host key, authentication, jump-chain и SFTP остаются в Rust; ключи и Windows
agent socket не экспортируются в WSL. Для remote Runner целевой маршрут
начинается через проверенную SSH-цепочку Runner. Origin и immutable plan
входят в route digest. Неподдержанный proxy/backend отклоняется; скрытого
fallback на Windows-origin нет. Не нужны Windows listener, `0.0.0.0`,
изменение Firewall или остановка WSL.

Manifest связывает revision, environment, playbook, параметры, inventory,
точные цели, policy, mode, nonce и два срока: приём подтверждения до 10 минут
и target-доступ не дольше timeout/grant. Изменение требует новой проверки
и подписи. Модель не имеет API произвольного подписания. Повтор принятого
manifest не создаёт второе исполнение.

Validation и run используют non-root bubblewrap с отдельными filesystem,
network и PID namespaces. Validation не получает target socket. Run получает
только socket своего job; разрешения из manifest не расширяются. Socket inode
0600 находится в каталоге 0700, проверяется SO_PEERCRED. Linux bind через
собственный `/proc/self/fd/<dirfd>/broker.sock` обходит sun_path limit без
открытия каталога другим пользователям.

После lock новые credentials запрещены; ранее выданный scope отменяется или
доживает до подписанного TTL по policy. Разрыв/отмена не означают откат:
неизвестный результат остаётся unknown без автоматического повтора.

## Изменение WSL-06

Черновик предлагал agent внутри WSL. Проверенный нативный broker заменяет его
без экспорта извлекаемых ключей в отдельную ОС. Это изменение транспорта,
без общего agent/resolver и без ослабления host key checks. Раздел 19 ТЗ
требует ADR, тест и явное обновление; редакция 0.2 фиксирует эту замену.
Для постоянного Runner E3 отдельно нужны identity, отзыв и ограниченное
делегирование: интерактивный канал не является постоянной службой.

## Фактические доказательства

Windows x64: native tests RPC, подписи, snapshot, редактора и target contract.
Raw WSL-origin transport: 1 114 112 байт, все значения 0–255, backpressure/EOF.
Два временных Ubuntu 24.04 x64 контейнера: Runner и отдельная цель с разными
ключами. Цель remote-сценария без опубликованного порта. Настоящий Ansible
copy/slurp/assert пишет и проверяет маркер на цели; на Runner маркера нет.
Проверены duplicate Job ID, event cursor и отказ чужому Host ID.

Такое же настоящее задание прошло с helper в WSL2 Ubuntu-26.04/evsikov и
WSL-origin TCP. Доступ этого дистрибутива к тестовому Windows loopback
подтверждён соединением; это не предположение о любой WSL-сети.
Артефакты общего Tasks: runner-execution-remote-tests.log,
runner-execution-wsl-tests.log, runner-network-origin-tests.log,
automation-native-rpc-tests.log. Драйверы test_runner_execution_remote.py и
test_runner_network_origin.py чистят только свои контейнеры/UUID-state.
Пользовательские контейнеры, службы и дистрибутив не останавливаются.

## Открытые условия поставки

После отдельного разрешения пользователя выполнен также настоящий прогон
WSL2 Ubuntu 24.04, UID10001, Python3.12.3: 48 tests и signed Ansible job с
проверкой копирования/чтения файла на отдельной цели. Стенд импортирован из
того же проверенного Ubuntu24 image в отдельный workspace VHD; после теста
регистрация удалена. Default-дистрибутив проверен до и после и не изменился.
Журнал: `Tasks/runner-execution-wsl2404-tests.log`; hashes и состояние cleanup
записаны в `Tasks/wsl2404-fixture-plan.json`. Это импортированный тестовый
rootfs, а не заявление о всех вариантах пользовательской установки Ubuntu.

Прогоны не сертифицируют native macOS или Linux arm64.
Per-process limits пока не равны aggregate cgroup budget на произвольном
нативном хосте. Нужны дополнительные проверки отмены, TTL, shared-route
отзыва, profile/lock и пересечения целей между разными helper. До них экран
подтверждения запуска не считается готовым. Версии и hashes закреплены в
runner/requirements-*.lock и environment manifests; installer не выпущен.
