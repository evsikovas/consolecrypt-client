# ConsoleCrypt 0.9.3

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.
Исправления команд работают со всеми серверами синхронизации с командами (0.2.1 и новее);
сервер 0.2.4 дополнительно закрывает изменение чужих ключей команды.

**Новое.**

- **Комплект восстановления** можно напечатать или сохранить в PDF.
- **Вкладки терминала** можно расположить колонкой у левого края; свёрнутая боковая
  панель запоминается.

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

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.
The team fixes work with every sync server that has teams (0.2.1 and later); server 0.2.4
also stops members from changing other members' team keys.

**New.**

- The **Recovery Kit** can be printed or saved as PDF.
- **Terminal tabs** can sit in a column at the left edge; a folded sidebar is remembered.

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
