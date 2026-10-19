# ConsoleCrypt 0.5.2

## Русский

Первый выпуск серии 0.5: локальные терминалы, расширенный каталог
моделей и сохранение загруженного локального ИИ при обновлении.

- **Локальные терминалы на macOS и Windows:** zsh, PowerShell, cmd и отдельная
  вкладка для каждого установленного дистрибутива WSL. Их можно держать рядом
  с SSH-сессиями. Поддерживаются интерактивный ввод, изменение размера,
  управляющие клавиши и завершение процесса при закрытии вкладки или блокировке.
- **Явная цель для ИИ:** локальная оболочка передаёт свой диалект без фиктивного
  SSH-хоста. Для локальных AI-команд и сниппетов доступна вставка без Enter;
  исполнение после проверки остаётся ручным. SFTP и массовый запуск — для SSH.
- **Три локальные модели:** лёгкая Qwen2.5-Coder 1.5B, средняя Qwen3.5 4B и мощная
  Qwen3.5 9B. Настройки показывают объём загрузки, ориентир по ОЗУ, источники и
  лицензии. Звезда рекомендует вариант по общей памяти компьютера, не меняя
  ваш выбор автоматически.
- **Сохранение локального ИИ при обновлении:** установка движка отделена от
  GGUF-файлов. Проверенная модель переиспользуется; смена модели не удаляет
  остальные. Удаление выбранной модели не удаляет другие модели и движок.
- **macOS:** запуск обычной zsh требует выхода из App Sandbox. Прежние данные
  контейнера используются на месте; текущий пакет подписан ad-hoc и не
  нотарифицирован; при конфликте двух хранилищ или ошибке
  доступа запуск останавливается с объяснением. Vault и Keychain удалять не надо.
- **Windows:** устранено зависание при закрытии локального терминала до завершения
  стартового обмена ConPTY; PowerShell и cmd проверены на настоящем Windows-раннере.
- Исправлены порядок восстановления терминального буфера после пропуска событий
  и контраст мобильной RDP-панели в тёмной теме.

Подробности: [локальные терминалы](https://github.com/evsikovas/consolecrypt-client/blob/v0.5.2/docs/public/LOCAL_TERMINALS.md), [локальный ИИ](https://github.com/evsikovas/consolecrypt-client/blob/v0.5.2/docs/public/LOCAL_AI.md)
и [обновления](https://github.com/evsikovas/consolecrypt-client/blob/v0.5.2/docs/public/UPDATES.md). Локальные оболочки и встроенный локальный ИИ доступны
на macOS/Windows; Linux и мобильные платформы сохраняют существующие возможности.
iOS-пакет предназначен для Simulator, а не для установки на физический iPhone.

### Проверка и происхождение снимков

Перед публикацией проверяются Rust PTY/lifecycle, Flutter-модели и контроллеры,
смешанные вкладки, выбор WSL, контекст ИИ, повторная установка моделей и миграция
каталогов. RU/EN снимки воспроизводят настоящий интерфейс с демонстрационными
данными. Они не доказывают запуск WSL или генерацию всеми тремя моделями на
пользовательском компьютере. Нативные сборки и проверки пакетов выполняются
отдельно в workflow релизного тега.

Ссылки на загрузки обновляются после появления проверенных файлов релиза.
Перед обновлением сохраните резервную копию и комплект восстановления;
не удаляйте данные приложения, модели или системную связку ключей.

## English

The first release in the 0.5 series, with local terminals, an expanded model catalog
and preservation of downloaded local AI models across application updates.

0.5 adds device-local zsh on macOS and PowerShell, cmd and individual WSL
distributions on Windows alongside SSH tabs. Local sessions use PTY/ConPTY,
handle resizing and control keys, and stop when their tab closes or the vault
locks. AI receives the shell dialect; local AI commands and snippets can be
inserted without Enter, with manual execution after review. SFTP and multi-run
remain SSH features.

Local AI now offers Qwen2.5-Coder 1.5B, Qwen3.5 4B and Qwen3.5 9B with download
sizes, RAM guidance, source/license details and a hardware-based recommendation.
The recommendation never changes selection or the device-only default. Verified
models survive runtime repairs and application updates; changing or removing
one model preserves the others.

The macOS package removes App Sandbox for ordinary local shells. The current
build is ad-hoc signed and not notarized; Hardened Runtime is enabled only by
the Developer ID signing path. Existing container data is reused in place;
conflicting or unreadable storage stops startup instead of creating an empty
profile. Windows local terminals no longer hang when closed before the initial
ConPTY cursor handshake completes; native PowerShell and cmd checks pass.
Terminal resynchronization ordering and dark mobile RDP controls are
also fixed. Local shells and built-in AI are macOS/Windows features; iOS remains
a Simulator preview.

See [local terminals](https://github.com/evsikovas/consolecrypt-client/blob/v0.5.2/docs/public/LOCAL_TERMINALS.md), [local AI](https://github.com/evsikovas/consolecrypt-client/blob/v0.5.2/docs/public/LOCAL_AI.md) and
[updating](https://github.com/evsikovas/consolecrypt-client/blob/v0.5.2/docs/public/UPDATES.md). Screenshots use real widgets and synthetic demo data,
not native WSL or model-inference recordings. Native package checks run
separately in the tag workflow. Download links change only after release assets
are available. Preserve app data, models, vault and Keychain when updating.
