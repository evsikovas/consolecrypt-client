# ConsoleCrypt 1.0.5

## Русский

Корзина менеджера паролей KDBX теперь имеет значок корзины и понятные действия.

- При выборе корзины или папки внутри неё доступна кнопка **Очистить корзину**.
  Это действие есть и в меню **⋯** / ПКМ самой корзины.
- После подтверждения окончательно удаляются **все группы, вложенные группы и
  записи** корзины. Действующие данные вне корзины сохраняются.
- Для отдельной удалённой группы или записи в меню доступно **Удалить навсегда**,
  также с подтверждением. Удаление группы включает её вложенные папки и записи.
- Системная папка корзины остаётся пустой. Обычная папка с названием Trash
  определяется как обычная папка: действия привязаны к идентификатору корзины KDBX.
- Изменения сразу сохраняются локально или через WebDAV. Поиск внутри выбранной
  корзины находит удалённые записи; общий поиск и палитра продолжают исключать их.

Исправление доступно на Windows, macOS, Linux DEB/RPM, Android и iOS Simulator.
Протокол синхронизации и сервер не меняются.

## English

The KDBX password manager now identifies its recycle bin with a trash icon and
provides clear actions for its contents.

- Select the bin or a recycled folder to use **Empty recycle bin**, also available
  from the bin's **⋯** / right-click menu.
- Confirmation permanently deletes **all groups, nested groups and entries** in
  the bin, preserving active data outside it.
- **Delete permanently** removes an individual recycled entry or group after
  confirmation; deleting a group includes every nested folder and entry.
- The system bin itself stays empty. Actions use the KDBX bin UUID rather than
  treating an ordinary folder named Trash as the recycle bin.
- Changes autosave locally or through WebDAV. Search inside the selected trash
  includes deleted entries; general search and the palette still exclude them.

Included on Windows, macOS, Linux DEB/RPM, Android and iOS Simulator.
No sync protocol or server changes are required.
