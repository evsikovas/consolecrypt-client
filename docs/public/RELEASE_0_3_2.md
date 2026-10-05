# ConsoleCrypt 0.3.2

Новый домен и сборки через GitHub Actions.

- Сайт и руководство: [consolecrypt.dev](https://consolecrypt.dev).
- Исходники клиента, сервера и документации — в отдельных репозиториях GitHub.
- Новая подписанная лента обновлений: `updates.consolecrypt.dev`.
- Пример адреса публичного сервера: `https://sync.consolecrypt.dev`.
- Существующие профили, хранилища, ключи и адреса синхронизации сохраняются.
  Для обновления не нужно удалять данные, выходить из аккаунта или пересоздавать профиль.
- macOS, Windows, Linux DEB/RPM, Android ARM64 и iOS Simulator собираются
  на новых GitHub Actions раннерах с возрастающими номерами сборок.

Все основные возможности доступны: SSH-терминалы, RDP с полноэкранным режимом,
обмен текстом и выбранной папкой в RDP на компьютерах, SFTP, сниппеты, локальное
зашифрованное хранилище, синхронизация и совместный доступ. SSH/RDP идут
непосредственно к вашим серверам. Исходный код — AGPL-3.0-only.

Известное ограничение: смена общей папки или её прав во время RDP-сеанса иногда
обрывает подключение. Сохраните удалённую работу перед изменением и подключитесь
снова при необходимости. Android — preview, iOS ZIP предназначен только для
Simulator на Mac и не устанавливается на физический iPhone.

[Скачать](https://consolecrypt.dev/download?lang=ru) ·
[Руководство](https://consolecrypt.dev/guide?lang=ru) ·
[Клиент](https://github.com/evsikovas/consolecrypt-client) ·
[Сервер](https://github.com/evsikovas/consolecrypt-server) ·
[Документация](https://github.com/evsikovas/consolecrypt-docs)

## English

This release moves native builds to GitHub Actions and switches public website,
source and signed update-feed links to the new ConsoleCrypt domain. Existing
profiles keep their server addresses, encrypted vaults, keys and trust settings.
The update-verification key and native application identities are unchanged.

Packages cover macOS Intel/Apple Silicon, Windows x64, Linux x64 DEB/RPM,
Android ARM64 preview and iOS Simulator. SSH, RDP, SFTP, snippets, encrypted
storage, synchronization and selective sharing remain available. Changing a
redirected RDP folder or its permissions may still disconnect the session;
save remote work first and reconnect if needed.

[Downloads](https://consolecrypt.dev/download?lang=en) ·
[Guide](https://consolecrypt.dev/guide?lang=en)
