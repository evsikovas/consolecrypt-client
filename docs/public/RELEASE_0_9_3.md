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
