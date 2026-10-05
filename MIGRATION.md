# Source migration / Перенос исходников

This repository was extracted from the published ConsoleCrypt GitLab main history on 2026-10-04.
The original repository remains operational: https://git.evsikov.net/publics/consolecrypt.
Version 0.3.2 introduces native builds through GitHub Actions and the public
consolecrypt.dev addresses. Build artifacts and production publication remain
separate steps; see [GITHUB_ACTIONS.md](docs/public/GITHUB_ACTIONS.md).

Перенесены необходимые исходники и публичная документация. Внутренние ТЗ, планы,
локальные настройки, секреты и рабочие данные не включены. История отфильтрована:
идентификаторы коммитов отличаются от монорепозитория, авторство сохранено.
Теги прежних выпусков сохраняют свою исходную структуру зависимостей для сборки;
актуальная структура разделённых проектов описана в README.

Client release 0.3.1 binaries are byte-for-byte copies of the published GitLab release,
built from original source commit `02ba6a8a28c04741b80ce0086f8411a6cfa08ff4`.
The 0.3.1 source migration did not announce an update or modify installed clients.
The 0.3.2 transition retains native app identities and the original signed-update
public key. Existing profiles retain their saved URLs and sharing trust pins.
Old clients need actual installer bytes on their previously trusted update host;
a redirect to GitHub or a new hostname is incompatible with their allowlist.
Keep the old API origin as an alias of the same server instance and database.
Historical license notices remain applicable to their original versions.
