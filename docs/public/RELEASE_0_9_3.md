# ConsoleCrypt 0.9.3

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.
Исправления команд работают со всеми серверами синхронизации с командами (0.2.1 и новее);
сервер 0.2.4 дополнительно запрещает участникам менять чужие ключи команды, разрешает назначать администраторов только владельцу и отправляет приглашения только на подтверждённые адреса.

**Новое.**

- **Комплект восстановления** можно напечатать или сохранить в PDF.
- **Вкладки терминала** можно расположить колонкой у левого края; свёрнутая боковая
  панель запоминается.
- **Импорт ключа SSH из файла:** кнопка «Выбрать файл…» в окне импорта ключа.
- **Пароль к ключу SSH** при подключении спрашивается отдельным окном с именем ключа,
  а не как пароль сервера.
- **Импорт из CSV в SQLite и Cassandra/ScyllaDB.** Мастер импорта теперь работает и с
  этими базами. SQLite через `sqlite3` на хосте и Cassandra пишут без общей транзакции
  (в CQL их нет), поэтому вариант «одна транзакция на весь файл» для них не
  показывается. В Cassandra «добавлять» не перезаписывает существующие строки
  (`INSERT … IF NOT EXISTS`). «Создать таблицу из CSV…» доступно и для SQLite.
- **Конструктор таблиц для SQLite и Cassandra/ScyllaDB.** Таблицы создаются и меняются
  без ручного SQL и CQL. Изменения SQLite, которые не умеет `ALTER TABLE`, пересоздают
  таблицу одной транзакцией с сохранением строк и индексов; таблицу с триггерами или
  вычисляемыми столбцами конструктор не пересоздаёт. Для Cassandra задаются ключ
  партиции, кластерные столбцы с порядком, вторичные индексы, TTL и `gc_grace_seconds`.
- **История разговоров с ИИ.** Кнопка «История разговоров» в чате показывает разговоры
  этой сессии и сохранённые в хранилище (настройка «Хранить разговоры с ИИ в
  хранилище»); их можно продолжить или удалить. Продолженный разговор снова передаёт
  модели прежние ходы, со скрытыми, как и раньше, данными: раньше модель начинала его с
  пустого контекста.

**Исправления.**

- **SFTP: восстановление правок на других хостах.** Подключение, открытое для
  восстановления несохранённых правок на хосте, которого нет в браузере, теперь
  общее для правок этого хоста и закрывается вместе с последней из них, а не живёт до
  выхода из приложения.

**Безопасность.** Исправления по итогам ревью безопасности кода.

- **Команды: подтверждение участника.** Ключ команды получают ровно те ключи участника,
  по которым посчитан код на экране администратора. Если после сверки сервер сообщит
  другие ключи или роль, добавление откажет, и код нужно сверить заново. Раньше
  взломанный сервер мог подменить ключи между сверкой и подтверждением и получить
  доступ к хранилищу команды.
- **Команды: вступление.** Код, который приглашённый диктует администратору, теперь
  привязывает команду к ключу этого администратора. Команда откроется, только если
  участника добавил именно он; поддельную команду, собранную сервером, приложение
  не откроет.
- **Команды: ключ команды закрепляется.** Приложение запоминает ключ команды, с которым
  работало. Подменить его сервер больше не может: другой ключ той же эпохи или новый
  ключ, который не связан цепочкой с прежним, приводит к паузе команды, а не к записи
  новых данных под чужим ключом.
- **Команды: ключ подтверждает администратор.** С сервером 0.2.4 (протокол 1.12)
  владелец или администратор подписывает отпечаток ключа команды при её создании, при
  смене ключа и перед добавлением участника. Новый участник открывает команду, только
  если ключ совпадает с подписанным отпечатком, — подменить ключ сервер не может даже
  при самом первом открытии. Для команд, созданных раньше, отпечаток публикует первый
  администратор, открывший команду в 0.9.3.
- **Общий доступ: свои устройства.** Устройство, которое сервер выдаёт за ваше, больше
  не становится получателем секрета, подключения Kubernetes или базы данных: свои
  устройства получают общий доступ только через подтверждённое подключение. Поиск по
  адресу собственного аккаунта отклоняется.
- **SFTP на Windows.** При скачивании папки имена вида `C:файл`, `файл:поток`, `CON`,
  `NUL` или с точкой/пробелом в конце больше не принимаются: раньше сервер мог записать
  файл за пределы выбранной папки или снять у соседнего файла метку «скачано из
  интернета».
- **ИИ: оценка риска команд.** Команды, которые сами запускают программы или пишут
  файлы, больше не помечаются как «только чтение»: `env -S '…'`, `fd -x/-X`,
  `rg --pre`, скрипты `sed` с командами `e`/`w` и флагами `s///e`/`s///w`, `awk` с
  `| getline` или файлом программы, `git -c …`. Для них снова нужно отметить, что вы
  понимаете риск, перед «Выполнить».
- **Новое устройство: подтверждение без проверки.** Если сервер хоть раз сообщил
  этому устройству, что поддерживает проверку по коду пары, переключить его на старый
  способ без проверки он уже не может: такое подтверждение отклоняется. Одобряющее
  устройство должно быть версии 0.9.2 или новее.
- **Синхронизация: испорченные данные.** Если сервер присылает объект, который не
  проходит проверку подлинности, приложение показывает предупреждение и оставляет
  последнюю хорошую локальную копию, а не заменяет её нечитаемыми данными.
- **SSH-агент на Windows.** Канал встроенного SSH-агента теперь доступен только вашей
  учётной записи и системе: раньше по умолчанию к нему могли подключаться администраторы,
  а на чтение — любые пользователи.
- **Зависимости.** `crypto-bigint` 0.7.10 (RUSTSEC-2026-0387, неравномерное по
  времени выполнение операций, которые должны быть постоянными), `tokio` 1.54.1
  (RUSTSEC-2026-0386) и плановые обновления `hyper`, `uuid`, `tokio-rustls`, `xml` и
  других библиотек.

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.
The team fixes work with every sync server that has teams (0.2.1 and later); server 0.2.4
also stops members from changing other members' team keys, lets only the owner appoint
admins and sends invitations only to verified addresses.

**New.**

- The **Recovery Kit** can be printed or saved as PDF.
- **Terminal tabs** can sit in a column at the left edge; a folded sidebar is remembered.
- **Import an SSH key from a file:** “Choose file…” in the import key dialog.
- **The SSH key passphrase** is asked at connect time in its own prompt naming the key,
  not as the server password.
- **CSV import into SQLite and Cassandra/ScyllaDB.** The import wizard now works with
  these databases too. SQLite through `sqlite3` on a host and Cassandra write without
  a shared transaction (CQL has none), so the whole-file transaction is not offered
  there. In Cassandra, insert does not overwrite existing rows (`INSERT … IF NOT EXISTS`).
  “Create table from CSV…” works for SQLite as well.
- **Table designer for SQLite and Cassandra/ScyllaDB.** Create and change tables without
  writing SQL or CQL. SQLite changes that `ALTER TABLE` cannot make rebuild the table in
  one transaction and keep its rows and indexes; a table with triggers or generated
  columns is never rebuilt. Cassandra tables get a partition key, clustering columns
  with their order, secondary indexes, TTL and `gc_grace_seconds`.
- **AI conversation history.** “Conversation history” in the chat lists this session’s
  conversations and those kept in the vault (“Keep AI conversations in the vault”); they
  can be continued or deleted. A continued conversation gives the model its earlier turns
  again, with the same data hidden as before; before, the model started it with an empty
  context.

**Fixes.**

- **SFTP: recovering edits on other hosts.** The connection opened to recover unsaved
  edits on a host the browser is not showing is now shared by that host's edits and
  closes with the last of them instead of living until the app quits.

**Security.** Fixes from the code security review.

- **Teams: confirming a member.** The team key goes only to the member keys the code on
  the admin's screen was computed from. If the server reports other keys or another role
  after the check, adding fails and the code has to be compared again. Before, a
  compromised server could swap the keys between the check and the confirmation and
  gain access to the team vault.
- **Teams: joining.** The code the invitee reads to the admin now ties the team to that
  admin's key. The team opens only if that admin adds the member; a fake team put
  together by the server is not opened.
- **Teams: the team key is pinned.** The app remembers the team key it has used. The
  server can no longer swap it: another key for the same epoch, or a new key that does
  not chain back to the previous one, pauses the team instead of writing new data
  under a foreign key.
- **Teams: an admin vouches for the key.** With server 0.2.4 (protocol 1.12) the owner or
  an admin signs the team key's fingerprint when the team is created, when the key is
  rotated and before a member is added. A new member opens the team only if the key
  matches the signed fingerprint — the server cannot swap it, not even on the very
  first open. For teams created earlier, the first admin who opens the team in 0.9.3
  publishes the fingerprint.
- **Sharing: your own devices.** A device the server presents as yours no longer
  becomes a recipient of a secret, a Kubernetes or database connection: your own devices
  get shared items only through a verified enrollment. Looking up your own account's
  address is refused.
- **SFTP on Windows.** Folder downloads no longer accept names such as `C:file`,
  `file:stream`, `CON`, `NUL` or names ending in a dot or space: a server could write a
  file outside the chosen folder or strip a sibling file's "downloaded from the
  internet" mark.
- **AI: command risk.** Commands that run programs or write files themselves are no
  longer marked read-only: `env -S '…'`, `fd -x/-X`, `rg --pre`, `sed` scripts with the
  `e`/`w` commands or the `s///e`/`s///w` flags, `awk` with `| getline` or a program
  file, `git -c …`. They again need the risk acknowledgement before Run.
- **New device: approval without a check.** Once the server told this device that it
  supports the pairing-code check, it can no longer switch it back to the unchecked way:
  such an approval is refused. The approving device must run 0.9.2 or later.
- **Sync: damaged data.** If the server sends an object that fails authentication, the
  app shows a warning and keeps the last good local copy instead of replacing it with
  unreadable data.
- **SSH agent on Windows.** The built-in SSH agent's pipe is now open only to your account
  and the system; before, the default descriptor let administrators in and gave every
  user read access.
- **Dependencies.** `crypto-bigint` 0.7.10 (RUSTSEC-2026-0387, timing variability in
  operations meant to be constant-time), `tokio` 1.54.1 (RUSTSEC-2026-0386) and routine
  updates of `hyper`, `uuid`, `tokio-rustls`, `xml` and other libraries.
