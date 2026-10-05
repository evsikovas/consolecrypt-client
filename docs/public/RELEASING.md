# Выпуск версии / Releasing

Сборки выполняет [GitHub Actions](../../.github/workflows/client.yml) в репозитории
`evsikovas/consolecrypt-client`. [Раннеры и нумерация](GITHUB_ACTIONS.md).
Исходники и lock-файлы хранятся в Git; установщики — во вложениях релиза.
Не добавляйте `dist/`, токены, Android keystore или приватные ключи подписи.

1. Поднимите версию через `python3 client/scripts/bump-version.py --part patch`.
   Перед первым GitHub выпуском исходный build установлен в 1394, выше всех файлов 0.3.1.
2. Запустите **Actions → Client packages** для проверенного коммита `main`.
   Workflow собирает macOS, Windows, Linux, Android и iOS Simulator; CI artifacts
   сами по себе ничего не публикуют на сайт и в автообновления.
3. Дождитесь всех задач, проверьте пакеты и сверяйте точную версию каждого файла
   с его `ConsoleCrypt.version`. Сохраните SHA исходников и run ID.
4. Соберите каталог файлов с уникальными именами, версиями и `SHA256SUMS-VERSION.txt`.
   iOS Simulator не является установщиком для iPhone.
5. Создайте тег на том же проверенном SHA, затем черновик GitHub Release:

   ```sh
   gh release create v0.3.2 --draft --target VERIFIED_COMMIT \
     --repo evsikovas/consolecrypt-client --title 'ConsoleCrypt 0.3.2' \
     --notes-file docs/public/RELEASE_0_3_2.md
   gh release upload v0.3.2 dist/release/* --repo evsikovas/consolecrypt-client
   ```

6. Опубликуйте релиз после проверки файлов, затем обновите сайт и подписанную
   ленту. Приватный ключ Ed25519 остаётся у оператора; обычным build jobs он
   не нужен. Скачайте опубликованные файлы без авторизации и проверьте SHA-256.

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

Build and test the exact release commit with **Client packages**, then collect
its per-platform artifacts and version receipts. Create a draft GitHub Release
at that SHA, verify packages and checksums, and publish only after acceptance.
Website deployment and the signed update feed remain separate operations.
Keep Android/macOS signing identities, app IDs and the Ed25519 update trust anchor.
Old clients must receive installer bytes from a previously trusted domain;
they reject redirects to new hosts.
