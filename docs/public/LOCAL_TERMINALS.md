# Локальные терминалы / Local terminals — 0.5

## Русский

В серии 0.5 локальная оболочка открывается рядом с SSH-сессиями в разделе
«Терминал». Это процесс на вашем компьютере с правами вашего пользователя.
Сервер синхронизации и SSH-подключение для него не нужны.

1. Откройте «Терминал» и нажмите кнопку новой вкладки либо `Cmd+T` на macOS /
   `Ctrl+T` на Windows. Общая кнопка «+» приложения по-прежнему выбирает
   сохранённые SSH/RDP-подключения.
2. В группе «Локальные оболочки» выберите **zsh** на macOS либо
   **Windows PowerShell**, **Command Prompt** или установленный **PowerShell 7**
   на Windows. Ниже остаются сохранённые SSH-хосты.
3. На Windows каждый установленный дистрибутив **WSL** показан отдельно.
   Можно одновременно открыть Ubuntu, Debian и несколько вкладок одной оболочки.
   ConsoleCrypt не устанавливает WSL и дистрибутивы автоматически.
4. Проверьте название вкладки: локальная оболочка подписана «Этот компьютер»,
   а WSL сохраняет имя дистрибутива. Ввод, изменение размера, управляющие клавиши
   и интерактивные программы работают через PTY (ConPTY на Windows).
5. Закрытие вкладки завершает её процесс. «Перезапустить оболочку» открывает
   новую сессию; это не продолжение старой программы. Блокировка хранилища,
   смена профиля и выход закрывают локальные терминалы.

Закрытие завершает собственную оболочку и её текущую foreground-группу.
Отсоединённые фоновые процессы (например, запущенные через `nohup`) отдельно
не разыскиваются; весь WSL-дистрибутив не останавливается.

Для локальной вкладки нет действия SFTP: SFTP работает через SSH-хост.
Локальные оболочки не создают фиктивные записи хостов и не синхронизируются как
SSH-подключения. В 0.5 они доступны на macOS и Windows; Linux, Android и iOS
продолжают использовать существующие удалённые подключения.

### ИИ и сниппеты

Палитра и чат получают тип выбранной оболочки: zsh, PowerShell, cmd либо
POSIX shell для WSL. Локальный терминал не получает выдуманный адрес SSH-хоста.
Текст вывода передаётся в ИИ только при явном выборе контекста; проверьте его
перед отправкой выбранному провайдеру.

Для локальной вкладки доступно **«Вставить» без Enter**. Кнопка автоматического
исполнения AI-команд и сниппетов отключена; массовый запуск предназначен для SSH.
Проверьте вставленный текст и, если хотите выполнить его, нажмите Enter сами.
Обычный ввод в терминале не ограничен этим правилом.

### Обновление macOS

Для обычной локальной zsh-сессии вне контейнера пакет 0.5 работает без App Sandbox;
текущая сборка подписывается ad-hoc и не нотарифицирована. Hardened Runtime
включается только при сборке с Developer ID; это не свойство текущего ad-hoc-пакета. При обновлении приложение ищет прежние
данные профиля и локального ИИ в контейнере 0.4.1 и использует их на месте.
Не удаляйте контейнер, vault, файлы модели или системную связку ключей.
Если оба каталога содержат данные либо прежний каталог недоступен, приложение
показывает ошибку до создания профиля: оно не объединяет разные хранилища и не
подменяет прежний профиль пустым. Сохраните резервную копию перед обновлением.

### Руководство и проверка

[Глава со снимками RU](https://consolecrypt.dev/guide?lang=ru#local-terminals) ·
[English guide](https://consolecrypt.dev/guide?lang=en#local-terminals).
Снимки 0.5 — настоящие Flutter-виджеты с синтетическими оболочками и текстом.
WSL-экран демонстрирует Windows-вариант интерфейса в widget-тесте, а не запуск
Windows или WSL. Проверки установленного пакета отмечаются отдельно в
[заметках выпуска](RELEASE_0_5_1.md).

## English

The 0.5 series adds local shells alongside SSH tabs. Open **Terminal** and
use its new-tab button, `Cmd+T` on macOS or `Ctrl+T` on Windows. Choose **zsh** on
macOS, or **Windows PowerShell**, **Command Prompt**, installed **PowerShell 7**
and individual **WSL** distributions on Windows. The app's global “+” still opens
the saved SSH/RDP connection picker. ConsoleCrypt does not install WSL itself.

Each local process runs as your current OS user through a PTY/Windows ConPTY.
Tabs support interactive input, control keys, resizing and independent sessions.
The label identifies this computer or the exact WSL distribution. Closing a tab,
locking the vault, switching profiles or quitting closes the local process;
Restart shell starts a new session. Closing terminates the owned shell and its
current foreground process group; detached/background processes such as `nohup`
are not searched for, and the entire WSL distribution is not stopped.
SFTP remains an SSH action. No synthetic SSH
host is saved or synced. Linux, Android and iOS do not offer local shells in 0.5.

AI receives the explicit shell dialect and only context you choose to attach.
For local tabs, AI commands and snippets offer **Insert without Enter**, while
automatic execution is disabled. Review the text, then press Enter yourself if
you want to run it. Ordinary terminal input works normally; multi-run targets
SSH sessions only.

The 0.5 macOS package removes App Sandbox to run ordinary local shells. The
current build is ad-hoc signed and not notarized; Hardened Runtime applies only
to the Developer ID signing path. It reuses populated 0.4.1 container storage in place,
including profiles and local models. Keep that container, vault and Keychain.
If both old and new storage locations contain data or old storage cannot be
read, startup stops with an explanation rather than silently creating an empty
profile. Save a backup before upgrading.

Guide screenshots use actual Flutter widgets with synthetic profiles and output.
The Windows/WSL picker is a widget-test rendering, not proof of a native Windows
or WSL run. Installed-package checks are reported separately in the release notes.
