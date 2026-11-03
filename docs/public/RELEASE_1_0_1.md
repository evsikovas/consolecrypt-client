# ConsoleCrypt 1.0.1

## Русский

В этом выпуске усиливаем защиту приложения и делаем заметки к паролям удобнее.

- **Пароли:** заметки в открытой карточке показываются обычным текстом.
  Пароли остаются скрытыми, база KDBX сохраняет шифрование.
- **SFTP:** загрузка сначала записывается в отдельный файл, затем атомарно
  переносится на место. Проверки путей защищают от выхода за выбранную папку
  через симлинки и подмену каталогов; прерванная загрузка не портит старый файл.
- **SSH:** обновлены russh и интеграция Pageant; RSA-подписи дополнительно
  используют blinding с системной случайностью. Для новых ключей рекомендуем
  Ed25519; upstream-ограничение RSA остаётся описанным в SECURITY.md.
- **Linux и зависимости:** перенесено исправление небезопасного итератора GLib,
  обновлены нативные зависимости и заменены несколько неподдерживаемых компонентов.
- **Проверки:** расширена проверка Cargo-графов, включая Flutter-плагины,
  добавлены регрессии для файловых гонок, RSA, GLib и D-Bus-совместимости.

Сервер 0.2.5 отдельно исправляет удержание лимита Argon2 при отмене запроса
и ограничивает потоковый ответ OIDC. Серверные тесты с PostgreSQL проверяют
изоляцию хранилищ и отклонение чужих recovery-данных.

Работа над безопасностью продолжается: исправления и автоматические проверки
снижают риски, но не заменяют защиту разблокированного устройства.

Пакеты: Windows, macOS, Linux DEB/RPM, Android и iOS Simulator для Mac.
Simulator не устанавливается на iPhone.

## English

This update strengthens application security and makes password notes easier to read.

- **Passwords:** notes are visible as plain text in an open card. Passwords
  remain masked and the KDBX database stays encrypted.
- **SFTP:** downloads use private staging files and atomic commits. Capability
  checks prevent symlink/directory-swap escapes; interrupted downloads preserve
  an existing destination file.
- **SSH:** updated russh and Pageant integration; RSA signing uses additional
  blinding with system randomness. Prefer Ed25519 for new keys. The upstream
  RSA advisory remains documented in SECURITY.md.
- **Linux and dependencies:** backported the GLib iterator security fix and
  replaced several unmaintained native components.
- **Checks:** expanded Cargo auditing to native Flutter plugins and added
  filesystem-race, RSA, GLib and D-Bus compatibility regressions.

Server 0.2.5 separately fixes cancellation handling for the Argon2 concurrency
limit and bounds streamed OIDC responses. PostgreSQL-backed regressions verify
vault isolation and rejection of another account's recovery envelopes.

Security work continues. These fixes and checks reduce risk; they do not
protect a compromised unlocked device.

Packages target Windows, macOS, Linux DEB/RPM, Android and iOS Simulator on Mac.
The Simulator archive is not an iPhone installer.
