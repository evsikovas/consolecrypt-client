# ConsoleCrypt 0.3.0 · SSH и удалённые рабочие столы

**Добавлена возможность подключиться к RDP.**

Windows-компьютеры теперь можно держать рядом с SSH-серверами в одном списке
хостов. Выберите тип подключения при создании хоста — форма покажет подходящие
поля. Сохранённые подключения синхронизируются через ваше зашифрованное хранилище.

- **Рабочие столы во вкладках.** До четырёх параллельных RDP-сеансов, панель
  вкладок в стиле терминала и независимое закрытие подключений.
- **Настоящий полный экран.** Рабочий стол занимает весь экран компьютера.
  Выдвижная панель сверху позволяет переключить подключение, свернуть окно,
  вернуться к обычному размеру и закрыть сеанс.
- **Обмен текстом.** Ctrl+V (⌘V на macOS) передаёт локальный текст и сразу
  вставляет его в активный RDP-сеанс. Разрешение включается отдельно для каждого
  сеанса; кнопки отправки и получения также доступны.
- **Обмен файлами.** Выберите папку на macOS, Windows или Linux. Она появится
  как перенаправленный диск в Windows. По умолчанию доступна только для чтения;
  запись включается отдельно.
- **Проверка сервера.** Подтверждение сертификата до аутентификации; сохранённый
  пароль остаётся в нативном ядре и не передаётся в интерфейс.
- **Вставка в SSH-терминал на Windows.** Помимо Ctrl+V, доступна комбинация
  Shift+Insert с той же проверкой многострочного текста перед вставкой.
- **Обновлённое руководство.** Пошаговое подключение RDP, вкладки и разрешения
  описаны на русском и английском. Раздел SFTP называется «Редактор по умолчанию».

Дополнение на Windows-сервер устанавливать не нужно: используется стандартный
Microsoft RDP. RDP-трафик идёт напрямую, без сервера синхронизации.

[Скачать](https://consolecrypt.evsikov.net/download?lang=ru)
· [Руководство RDP](https://consolecrypt.evsikov.net/guide?lang=ru#rdp)
· [Исходный код](https://git.evsikov.net/publics/consolecrypt)

## Доступность

Обмен через буфер поддерживает текст до 64 КиБ. Передача файлов через буфер
не включена; для файлов используйте выбранную папку. Перенаправление папок
пока доступно на компьютерах, с ограничением 256 МиБ на файл. Серверная политика
Windows может запрещать эти возможности. Передача RDP-хостов коллегам пока
недоступна. iOS остаётся предварительной сборкой для Simulator.

## Известное ограничение

При смене подключённой папки в активном RDP-сеансе возможно отключение сеанса.
Если это произошло, подключитесь заново и выберите нужную папку. Исправление
готовится для следующей версии.

## English

**Added support for RDP connections.** Save SSH and RDP connections in Hosts,
switch between up to four remote desktop tabs, and explicitly enable text
clipboard or access to one selected local folder. Folder access is read-only
by default; writing requires a separate opt-in. No IronRDP add-on is needed
on Windows, and the sync server does not relay RDP traffic.

With clipboard access enabled, Ctrl+V (⌘V on macOS) sends and pastes local text
into the active RDP session without a separate click on Send.

True fullscreen fills the display. Move to the top edge to reveal the connection
bar, switch sessions, minimize the window, restore its size or disconnect.
Ctrl+Alt+Home also reveals the bar.

The SSH terminal on Windows also accepts Shift+Insert for pasting, alongside
Ctrl+V, with the same review for multiline text.

The [guide](https://consolecrypt.evsikov.net/guide?lang=en#rdp) explains setup,
certificate verification and permissions. Clipboard supports text up to 64 KiB;
folder redirection is available on desktop platforms with a 256 MiB per-file
limit. Sharing RDP hosts with colleagues is not yet available.

Known limitation: changing the redirected folder during an active RDP session may disconnect it. Reconnect and select the intended folder if this occurs. A fix is planned for a later version.
