# ConsoleCrypt 0.5.5

## Русский

На телефоне RDP автоматически открывается на весь экран. Постоянные панели
заменены одной плавающей кнопкой: её можно перемещать к краю, а через неё —
открывать клавиатуру, менять режим управления, выбирать сеанс и сворачивать
рабочий стол без отключения.

- **Тачпад по умолчанию на телефоне.** Палец двигает курсор относительно его
  положения. Касание двумя пальцами — правый клик, параллельное движение —
  прокрутка, щипок — масштаб 1–4×, двойное касание двумя пальцами — вписать
  картинку, касание тремя пальцами — клавиатура.
- **Клавиатура поверх рабочего стола.** Картинка не сжимается. Полоса клавиш
  над клавиатурой и отдельный лист предлагают Ctrl, Alt, Win, Shift, стрелки,
  F1–F12 и сочетания. Одно нажатие модификатора действует на следующую клавишу,
  двойное фиксирует его, следующее отпускает.
- **Размер под экран.** Разрешение запрашивается с учётом ориентации и
  безопасной области; масштаб Windows можно выбрать в настройках. Если
  RDP-сервер не поддерживает изменение размера, сеанс продолжает работать
  с прежним разрешением.
- **Локальные настройки управления.** Скорость, ускорение, размер курсора,
  чёткость, ориентация и автоскрытие кнопки. Режим управления для хоста
  запоминается только на этом устройстве.
- **Импорт SSH-хостов.** Кнопка «Импорт» в форме нового SSH-хоста на
  компьютере предлагает адреса из стандартных файлов OpenSSH `known_hosts`.
  Перед сохранением можно выбрать записи. Имена пользователей и пароли
  нужно указать отдельно; доверенные ключи не изменяются. Хешированные
  имена, шаблоны, отозванные ключи и записи центров сертификации пропускаются.
- **Палитра команд.** «Новый локальный терминал» открывает zsh на macOS;
  Windows предлагает PowerShell, cmd и установленные дистрибутивы WSL.
- Исправлено перекрытие системных кнопок окна и боковой панели на macOS,
  а также размещение фильтров хостов при повороте телефона.
- Модель Qwen3.5 9B называется **«Тяжёлая»**. Идентификаторы моделей и
  расположение скачанных файлов не меняются.

На планшете сохраняется обычная раскладка и ручной переход на полный экран.
На Android первый «Назад» раскрывает управление, повторный в течение двух
секунд сворачивает сеанс. «Отключить» требует подтверждения. Блокировка
хранилища закрывает сеансы и их диалоги.

[Управление RDP](RDP.md) · [Локальные терминалы](LOCAL_TERMINALS.md) ·
[Локальный ИИ](LOCAL_AI.md)

Архив iOS предназначен для Simulator на Mac; это не установщик для
физического iPhone. Перенаправление папок в мобильном RDP пока недоступно.

## English

Phone RDP sessions now open fullscreen automatically. One draggable floating
button provides the keyboard, input mode, session switcher and minimize action.
Minimizing keeps the connection open.

- Touchpad mode is the phone default. Move a finger to move the pointer
  relatively; two-finger tap right-clicks, parallel motion scrolls, a pinch
  zooms 1–4×, two-finger double tap fits the desktop, and three-finger tap
  toggles the keyboard.
- The keyboard overlays a stable desktop surface. A key strip and expanded
  sheet provide modifiers, arrows, F1–F12 and shortcuts. Tap a modifier once
  for the next key, twice to lock it, then again to release it.
- Display size follows the phone orientation and safe area when the server
  supports resizing. Pointer speed, acceleration, cursor size, display
  clarity, Windows scale, orientation and button hiding are device settings.
  A host's input mode is remembered only on that device.
- The new SSH-host editor on desktop has an **Import** action for standard
  OpenSSH `known_hosts` files. Preview and choose addresses before saving.
  Supply usernames and credentials separately. Trusted keys are unchanged;
  hashed names, patterns, revoked entries and CA records are skipped.
- **New local terminal** in the command palette opens zsh on macOS or offers
  PowerShell, cmd and installed WSL distributions on Windows.
- macOS window buttons no longer overlap the collapsed sidebar control.
  Host filters also fit a phone's landscape layout.
- The Qwen3.5 9B tier is now named **Heavy**. Model IDs and downloaded-file
  locations are unchanged.

Tablets retain the regular layout and manual fullscreen action. On Android,
Back first opens the controls; another Back within two seconds minimizes.
Disconnect requires confirmation. Vault locking closes sessions and overlays.
The iOS archive remains a Simulator preview, and mobile folder redirection
is not supported.
