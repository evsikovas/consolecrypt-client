# ConsoleCrypt 0.9.1

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

**Команды** (сервер синхронизации 0.2.1). Команда — общее хранилище
нескольких аккаунтов на одном сервере: её хосты, группы, учётные данные и
сниппеты видят все участники, а роль решает, кто может их менять. Названия,
настройки и ресурсы команды зашифрованы сквозным шифрованием — сервер видит
только состав участников.

- «Настройки → Команды»: создать команду с названием, цветом и описанием; она
  сама появляется на всех ваших устройствах.
- Приглашение по почте с ролью и комментарием (комментарий не шифруется).
  Приглашённый принимает его и диктует администратору код из 6 групп цифр;
  участник добавляется только после того, как администратор подтвердит
  совпадение. «Коды не совпадают» никого не добавляет.
- Роли: владелец, администратор, редактор, наблюдатель. Наблюдатель только
  просматривает и подключается — изменения отклоняют и приложение, и сервер.
- В «Хостах» слева разделы «Личное» и по одному на команду с её цветом. Всё,
  что создано в группе команды, принадлежит команде; хост можно скопировать
  или переместить в команду из его меню — нужные учётные данные копируются
  вместе с ним. Ресурсы команды отмечены её цветом в списках и на вкладках
  терминала; у наблюдателя — меткой «Только чтение».
- Политики команды: помечать новые контексты Kubernetes и базы данных как
  прод, не отправлять ресурсы команды ИИ-помощникам, быстрее очищать
  скопированные пароли, требовать подтверждения каждой команды на хостах
  команды. Свой цвет команды можно выбрать только для своих устройств.
- Удаление или уход участника меняет ключ команды: всё перешифровывается, и
  бывший участник не прочитает новые данные. Вкладка «История» показывает
  подписанный журнал состава.
- Каждое изменение состава подписывает администратор, а каждое приложение
  проверяет весь журнал само. Если сервер покажет участника, которого никто не
  подписывал, или вернёт старый состав, команда встаёт на паузу «только
  чтение» с предупреждением, и ключ никому не выдаётся.
- Команда открывается и без сети — из кэша в зашифрованной базе профиля;
  изменения уходят, когда сеть появится.

Нужен сервер синхронизации 0.2.1 или новее; со старым сервером раздел
«Команды» не показывается. Работает и в обычном, и в корпоративном режиме
входа. Личные хранилища, общий доступ, общие заметки и резервные копии
работают как раньше.

## English

Available for macOS, Windows, Linux and Android. iOS builds and checks are postponed.

**Teams** (sync server 0.2.1). A team is a vault shared by several accounts on
the same server: all members see its hosts, groups, credentials and snippets,
and roles decide who may change them. Team names, settings and resources are
end-to-end encrypted — the server only sees who is a member.

- Settings → Teams: create a team with a name, a colour and a description; it
  appears on all your devices by itself.
- Invite by email with a role and a note (the note is not encrypted). The
  invitee accepts and reads a code of 6 groups of digits to the admin; the
  member is added only after the admin confirms the codes match. "Codes don't
  match" adds nobody.
- Roles: owner, admin, editor, viewer. Viewers only view and connect — both the
  app and the server reject their changes.
- Hosts shows "Personal" and one section per team in its colour. Everything
  created in a team group belongs to the team; a host can be copied or moved
  into a team from its menu, together with the credentials it needs. Team
  resources carry the team colour in lists and on terminal tabs, and a
  "Read only" badge for viewers.
- Team policies: mark new Kubernetes contexts and databases as production,
  never send team resources to AI assistants, clear copied passwords sooner,
  and require confirming every command on team hosts. You can pick your own
  team colour for your devices only.
- Removing a member, or a member leaving, rotates the team key: everything is
  re-encrypted and the former member cannot read new data. The History tab
  shows the signed membership log.
- Every membership change is signed by an admin, and every app verifies the
  whole log itself. If the server shows a member nobody signed, or an older
  membership, the team is paused read-only with a warning and no key is handed
  out.
- Teams open offline from a cache in the encrypted profile database; changes
  are sent when the network is back.

Requires sync server 0.2.1 or later; with an older server Teams is hidden.
Works in both the standard and the corporate sign-in mode. Personal vaults,
sharing, shared notes and backups work as before.
