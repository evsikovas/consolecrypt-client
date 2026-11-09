# ConsoleCrypt 1.1.0

## Русский

Минималистичный терминал для рабочего стола, более удобный менеджер паролей и
отдельные Android-сборки для RuStore.

- **Минимализм** на Windows, macOS и Linux: кнопка над терминалом и
  Ctrl+Shift+M / ⌘⇧M скрывают панели приложения, оставляя терминал и вкладки.
  Вкладки можно расположить сверху или слева. Открытые сессии, вывод, разделение
  терминала и палитра Ctrl+K / ⌘K сохраняются. На телефонах этот режим не включается.
- **SFTP из терминала**: кнопка слева сверху и действие «Открыть SFTP» в ПКМ
  по SSH-вкладке открывают файлы соответствующего сервера, сохраняя терминальные
  сессии. Работает и в минималистичном режиме.
- **Пароли KDBX**: в списке показаны только разблокированные базы. Маленькая
  кнопка с замком открывает остальные сохранённые базы; их параметры WebDAV и
  меню управления остаются доступны до разблокировки.
- Фильтры «Все записи» и «Без папки» отделены от настоящих групп, корзина — от
  действующих папок. Иерархия и данные KDBX сохраняются. Карточки полей стали
  компактнее, включая длинные значения и увеличенный масштаб текста.
- В строке открытой WebDAV-базы доступно меню настройки быстрого доступа через
  системную аутентификацию и физический ключ там, где эти способы поддерживаются.
  Первая разблокировка требует пароль; платформенные ограничения FIDO2 сохраняются.
- **SQL**: тонкие вертикальная и горизонтальная полосы прокрутки увеличиваются
  при наведении и перетаскивании. Сохранены редактирование и закреплённые столбцы.
- **Настройки** сворачиваются нажатием на заголовок блока; введённые значения
  сохраняются. Раздел SFTP переименован в «Редактор по умолчанию для SFTP».
- **RuStore**: отдельные production-подписанные APK и AAB для Android 11+ ARM64.
  В варианте магазина нет REQUEST_INSTALL_PACKAGES и встроенной установки APK;
  обновления выполняются через RuStore. Обычный APK остаётся отдельным каналом
  для совместимости с ранее установленными версиями.

Для RuStore используются постоянные ключи приложения и загрузки. Новый
production-ключ отличается от прежнего Android preview/debug-ключа: перед
переходом между такими установками сохраните резервную копию локальных данных.
Публикация файлов на GitHub не означает прохождения модерации RuStore.

Доступны Windows x64, универсальный macOS DMG, Linux DEB/RPM, Android APK,
RuStore APK/AAB и iOS Simulator на Mac. Архив Simulator не устанавливается на iPhone.
Сервер синхронизации и формат зашифрованных данных не изменились; обновление
сервера для этого выпуска не требуется. Интеграция с Apple «Пароли» не добавляется.

## English

ConsoleCrypt 1.1.0 adds a minimal desktop terminal, improves the KDBX password
manager, and introduces separately signed Android packages for RuStore.

- **Minimal mode**, Windows/macOS/Linux: use the terminal toolbar button or
  Ctrl+Shift+M / ⌘⇧M. Keep terminal tabs above or beside the terminal, with live
  sessions, split panes and Ctrl+K / ⌘K command palette. Mobile layout stays unchanged.
- **Open SFTP** from the upper-left terminal button or an SSH tab's context menu.
  It targets that tab's server, including inactive tabs and minimal mode, without
  closing terminal sessions.
- **KDBX databases** list only unlocked databases. A small unlock button provides
  access to other remembered databases and their WebDAV settings/context actions.
  Root-entry filters, actual folders and the recycle bin have distinct sections.
  Field cards are more compact and still support long values and larger text.
- Open WebDAV databases expose existing device-authentication and hardware-key
  protection actions on supported platforms. Initial opening still requires the
  database password; existing FIDO2 platform limits remain.
- **SQL results** have thin two-axis scrollbars that expand on hover or drag,
  preserving cell editing and pinned columns.
- **Settings sections** fold by clicking their title while retaining form values.
  The SFTP section is named “Default editor for SFTP”.
- **RuStore APK/AAB**: production-signed Android 11+ ARM64 packages use store
  updates only, without REQUEST_INSTALL_PACKAGES or an in-app APK installer.
  The ordinary APK remains a separate update channel for existing installations.

Back up local data before switching from an older debug-signed Android APK to
the new production certificate. GitHub assets are not a claim of RuStore approval.
Windows, macOS, Linux DEB/RPM, Android and the iOS Simulator archive are included.
No sync-server or encrypted-data-format changes are required. Apple Passwords
integration is not part of this release.
