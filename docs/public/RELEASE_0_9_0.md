# ConsoleCrypt 0.9.0

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

**Корпоративный вход в сервер синхронизации.** Сервер синхронизации 0.2.0 и
новее может пускать в аккаунты только через сервер авторизации организации.
Приложение само узнаёт режим сервера после ввода адреса и показывает нужную
форму.

- **Вход через SSO (OIDC).** Кнопка «Войти через {название}» открывает
  страницу входа организации в системном браузере; e-mail можно указать
  заранее. Пока идёт вход, виден статус ожидания и кнопка «Отмена». На
  компьютере браузер возвращается в приложение через локальный адрес
  `127.0.0.1`, на Android — через Custom Tabs, на iOS — через системное окно
  входа. Используются PKCE и проверка `state`; код входа одноразовый.
- **Вход через Active Directory.** Поля «Логин (email или DOMAIN\user)» и
  «Пароль» с подписью «Учётная запись вашей организации». Пароль отправляется
  один раз по TLS, не сохраняется и стирается из поля после каждой попытки.
- В корпоративном режиме нет регистрации, пароля аккаунта и «Забыли пароль».
  Новый пользователь после входа проходит обычное создание хранилища.

**Видно, где используется корпоративный сервер.** На экране входа:
«Вход через корпоративный сервер авторизации {название}. Пароль организации
не открывает хранилище — для этого нужен пароль хранилища.» В «Настройках»,
«Синхронизации» и «Устройствах» — строка «Авторизация: корпоративный сервер —
{название} (OIDC | Active Directory)»; на обычном сервере — «Email и пароль
аккаунта».

**Приложение помнит, как вы входили.** Для каждого сервера на устройстве
сохраняются способ входа и логин (не пароль). При следующем подключении способ
выбран заранее, логин подставлен, видна подсказка «В прошлый раз вы входили
через … как …» с кнопкой «Забыть», а недавние серверы показаны с меткой
«Email», «SSO: …» или «AD». Если сервер сменил режим входа, приложение
предупреждает об этом. В настройках видно, как и когда это устройство вошло.

**Доступ отключён администратором.** Когда организация отключает учётную
запись, сервер отзывает сессии, а приложение блокирует профиль и показывает
«Доступ отключён администратором». Данные на устройстве остаются
зашифрованными; профиль можно удалить с устройства или войти снова, если
доступ вернули. Ошибки корпоративного входа (домен не допущен, нет группы,
нужен второй фактор, учётная запись уже связана, служба входа недоступна,
слишком много попыток) показываются понятным текстом.

**Что не меняется.** Шифрование, хранилища, устройства и общий доступ
работают как раньше: корпоративный вход заменяет только пароль аккаунта.
После входа устройство — обычное новое устройство, хранилище открывает пароль
хранилища, ключ восстановления или одобрение. Со старыми серверами и
серверами в стандартном режиме всё работает как прежде.

На iOS добавлен системный обработчик входа через браузер; он же используется
для входа OIDC в Kubernetes.

**Заметки, задачи и напоминания.** В правой панели рядом со «Сниппетами» и
«ИИ» появился инструмент «Заметки» (также в палитре команд и в шапке на
телефоне).

- Заметки с Markdown, метками и закреплением. Переключатель «Задача» добавляет
  срок; просроченные задачи выделены, а число просроченных задач и
  сработавших напоминаний видно на значке «Заметок».
- Напоминания: «Через час», «Сегодня в 18:00», «Завтра в 09:00», «Через неделю»
  или своё время. Пока приложение открыто, в срок появляется уведомление с
  кнопкой «Открыть»; в меню заметки — «Напомнить через час». Системных
  уведомлений при закрытом приложении пока нет.
- Заметки к объектам. Вкладка «Объект» показывает заметки к тому, что открыто
  сейчас: SSH-хосту активной вкладки терминала, RDP-хосту, хосту SFTP,
  открытому подключению к базе данных, ресурсу или контексту Kubernetes,
  выбранному Docker-контейнеру. Вкладка «Все» показывает все заметки и к чему
  они привязаны, «Задачи» — только задачи.
- Текст из поля «Заметки» карточки хоста можно перенести в обычную заметку
  кнопкой «Перенести в заметки».
- Заметки шифруются на устройстве и синхронизируются сквозным шифрованием, как
  остальные данные хранилища. Удаление хоста или базы заметки не удаляет — у
  них появляется пометка «Объект не найден».

**Общие заметки** (сервер синхронизации 0.2.0 с включённым
`CC_SHARED_NOTES_ENABLED`).

- У общего хоста, подключения к базе данных или контекста Kubernetes во
  вкладке «Объект» появляется блок «Общие заметки». Их видят все участники
  объекта; писать может любой участник, изменить — только автор, удалить —
  автор или владелец. Заметка к Docker-контейнеру общего SSH-хоста или к
  ресурсу общего контекста видна только у этого контейнера или ресурса.
- Ключ общих заметок выдаёт и подписывает устройство владельца, каждую заметку
  подписывает устройство автора — сервер не может подменить ключ или прочитать
  текст. Пока владелец ни разу не открыл панель заметок для объекта, участники
  видят сообщение об ожидании. После исключения участника устройство владельца
  при следующем открытии панели перешифровывает заметки новым ключом.
- Связь объекта с общим доступом запоминается при публикации и при сохранении
  личной копии. Для объектов, опубликованных до 0.9.0 (кроме скопированных
  хостов), опубликуйте или скопируйте объект заново, чтобы у него появились
  общие заметки.

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.

**Corporate sign-in to the sync server.** Sync server 0.2.0 and later can admit
accounts only through the organisation's authorization server. After you enter
the server address, the app detects the server's mode and shows the matching
form.

- **SSO sign-in (OIDC).** "Sign in with {name}" opens the organisation's
  sign-in page in the system browser; the e-mail can be entered beforehand.
  While signing in, the app shows a waiting state with Cancel. On desktops the
  browser returns to the app through a local `127.0.0.1` address, on Android
  through Custom Tabs, on iOS through the system sign-in sheet. PKCE and a
  `state` check are used; the sign-in code is single-use.
- **Active Directory sign-in.** "Login (email or DOMAIN\user)" and "Password"
  under "Your organisation's account". The password is sent once over TLS,
  never stored and cleared from the field after every attempt.
- Corporate mode has no registration, account password or "Forgot password".
  A new user goes through the usual vault setup after signing in.

**The corporate server is always named.** The sign-in screen says "Sign-in
through the corporate authorization server {name}. Your organisation's
password does not open the vault — the vault passphrase does." Settings, Sync
and Devices show "Authorization: Corporate server — {name} (OIDC | Active
Directory)"; on a standard server it reads "E-mail and account password".

**The app remembers how you signed in.** For every server this device keeps
the sign-in method and login (never the password). Next time the method is
preselected, the login is filled in, a hint "Last time you signed in with …
as …" offers Forget, and recent servers carry an "Email", "SSO: …" or "AD"
badge. If the server changed its sign-in mode, the app warns about it.
Settings show how and when this device signed in.

**Access disabled by the administrator.** When the organisation disables the
account, the server revokes its sessions, and the app locks the profile and
shows "Access disabled by the administrator". Data on the device stays
encrypted; the profile can be deleted from the device, or you can sign in
again once access is restored. Corporate sign-in errors (domain not allowed,
group missing, second factor required, account already linked, sign-in
service unavailable, too many attempts) are explained in plain words.

**What stays the same.** Encryption, vaults, devices and sharing work as
before: corporate sign-in replaces only the account password. After signing
in the device is an ordinary new device; the vault opens with the vault
passphrase, the Recovery Key or approval. Older servers and servers in
standard mode work as before.

iOS gains a system browser sign-in handler, also used for Kubernetes OIDC.

**Notes, tasks and reminders.** The right workspace panel gains a "Notes"
tool next to Snippets and AI (also in the command palette and the phone
header).

- Notes with Markdown, tags and pinning. The "Task" switch adds a due time;
  overdue tasks are highlighted, and the Notes icon shows how many tasks are
  overdue and reminders went off.
- Reminders: in an hour, today at 18:00, tomorrow at 09:00, in a week or a
  time of your choice. While the app is open a notice with "Open" appears on
  time; the note menu offers "Remind in an hour". System notifications while
  the app is closed are not available yet.
- Notes about objects. The "Object" tab shows notes about what is open now:
  the SSH host of the active terminal tab, the RDP host, the SFTP host, the
  open database connection, the Kubernetes resource or context, the selected
  Docker container. "All" lists every note and what it is attached to, "Tasks"
  only tasks.
- The host card's free-text notes move into a regular note with "Move to
  notes".
- Notes are encrypted on the device and synced end-to-end encrypted like the
  rest of the vault. Deleting a host or database keeps its notes, marked
  "Object not found".

**Shared notes** (sync server 0.2.0 with `CC_SHARED_NOTES_ENABLED` on).

- A shared host, database connection or Kubernetes context shows "Shared
  notes" in the "Object" tab. Every member reads them; any member writes, only
  the author edits, the author or the owner deletes. A note about a Docker
  container of a shared SSH host or a resource of a shared context stays with
  that container or resource.
- The owner device issues and signs the notes key and the author device signs
  every note, so the server can neither substitute the key nor read the text.
  Members wait until the owner has opened the notes panel for the object once.
  After a member is removed, the owner's device moves the notes to a new key
  the next time the panel opens.
- The link between an object and its share is remembered when publishing it
  and when saving a personal copy. For objects published before 0.9.0 (other
  than copied hosts), publish or copy the object again to get shared notes.

See the [Account and sync guide](SYNC_ACCOUNT.md).
