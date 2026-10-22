# ConsoleCrypt 0.8.8

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

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

**Общие заметки** (сервер синхронизации 0.1.18 с включённым
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
  личной копии. Для объектов, опубликованных до 0.8.8 (кроме скопированных
  хостов), опубликуйте или скопируйте объект заново, чтобы у него появились
  общие заметки.

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.

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

**Shared notes** (sync server 0.1.18 with `CC_SHARED_NOTES_ENABLED` on).

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
  and when saving a personal copy. For objects published before 0.8.8 (other
  than copied hosts), publish or copy the object again to get shared notes.
