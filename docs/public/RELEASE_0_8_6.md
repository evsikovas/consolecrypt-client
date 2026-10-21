# ConsoleCrypt 0.8.6

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

**Общий доступ к подключениям баз данных.** Сохранённым подключением теперь
можно поделиться с проверенным устройством коллеги — так же, как хостами и
контекстами Kubernetes. Нужен сервер синхронизации 0.1.15 или новее с
включённой функцией; на `sync.consolecrypt.dev` она включена. Со старым
сервером пункт «Поделиться» не показывается.

- «Поделиться» в меню подключения в разделе «Базы данных». Перед отправкой
  видно, какие поля получит коллега: движок, адрес, порт, база, пользователь,
  TLS, лимиты и пометка «прод».
- Группа, история запросов, настройки ИИ и разрешения сеанса не передаются.
  Пароль и ключ клиента уходят только по явной отметке и только если сервер
  поддерживает общие секреты.
- Маршрут через SSH-хост, jump-профиль, прокси или туннель не передаётся:
  получатель выбирает свой. У проброса Kubernetes сохраняются пространство
  имён и нагрузка, у Docker — имя контейнера. Такое подключение нельзя
  сохранить как прямое.
- Общие подключения видны в разделе «Базы данных» с меткой «Общее» (свои —
  «Вы поделились»). Оттуда можно принять подключение, сохранить личную копию,
  управлять участниками и отозвать доступ.
- Копия прод-подключения остаётся прод-подключением только для чтения.
- Подключения можно добавлять в общие коллекции.

Отзыв доступа не отменяет уже скопированный пароль — смените его на сервере
базы данных.

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.

**Sharing database connections.** A saved connection can now be shared with a
verified colleague's device, like hosts and Kubernetes contexts. It needs sync
server 0.1.15 or later with the feature enabled; `sync.consolecrypt.dev` has
it on. With an older server the Share action is hidden.

- Share from the connection menu in Databases. The preview shows exactly what
  the colleague receives: engine, address, port, database, user, TLS, limits
  and the production marker.
- Groups, query history, AI settings and session permissions are never
  shared. The password and client key are included only when explicitly
  selected and only if the server supports shared secrets.
- A route through an SSH host, jump profile, proxy or tunnel is not shared:
  the recipient picks their own. A Kubernetes forward keeps its namespace and
  workload, a Docker forward its container name. Such a connection cannot be
  saved as direct.
- Shared connections appear in Databases with a "Shared" badge ("Shared by
  you" for your own). From there you accept, save a personal copy, manage
  members and revoke access.
- A copy of a production connection stays read-only production.
- Connections can be added to shared collections.

Revoking access does not invalidate an already copied password; change it on
the database server.

See the [Databases guide](DATABASES.md) and the [Sharing guide](SHARING.md).
