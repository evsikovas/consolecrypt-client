# ConsoleCrypt 0.9.0

## Русский

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

## English

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

See the [Account and sync guide](SYNC_ACCOUNT.md).
