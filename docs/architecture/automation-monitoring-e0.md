# Автоматизация и мониторинг: границы E0

Статус: рабочие решения для прототипа, 10 ноября 2026. Основание:
`Tasks/CONSOLECRYPT_AUTOMATION_MONITORING_TZ.md`, редакция 0.1. Этот документ
не объявляет готовыми Ansible, мониторинг, Agent, Relay или постоянный Runner.

## Навигация

Реестр разделов общий для sidebar, «Все разделы», настройки и палитры. ID
соответствует устойчивому имени раздела, а не переводу или индексу маршрута.
Имена уже сохранённых ID нельзя менять при переименовании UI. Фильтрация
доступности профиля и compile-time capabilities выполняется независимо от
закрепления. Скрытие не вызывает перехода или удаления Navigator/session.

`navigation_by_profile` в существующих device-local `ui-state.json` содержит
profile ID и `{version: 1, visibility: {sectionId: bool}}`. Эти данные не входят
в синхронизируемые настройки vault. Отсутствующая запись сохраняет исходный
набор продукта; после первой правки сохраняется явный набор. Неизвестные ID
сохраняются, но не создают маршрут; добавленные после настройки разделы
сами не закрепляются. Неподдержанная будущая схема читается без редактирования.
Настройка при смене профиля/блокировке закрывается; запись сверяет profile ID.

«Автоматизация» регистрируется с defaultPinned=false и открывает редактор
проектов. Метаданные локальны в SQLCipher профиля, обычные файлы остаются на
устройстве. Мониторинг до готового экрана не регистрируется. Отсутствие ID в
существующем явном наборе означает false; пустые экраны не добавляются.

## Разделение компонентов

Клиент готовит immutable revision и показывает план. Runner исполняет только
подтверждённый manifest; модель не подписывает и не расширяет разрешения.
Agent мониторинга по умолчанию не имеет jobs.execute. Sync server продолжает
передавать шифротекст и не становится SSH gateway или держателем vault key.

Новые зашифрованные типы нельзя включить в общую синхронизацию до проверки
сохранения неизвестных объектов старым клиентом, backup/export/sharing и
feature negotiation. Для E0 настройки навигации целиком локальны: изменение
серверного API и обновление sync server не нужны.

## Контракт helper v1 для следующего прототипа

Транспорт: отдельный процесс и отдельный IPC, никогда ввод в пользовательскую
терминальную сессию. Native Unix, явный WSL2 distribution/user и remote SSH
доставляют один и тот же helper. Windows host не считается Ansible control node.
Рабочий каталог Unix 0700, временные credential-файлы 0600. В WSL снимок живёт
в Linux filesystem, не исполняется непосредственно из Windows-mounted дерева.

Control messages: version, type, job_id, revision_digest, request nonce;
events: version, job_id, monotonic sequence, UTC timestamp, type и bounded payload.
Один frame не более 1 МиБ; крупный snapshot передаётся отдельно с проверкой
размера, количества файлов и SHA-256. Секреты не попадают в argv, URL, обычный
environment или diagnostic stdout. Проверенный job-specific credential channel
описан в [отдельном ADR](automation-target-transport.md); общий credential
resolver клиент не выдаёт.

`probe` сообщает окружение и capabilities без выполнения проекта и без выдачи
целевых credentials. Missing dependency — отдельный результат, не установка.
`validate` исполняет недоверенные plugins/lookups в изоляции без credentials.
`run` требует exact revision/inventory/targets/policy digest и свежую подпись.
Обычный subprocess не является sandbox; strict-required manifest должен быть
отклонён backend без доказанных filesystem/network controls.

Job ID + тот же digest возвращает известный job; другой digest конфликтует.
Разрыв соединения не создаёт повторный run. Отмена адресует process group/job,
через 15 секунд завершает только его процессы; `wsl --shutdown` запрещён.
Unknown outcome остаётся unknown до reconciliation, а не становится cancelled.
Пересекающиеся изменяющие targets сериализуются. Дополнительная coordinator
lease/fencing для пула относится к постоянным службам E3.

## Следующие обязательные доказательства

1. Версии ansible-core 2.20.11, ansible-runner 2.4.3, ansible-lint 26.11.0 и
   полные wheel hash locks закреплены для проверенных Python 3.12/3.14 x64
   окружений. Остальные платформы и installer требуют отдельной проверки.
2. Настоящее WSL2 non-root исполнение и перенос снимка с пробелами/кириллицей
   проверены; матрица Ubuntu 24.04 WSL, отмены, TTL и resource budget остаётся.
3. Изолированный SSH стенд подтвердил разные ключи jump/target, повтор ID,
   snapshot и actual SFTP. Потеря связи и lifecycle требуют дополнительных
   проверок. Пользовательские серверы испытательным стендом не становятся.
4. Snapshot traversal/symlink/hardlink limits, подписанный approval и поведение
   при lock/profile switch. До этого кнопка выполнения не появится.
5. Зафиксировать эталонное устройство и измеренный p95/CPU/RAM. Числа из ТЗ
   не считаются результатами измерения и не превращаются в обещание производительности.

Официальные опорные документы, просмотрены 10 ноября 2026:
[Ansible Runner](https://docs.ansible.com/projects/runner/en/stable/),
[матрица поддержки Ansible](https://docs.ansible.com/projects/ansible/latest/reference_appendices/release_and_maintenance.html).
