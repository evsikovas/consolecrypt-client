# Выпуск версии / Releasing

Сборки выполняет [GitHub Actions](../../.github/workflows/client.yml) в репозитории
`evsikovas/consolecrypt-client`. [Раннеры и нумерация](GITHUB_ACTIONS.md).
Исходники и lock-файлы хранятся в Git; установщики — во вложениях релиза.
Не добавляйте `dist/`, токены, Android keystore или приватные ключи подписи.

1. Поднимите версию через `python3 client/scripts/bump-version.py --part patch`
   (для функционального выпуска используйте `--part minor`). Создайте
   `docs/public/RELEASE_X_Y_Z.md` с описанием изменений и ограничений.
2. Проверьте изменения и отправьте коммит в `main`. Push ветки не собирает пакеты.
   Перед тегом прогоните live-тесты, которых нет в CI (`#[ignore]`):

   ```sh
   python3 client/scripts/live-stand.py list
   python3 client/scripts/live-stand.py sharing
   python3 client/scripts/live-stand.py fingerprint --host <windows-host>
   python3 client/scripts/live-stand.py rdp --host <windows-host> --user <user> --certificate-sha256 <hex>
   ```

   `sharing` поднимает одноразовый PostgreSQL 16 в Docker и соседний
   `consolecrypt-server` на loopback со всеми флагами общего доступа и FileMailer,
   запускает тесты sharing и enrollment и всё удаляет. `rdp` пишет приватный
   конфиг (права 600) для Windows-хоста, на котором владелец разрешил тесты;
   пароль берётся из `CC_RDP_LIVE_PASSWORD` или `~/.config/consolecrypt-test/rdp.env`
   и не печатается, сертификат закрепляется по SHA-256. Kubernetes и Linux
   Secret Service проверяет Linux-задание CI; тесты с системными запросами
   (связка ключей, Secure Enclave, Windows Hello) запускаются вручную.
3. Создайте аннотированный тег на этом коммите и отправьте его:

   ```sh
   git tag -a vX.Y.Z -m 'ConsoleCrypt X.Y.Z'
   git push origin vX.Y.Z
   ```

4. **Actions → Client tag release** проверит совпадение версии и происхождение
   коммита из `main` и проверку зависимостей (`cargo deny`, `osv-scanner`; см.
   [BUILDING.md](BUILDING.md#проверка-зависимостей)), затем соберёт macOS,
   Windows, Linux и Android (iOS не собирается — сборки отложены). Перед тегом закройте открытый issue
   «Уязвимые зависимости»: обновите пакет или оформите исключение с датой
   пересмотра. Локально: `python3 client/scripts/dependency-audit.py check`.
   Номера сборок выдаются отдельно каждой платформе; все сохраняют одну `X.Y.Z`.
5. После успеха всех платформ publisher проверит `ConsoleCrypt.version`, соберёт
   шесть установочных файлов (DMG, EXE, ZIP, DEB, RPM, APK) и
   `SHA256SUMS-X.Y.Z.txt`, загрузит их в draft и опубликует GitHub Release.
   Существующий релиз не перезаписывается. Если загрузка оборвалась после создания
   draft, проверьте его вручную перед повторным запуском; опубликованные файлы
   и теги остаются неизменяемыми, исправления выпускаются новой версией.
6. Workflow отдельно запустит подписанную ленту на доверенном `main`. Проверьте
   результаты этого workflow и доступность установщиков без авторизации.
   Сайт и сервер выпускаются собственными тегами в отдельных репозиториях;
   публикация клиента сама по себе не меняет production Compose.
7. Перед тем как docs и сайт сошлются на новый выпуск, проверьте его ассеты и
   обновите ссылки (соседние checkout `consolecrypt-docs` и `consolecrypt-site`):

   ```sh
   python3 client/scripts/verify-client-release.py --tag vX.Y.Z --download > verified-downloads.json
   python3 client/scripts/update-release-docs.py verified-downloads.json --docs ../consolecrypt-docs --site ../consolecrypt-site
   ```

   Первый скрипт проверяет опубликованный релиз: по одному файлу каждого вида,
   один номер сборки на платформу, размеры, пути и digest GitHub против
   `SHA256SUMS`, допустимые ленты; с `--download` скачивает установщики и
   сверяет SHA-256. Второй переписывает в README docs заголовок, ссылки и
   таблицу сборок, а на сайте — `download.html`, `index.html` и `assets/en.js`;
   если после правки где-то осталась прежняя версия, он ничего не записывает.
   `--check` только сообщает, что изменится. Затем `npm test` на сайте; версия
   сайта в `package.json` и `releases/0.1.N.md` правятся вручную.

## Переход с прежнего сервера обновлений

Клиенты до 0.3.2 доверяют только `git.evsikov.net` и
`updates.consolecrypt.evsikov.net`, включая каждый redirect. Старый `stable.json`
должен указывать на настоящие файлы под старым доверенным доменом, с прежней
подписью. Redirect на GitHub или новый домен не обновит старый клиент.
После установки 0.3.2 используется `https://updates.consolecrypt.dev/stable.json`
и тот же публичный ключ.

Не меняйте сохранённые URL синхронизации в профилях: они входят в локальную
привязку доверия совместного доступа. Поддерживайте старый API-домен как alias
того же экземпляра сервера и одной базы; обычный redirect или второй независимый
пустой сервер не заменяют такую совместимость.

## English

Keep the tested baseline on `main`. Update the source version and release notes,
push the commit, then push its matching stable `vX.Y.Z` tag. Only the tag builds
native packages, and only after the hosted dependency audit (`cargo deny`,
`osv-scanner`) passes; fix or document any open "Уязвимые зависимости" issue first. The workflow publishes a complete GitHub Release after all four
platforms (macOS, Windows, Linux, Android; no iOS) pass and dispatches signed update metadata on trusted `main`. Website
and server deployment are independent tag releases. Preserve signing identities,
app IDs and the Ed25519 update trust anchor. Old clients still require installer
bytes on their previously trusted domains.

Before the tag, run the live tests CI cannot (`#[ignore]`) with
`client/scripts/live-stand.py`: `sharing` starts a disposable PostgreSQL 16 in
Docker and the sibling `consolecrypt-server` on loopback with every sharing flag
and a FileMailer, runs the sharing and enrollment tests and removes everything;
`rdp` writes a private (mode 600) config for an owner-authorized Windows host
pinned by certificate SHA-256 (`fingerprint` reads it) and runs the RDP tests.

Before the docs and the website point at a release, run
`verify-client-release.py --tag vX.Y.Z --download > verified-downloads.json` (one
file of each kind, one build per platform, sizes, paths and GitHub digests against
`SHA256SUMS`, then the downloaded SHA-256) and
`update-release-docs.py verified-downloads.json --docs ../consolecrypt-docs --site ../consolecrypt-site`
(the docs README build table and the website download links; it writes nothing if
the previous version would remain anywhere). Run `npm test` in the site afterwards.
