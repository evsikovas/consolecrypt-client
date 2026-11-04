# ConsoleCrypt 1.0.3

## Русский

В менеджере паролей теперь можно переименовывать и удалять группы KDBX.

- На компьютере откройте меню **⋯** у папки или нажмите по ней правой кнопкой мыши.
- На телефоне выберите папку через кнопку групп, затем откройте **⋯** рядом.
- **Переименовать** меняет название выбранной папки, сохраняя её записи и вложенные группы.
- **Удалить** требует подтверждения и переносит папку со всем содержимым в корзину KDBX.
  Корневая группа и папки в корзине защищены от этих действий.
- Если системная корзина расположена внутри обычной папки, удаление её родителя запрещено;
  переименование остаётся доступным. Это предотвращает потерю вложенного содержимого.
- Обе операции автоматически сохраняются в локальный файл или через WebDAV.
  Если файл изменился извне, прежняя защита от перезаписи сохраняется.

Исправление входит в Windows, macOS, Linux DEB/RPM, Android и iOS Simulator.
iOS Simulator предназначен для запуска на Mac, а не на физическом iPhone.
Протокол синхронизации и схема сервера не меняются; обновлять sync-сервер для этого исправления не требуется.

## English

The password manager now supports renaming and deleting KDBX groups.

- On desktop, use a folder's **⋯** menu or right-click the folder.
- On a phone, select a folder using the groups button, then open the adjacent **⋯** menu.
- **Rename** preserves the group's entries and nested folders.
- **Delete** asks for confirmation and moves the entire group tree to the KDBX recycle bin.
  The root group and recycled groups are protected from these actions.
- If the system recycle bin is nested inside an ordinary folder, deleting that ancestor is disabled;
  renaming it remains available. The worker independently enforces this guard before mutation.
- Both changes autosave to the local file or WebDAV, retaining external-change protection.

Included in Windows, macOS, Linux DEB/RPM, Android and iOS Simulator packages.
The iOS Simulator package runs on a Mac and is not an iPhone installer.
No sync protocol, server schema or sync-server upgrade is required for this fix.
