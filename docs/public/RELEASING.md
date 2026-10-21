# Выпуск версии / Releasing

Сборки выполняет [GitHub Actions](../../.github/workflows/client.yml) в репозитории
`evsikovas/consolecrypt-client`. [Раннеры и нумерация](GITHUB_ACTIONS.md).
Исходники и lock-файлы хранятся в Git; установщики — во вложениях релиза.
Не добавляйте `dist/`, токены, Android keystore или приватные ключи подписи.

1. Поднимите версию через `python3 client/scripts/bump-version.py --part patch`
   (для функционального выпуска используйте `--part minor`). Создайте
   `docs/public/RELEASE_X_Y_Z.md` с описанием изменений и ограничений.
2. Проверьте изменения и отправьте коммит в `main`. Push ветки не собирает пакеты.
3. Создайте аннотированный тег на этом коммите и отправьте его:

   ```sh
   git tag -a vX.Y.Z -m 'ConsoleCrypt X.Y.Z'
   git push origin vX.Y.Z
   ```

4. **Actions → Client tag release** проверит совпадение версии и происхождение
   коммита из `main` и проверку зависимостей (`cargo deny`, `osv-scanner`; см.
   [BUILDING.md](BUILDING.md#проверка-зависимостей)), затем соберёт macOS,
   Windows, Linux, Android и iOS Simulator. Перед тегом закройте открытый issue
   «Уязвимые зависимости»: обновите пакет или оформите исключение с датой
   пересмотра. Локально: `python3 client/scripts/dependency-audit.py check`.
   Номера сборок выдаются отдельно каждой платформе; все сохраняют одну `X.Y.Z`.
5. После успеха всех платформ publisher проверит `ConsoleCrypt.version`, соберёт
   семь установочных файлов и `SHA256SUMS-X.Y.Z.txt`, загрузит их в draft и
   опубликует GitHub Release. iOS Simulator не устанавливается на iPhone.
   Существующий релиз не перезаписывается. Если загрузка оборвалась после создания
   draft, проверьте его вручную перед повторным запуском; опубликованные файлы
   и теги остаются неизменяемыми, исправления выпускаются новой версией.
6. Workflow отдельно запустит подписанную ленту на доверенном `main`. Проверьте
   результаты этого workflow и доступность установщиков без авторизации.
   Сайт и сервер выпускаются собственными тегами в отдельных репозиториях;
   публикация клиента сама по себе не меняет production Compose.

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
`osv-scanner`) passes; fix or document any open "Уязвимые зависимости" issue first. The workflow publishes a complete GitHub Release after all five
platforms pass and dispatches signed update metadata on trusted `main`. Website
and server deployment are independent tag releases. Preserve signing identities,
app IDs and the Ed25519 update trust anchor. Old clients still require installer
bytes on their previously trusted domains.
