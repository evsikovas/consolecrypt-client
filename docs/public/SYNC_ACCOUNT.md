# Аккаунт и синхронизация

## Русский

Профиль синхронизации связывает это устройство с аккаунтом на сервере
синхронизации ConsoleCrypt. Сервер хранит только зашифрованные данные: открыть
хранилище можно паролем хранилища, ключом восстановления или одобрением с уже
доверенного устройства. Пароль или вход в аккаунт лишь доказывают серверу, кто
вы.

### Подключение к серверу

1. **Добавить профиль → Подключиться к серверу** (или **Настройки →
   Синхронизация → Включить синхронизацию** для локального профиля).
2. Введите адрес сервера и нажмите **Проверить**. Приложение читает `/v1/meta`
   и показывает форму входа, которую ожидает сервер.
3. Новый аккаунт проходит обычное создание хранилища: пароль хранилища и
   Recovery Kit. Второе устройство подключается паролем хранилища, ключом
   восстановления или одобрением.

Приложение запоминает на этом устройстве, как вы входили на каждый сервер:
способ входа и логин (никогда пароль или токены). При следующем подключении
этот способ выбран заранее, логин подставлен, а под адресом видны недавние
серверы с меткой способа — «Email», «SSO: …», «AD». Подсказку можно удалить
кнопкой **Забыть**.

### Одобрение нового устройства и ключ хранилища (0.9.2)

Каждое устройство помнит отпечаток ключа своего хранилища и сверяет с ним любой
полученный ключ — при разблокировке паролем, ключом восстановления, отпечатком
или Windows Hello, при одобрении и при включении синхронизации. Если сервер
передал другой ключ, приложение показывает экран «Другой ключ хранилища»:
данные на устройстве не меняются, синхронизация остановлена, принять «новый»
ключ нельзя. Не вводите секреты на этом устройстве, скопируйте диагностику и
сообщите администратору сервера.

С сервером 0.2.2 и новее одобрение проходит так: на доверенном устройстве
откройте запрос в **Устройствах** — новое устройство сразу показывает имя
одобряющего и **код пары**. Код вычисляется из ключей обоих устройств и должен
совпадать на обоих экранах; отметьте совпадение на обоих. Подпись одобрения
охватывает ключ хранилища, и новое устройство проверяет её ключом, который вы
подтвердили. Со старым сервером одобрение работает как раньше, а ключ
хранилища помечается «не подтверждён» — один раз введите пароль хранилища
(**Устройства** или **Настройки → Хранилище → Подтвердить паролем**). Если
другое устройство аккаунта сообщило иной ключ, в **Устройствах** появится
предупреждение с предложением отозвать его.

### Запрос на удаление хранилища (0.9.9, протокол 1.13)

Сервер 0.2.4 разрешает ещё не доверенному устройству владельца, на котором
хранилище разблокировано, запросить удаление хранилища. Хранилище при этом не
удаляется сразу: 7 дней оно ждёт в состоянии «удаление запланировано» и
продолжает синхронизироваться. Доверенное устройство показывает в
«Синхронизации» предупреждение «Хранилище будет удалено …» с кнопкой
«Сохранить хранилище» — она отменяет удаление. Если запрос сделали не вы,
отмените удаление и смените пароль аккаунта. Удаление с доверенного устройства
по-прежнему происходит сразу.

### Удалённое хранилище и проверка данных (0.9.9)

Если хранилище удалено на сервере, синхронизация останавливается с понятной
причиной («хранилище не найдено на сервере») и не повторяет попытки бесконечно.
Объект с сервера, который не прошёл проверку подлинности, не заменяет последнюю
хорошую локальную копию: приложение показывает предупреждение. Если сервер хоть
раз сообщил устройству о проверке по коду пары, одобрение без этой проверки
больше не принимается.

### Корпоративный вход (Enterprise)

Сервер синхронизации 0.2.0 и новее (протокол 1.8) может работать в
enterprise-режиме: вход в аккаунт выполняется только через корпоративный
сервер авторизации организации. Регистрации, пароля аккаунта и сброса пароля
по почте в этом режиме нет.

- **IdP по OIDC** (например, Entra ID, AD FS, Keycloak, Google Workspace):
  кнопка **Войти через {название}** открывает страницу входа организации в
  системном браузере. Поле e-mail необязательно — оно только подставляется
  на странице входа. Пока идёт вход, приложение ждёт браузер; **Отмена**
  прерывает вход. На компьютере браузер возвращается в приложение через
  адрес `http://127.0.0.1:…/auth/callback`, на Android и iOS — через
  `consolecrypt://auth/callback`. Приложение использует PKCE и проверяет
  `state`; одноразовый код входа живёт 60 секунд.
- **Active Directory / LDAP:** поля **Логин (email или DOMAIN\user)** и
  **Пароль** под подписью «Учётная запись вашей организации». Пароль
  передаётся серверу по TLS один раз, не сохраняется и стирается из поля
  после каждой попытки.

Везде, где показан аккаунт или подключение к серверу, приложение говорит, что
используется корпоративный сервер авторизации:

- на экране входа — «Вход через корпоративный сервер авторизации {название}.
  Пароль организации не открывает хранилище — для этого нужен пароль
  хранилища.»;
- в **Настройках → Аккаунт и синхронизация** — строка **Авторизация:
  корпоративный сервер — {название} (OIDC | Active Directory)** с пояснением,
  а также как и когда это устройство вошло. На стандартном сервере там
  написано «Email и пароль аккаунта»;
- в разделах **Синхронизация** и **Устройства**.

Если сервер сменил режим (например, перешёл со входа по паролю на
корпоративный), экран входа предупреждает об этом: прежний пароль аккаунта
больше не используется.

Корпоративный вход не даёт доступа к данным. После входа устройство —
обычное новое устройство: действуют защита от входа с нового устройства,
письмо о новом устройстве и одобрение. Ни IdP, ни Active Directory, ни сервер
не получают ничего, чем можно открыть хранилище.

**Доступ отключён администратором.** Если организация отключила учётную
запись (в IdP, в AD или командой администратора сервера), сервер отзывает все
сессии. Приложение блокирует профиль и показывает «Доступ отключён
администратором». Локальные данные остаются зашифрованными. Можно попробовать
**Войти снова** (если доступ вернули), переключиться на другой профиль или
**Удалить профиль с этого устройства** — на сервере при этом ничего не
меняется.

Другие сообщения корпоративного входа: домен почты не допущен, нет нужной
группы, требуется второй фактор, учётная запись организации уже связана с
другим аккаунтом, служба входа организации недоступна, слишком много попыток
(приложение показывает, когда можно повторить).

Старые серверы и серверы в стандартном режиме работают как раньше.
Настройка enterprise-режима на сервере описана в документации сервера.

## English

A sync profile links this device to an account on a ConsoleCrypt sync server.
The server only stores encrypted data: the vault is opened with the vault
passphrase, the Recovery Key or approval from an already trusted device.
Signing in to the account only proves to the server who you are.

### Connecting to a server

1. **Add profile → Connect to a server** (or **Settings → Sync → Enable sync**
   for a local profile).
2. Enter the server address and press **Check**. The app reads `/v1/meta` and
   shows the sign-in form the server expects.
3. A new account goes through the usual vault setup: vault passphrase and
   Recovery Kit. A second device joins with the vault passphrase, the Recovery
   Key or approval.

The app remembers on this device how you signed in to each server: the method
and the login (never a password or tokens). Next time that method is
preselected, the login is filled in, and recent servers are listed under the
address with a method badge — "Email", "SSO: …", "AD". **Forget** removes the
hint.

### Approving a new device and the vault key (0.9.2)

Every device remembers the fingerprint of its vault key and checks every key it
receives against it — on passphrase, Recovery Key or biometric unlock, approval
and enabling sync. If the server sends a different key, the app shows
"Different vault key": data on the device is unchanged, sync stops, and there
is no way to accept the "new" key. Do not enter secrets on that device; copy
the diagnostics and tell the server administrator.

With server 0.2.2 and later, open the request under **Devices** on a trusted
device: the new device then shows the approver's name and a **pairing code**
computed from both devices' keys. Confirm that the codes match on both screens.
The approval signature covers the vault key and the new device verifies it with
the key you confirmed. With an older server approval works as before and the
vault key is marked "not confirmed" until you enter the vault passphrase once
(**Devices** or **Settings → Vault → Confirm with passphrase**). If another
device of the account reports a different key, **Devices** warns and offers to
revoke it.

### A request to delete the vault (0.9.9, protocol 1.13)

Server 0.2.4 lets a device of the owner that is not trusted yet, with the
vault unlocked, ask to delete the vault. It is not deleted at once: for 7
days it waits as “deletion scheduled” and keeps syncing. A trusted device
shows “This vault will be deleted on …” under Sync with **Keep the vault**,
which cancels the deletion. If you did not ask for it, keep the vault and
change your account password. A trusted device still deletes at once.

### A deleted vault and data checks (0.9.9)

When the vault was deleted on the server, sync stops with a clear reason (“vault
not found on server”) instead of retrying forever. An object from the server that
fails authentication never replaces the last good local copy; the app shows a
warning. Once the server told a device about the pairing-code check, an approval
without that check is refused.

### Corporate sign-in (Enterprise)

Sync server 0.2.0 and later (protocol 1.8) can run in enterprise mode: account
sign-in goes only through the organisation's corporate authorization server.
There is no registration, account password or password reset by e-mail in
this mode.

- **OIDC identity provider** (for example Entra ID, AD FS, Keycloak, Google
  Workspace): **Sign in with {name}** opens the organisation's sign-in page in
  the system browser. The e-mail field is optional — it is only pre-filled on
  the sign-in page. While signing in, the app waits for the browser; **Cancel**
  stops it. On desktops the browser returns to the app through
  `http://127.0.0.1:…/auth/callback`, on Android and iOS through
  `consolecrypt://auth/callback`. The app uses PKCE and checks `state`; the
  single-use sign-in code lives 60 seconds.
- **Active Directory / LDAP:** **Login (email or DOMAIN\user)** and
  **Password** under "Your organisation's account". The password is sent to
  the server once over TLS, never stored, and cleared from the field after
  every attempt.

Wherever the account or the server connection is shown, the app says that a
corporate authorization server is used:

- on the sign-in screen — "Sign-in through the corporate authorization server
  {name}. Your organisation's password does not open the vault — the vault
  passphrase does.";
- in **Settings → Account and sync** — the **Authorization: Corporate server —
  {name} (OIDC | Active Directory)** row with an explanation, and how and when
  this device signed in. On a standard server it reads "E-mail and account
  password";
- in **Sync** and **Devices**.

If the server changed its mode (for example from password sign-in to corporate
sign-in), the sign-in screen says so: the previous account password is no
longer used.

Corporate sign-in grants no access to data. After signing in the device is an
ordinary new device: new-device protection, the new-device e-mail and approval
apply. Neither the IdP, Active Directory nor the server receives anything that
can open the vault.

**Access disabled by the administrator.** When the organisation disables the
account (in the IdP, in AD or with the server administrator's command), the
server revokes every session. The app locks the profile and shows "Access
disabled by the administrator". Local data stays encrypted. You can **Sign in
again** (if access was restored), switch to another profile, or **Delete
profile from this device** — nothing changes on the server.

Other corporate sign-in messages: the e-mail domain is not allowed, the
required group is missing, a second factor is required, the organisation's
account is already linked to another account, the organisation's sign-in
service is unavailable, too many attempts (the app shows when to retry).

Older servers and servers in standard mode work as before. Setting up the
enterprise mode is described in the server documentation.
