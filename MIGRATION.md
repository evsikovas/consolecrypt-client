# Source migration / Перенос исходников

This repository was extracted from the published ConsoleCrypt GitLab main history on 2026-10-04.
The original repository remains operational: https://git.evsikov.net/publics/consolecrypt.
GitLab CI, production deployment, DNS, analytics, signing and update delivery have not been switched to GitHub.
There are no active GitHub Actions workflows in this migration.

Перенесены необходимые исходники и публичная документация. Внутренние ТЗ, планы,
локальные настройки, секреты и рабочие данные не включены. История отфильтрована:
идентификаторы коммитов отличаются от монорепозитория, авторство сохранено.
Теги прежних выпусков сохраняют свою исходную структуру зависимостей для сборки;
актуальная структура разделённых проектов описана в README.

Client release 0.3.1 binaries are byte-for-byte copies of the published GitLab release,
built from original source commit `02ba6a8a28c04741b80ce0086f8411a6cfa08ff4`.
This migration does not announce a new application update or change installed clients.
Historical license notices remain applicable to their original versions.
