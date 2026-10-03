# ConsoleCrypt 0.3.0 · SSH и удалённые рабочие столы

**Добавлена возможность подключиться к RDP.**

Windows-компьютеры теперь можно держать рядом с SSH-серверами в одном списке
хостов. Выберите тип подключения при создании хоста — форма покажет подходящие
поля. Сохранённые подключения синхронизируются через ваше зашифрованное хранилище.

- **Рабочие столы во вкладках.** До четырёх параллельных RDP-сеансов, панель
  вкладок в стиле терминала и независимое закрытие подключений.
- **Обмен текстом.** Явные действия отправки и получения Unicode-текста;
  разрешение включается отдельно для каждого сеанса.
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

## English

**Added support for RDP connections.** Save SSH and RDP connections in Hosts,
switch between up to four remote desktop tabs, and explicitly enable text
clipboard or access to one selected local folder. Folder access is read-only
by default; writing requires a separate opt-in. No IronRDP add-on is needed
on Windows, and the sync server does not relay RDP traffic.

The SSH terminal on Windows also accepts Shift+Insert for pasting, alongside
Ctrl+V, with the same review for multiline text.

The [guide](https://consolecrypt.evsikov.net/guide?lang=en#rdp) explains setup,
certificate verification and permissions. Clipboard supports text up to 64 KiB;
folder redirection is available on desktop platforms with a 256 MiB per-file
limit. Sharing RDP hosts with colleagues is not yet available.
