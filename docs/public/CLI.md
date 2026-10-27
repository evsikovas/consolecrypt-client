# Командная строка: consolecrypt-cli

`consolecrypt-cli` — консольный клиент ConsoleCrypt. Он входит в установщики для
macOS, Windows и Linux (с версии 0.9.14) и работает с теми же профилями, что и
приложение: хранилище, ключи и учётные данные не копируются.

## Установка

**Настройки → Командная строка → «Установить команды CLI»**:

- **macOS** — ссылка `/usr/local/bin/consolecrypt-cli` на файл внутри
  `ConsoleCrypt.app`. Если папка принадлежит системе, macOS спросит пароль
  администратора.
- **Windows** — папка `bin` программы добавляется в PATH пользователя.
- **Linux** (DEB, RPM) — ссылка `~/.local/bin/consolecrypt-cli` на
  `/opt/consolecrypt/consolecrypt-cli`. Если `~/.local/bin` нет в PATH, раздел
  подскажет это; добавьте папку в настройки своей оболочки.

Откройте новое окно терминала. «Удалить команды CLI» убирает только ссылку или
запись в PATH, созданные ConsoleCrypt; чужой файл с тем же именем не трогается.
На телефонах командной строки нет.

## Команды

```text
consolecrypt-cli --help                       # все команды
consolecrypt-cli profiles                     # профили; use / rm
consolecrypt-cli unlock-check                 # проверить разблокировку хранилища
consolecrypt-cli host list | show | add | rm  # хосты
consolecrypt-cli cred list | add-password | gen-key | import-key | rm
consolecrypt-cli ssh HOST                     # интерактивная сессия
consolecrypt-cli ssh HOST -- uptime           # выполнить команду
consolecrypt-cli tunnel list | add | start    # пробросы портов
consolecrypt-cli sftp ls | get | put          # файлы
consolecrypt-cli sync now | status            # синхронизация
consolecrypt-cli device list | code | ...     # устройства и их одобрение
consolecrypt-cli backup ...                   # зашифрованные копии .ccbackup
consolecrypt-cli recovery-kit                 # комплект восстановления
consolecrypt-cli passphrase change | reset    # парольная фраза хранилища
consolecrypt-cli ai search | ask | ...        # ИИ: поиск, вопросы, команды
```

Общие флаги: `--profile` (по умолчанию — последний профиль), `--json` (вывод
для скриптов), `--offline` (без синхронизации), `-v` (подробный журнал; секреты
в него не попадают). Парольная фраза и другие секреты берутся из окна запроса
или из переменных окружения для автоматизации: `CC_PASSPHRASE`, `CC_PASSWORD`,
`CC_SECRET`, `CC_KEY_PASSPHRASE`, `CC_SSH_PASSWORD`, `CC_RECOVERY_KEY`.
Обычный `http://` разрешён только для адресов этого компьютера; `--insecure-http`
— только для доверенной локальной сети.

`consolecrypt-cli --version` показывает версию приложения, с которым он
поставляется. На macOS CLI находит профили там же, где приложение (в том числе в
папке прежних сборок внутри `~/Library/Containers`); другую папку задаёт
`--data-dir` или `CONSOLECRYPT_DATA_DIR`.

Приложение и CLI могут работать одновременно с одним профилем. Синхронизация
сводит их изменения, но одну и ту же запись лучше не править в двух местах сразу.

## English

`consolecrypt-cli` is the ConsoleCrypt command-line client. From 0.9.14 it ships
with the macOS, Windows and Linux installers and works with the same profiles as
the app; the vault, keys and credentials are not copied.

**Settings → Command line → Install CLI commands** links it into
`/usr/local/bin` on macOS (asking for the administrator password when needed),
adds the installation's `bin` folder to the user PATH on Windows, and links it
into `~/.local/bin` on Linux (the section tells you when that folder is not on
PATH). Open a new terminal afterwards. Remove CLI commands removes only the link
or PATH entry ConsoleCrypt created; a foreign file of the same name is left
alone. Phones have no command line.

`consolecrypt-cli --help` lists the commands: profiles, unlock-check, host,
cred, ssh (interactive, or `-- COMMAND`), tunnel, sftp, sync, device, backup,
recovery-kit, passphrase and ai. Global flags: `--profile`, `--json`,
`--offline`, `-v`. Secrets come from a prompt or, for automation, from
`CC_PASSPHRASE`, `CC_PASSWORD`, `CC_SECRET`, `CC_KEY_PASSPHRASE`,
`CC_SSH_PASSWORD` and `CC_RECOVERY_KEY`. Plain `http://` is allowed only for
this computer; `--insecure-http` is for a trusted local network only. The app
and the CLI can use one profile at the same time; avoid editing the same item in
both at once. `consolecrypt-cli --version` shows the app release it ships with; on
macOS the CLI finds the profiles where the app keeps them (including the folder of
earlier builds inside `~/Library/Containers`); `--data-dir` or
`CONSOLECRYPT_DATA_DIR` chooses another folder.
