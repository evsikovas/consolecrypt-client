# Буфер обмена / Clipboard

## Русский

ConsoleCrypt копирует данные тремя способами.

- **Секреты** — пароли, ключи, раскрытые значения Kubernetes Secret, переменные
  окружения Docker, чувствительные ячейки баз данных, слова комплекта
  восстановления, текст из буфера RDP-сеанса. Не попадают в историю буфера и на
  другие устройства, помечаются для менеджеров буфера как секрет и удаляются из
  буфера через заданный срок.
- **Личный текст** — выделение в терминале и логах, результаты запросов. Не
  попадает в историю буфера и на другие устройства, но не удаляется: обычная
  работа в терминале не должна мешать. Если выделение похоже на секрет
  (`password=…`, токен, приватный ключ), оно копируется как секрет.
- **Обычный текст** — пути, идентификаторы, публичные ключи, команды.

Срок удаления задаётся в «Настройки → Безопасность → Очищать буфер обмена»:
от 30 секунд до 1 дня, по умолчанию 30 секунд. Политика команды может только
сократить срок. Срок переживает перезапуск приложения. Буфер очищается, только
если в нём всё ещё наш секрет: то, что вы скопировали позже в другом приложении,
не стирается. Если безопасный буфер на устройстве недоступен, приложение
сообщает «Не удалось безопасно скопировать» и ничего не копирует.

| Платформа | Что гарантируется |
|---|---|
| Windows 10/11 | Секрет не попадает в журнал буфера (Win+V) и облачный буфер; мониторы буфера, соблюдающие соглашение Windows, его пропускают. Удаляется по сроку и при закрытии окна приложения. |
| macOS 12+ | Секрет не уходит на iPhone/iPad через универсальный буфер обмена, помечен как скрытый и временный для менеджеров буфера. Удаляется по сроку и при выходе из приложения. |
| Linux (GTK, X11 и Wayland) | Секрет есть только в буфере обмена, не в выделении для вставки средней кнопкой. Менеджеры буфера получают пометку «секрет» (KDE Klipper её учитывает). При выходе из приложения секрет пропадает, менеджеру буфера он не передаётся. |
| iOS | Только на этом устройстве (без Handoff), система удаляет секрет по сроку даже при выгруженном приложении. |
| Android 11+ | Секрет помечен как чувствительный: его не видно в системном уведомлении о копировании и в подсказках клавиатуры. Удаляется по сроку будильником системы, даже если приложение свёрнуто или выгружено; в режиме энергосбережения — с задержкой до ~10 минут. |

**Android в фоне.** Android не даёт свёрнутому приложению прочитать буфер
обмена, поэтому проверить, что там всё ещё наш секрет, нельзя. Настройка
«Очищать буфер в фоне, даже если нельзя проверить, что там наш секрет»
(включена по умолчанию) очищает буфер без проверки — при этом может пропасть
текст, который вы за время ожидания скопировали в другом приложении. Если
выключить её, секрет остаётся в буфере, пока вы не вернётесь в ConsoleCrypt;
тогда наш секрет удаляется, а чужой текст — нет. После перезагрузки телефона
очистка завершается при следующем открытии приложения.

Ограничения: приложение, которое уже прочитало буфер до удаления, могло
сохранить секрет; менеджеры буфера, не поддерживающие пометки, могут его
записать.

## English

ConsoleCrypt copies in three ways.

- **Secrets** — passwords, keys, revealed Kubernetes Secret values, Docker
  environment values, sensitive database cells, recovery words, text received
  from an RDP session. Kept out of clipboard history and other devices, marked
  as a secret for clipboard managers, and removed after the configured time.
- **Private text** — terminal and log selections, query results. Kept out of
  clipboard history and other devices but not removed, so ordinary terminal
  work is not disrupted. A selection that looks like a secret (`password=…`, a
  token, a private key) is copied as a secret.
- **Plain text** — paths, IDs, public keys, commands.

Set the time in Settings → Security → Clear clipboard: 30 seconds to 1 day,
30 seconds by default. A team policy can only shorten it. The deadline
survives an app restart. The clipboard is cleared only while it still holds our
secret: anything you copied later in another app is left alone. If the
protected clipboard is unavailable on the device, the app says "Couldn't copy
securely" and copies nothing.

| Platform | What is guaranteed |
|---|---|
| Windows 10/11 | The secret stays out of clipboard history (Win+V) and the cloud clipboard; clipboard monitors that follow the Windows convention skip it. Removed at the deadline and when the app window closes. |
| macOS 12+ | The secret is not sent to iPhone/iPad through Universal Clipboard and is marked concealed and transient for clipboard managers. Removed at the deadline and when the app quits. |
| Linux (GTK, X11 and Wayland) | The secret is on the clipboard only, never in the middle-click selection. Clipboard managers get a "secret" hint (KDE Klipper honours it). It disappears when the app quits and is not handed to a clipboard manager. |
| iOS | This device only (no Handoff); the system removes it at the deadline even if the app is not running. |
| Android 11+ | Marked sensitive: hidden from the system copy notification and keyboard suggestions. Removed at the deadline by a system alarm even when the app is in the background or was closed; in battery saver, up to ~10 minutes later. |

**Android in the background.** Android does not let a background app read the
clipboard, so it cannot check that our secret is still there. The setting
"Clear the clipboard in the background even if the secret can't be verified"
(on by default) clears it without a check — text you copied in another app
while waiting may be erased too. With it off, the secret stays until you return
to ConsoleCrypt; then our secret is removed and other content is not. After a
phone restart, the clear finishes the next time you open the app.

Limits: an app that read the clipboard before the clear may have kept the
secret; clipboard managers that ignore the hints may record it.
