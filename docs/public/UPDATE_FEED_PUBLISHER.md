# Signed update-feed renewal

`.github/workflows/update-feeds.yml` runs on GitHub-hosted Ubuntu, independently
of native builds. It checks daily and supports a manual run from `main`. A
published stable-release event only dispatches a separate run on `main`; the
release-tag job never receives the signing secret. There are no PR triggers or
self-hosted signing jobs.

The repository secret **UPDATE_SIGNING_KEY** must contain the existing Ed25519
PKCS#8 PEM private key. The publisher checks it against the public key already
embedded in installed clients:

```text
aK3R5vXwJ9eeqQ1iYah9fzBAIHOeKNejoRdI8weXUYo=
```

This is a trust-anchor check, not a request to create a new key. An operator
must bootstrap the first release with an already signed `stable.json` and
`legacy-stable.json`. The publisher refuses to infer trust from unsigned release
metadata alone. Keep the offline signing-key backup outside Git and the VPS.
The workflow has no server SSH key or Timeweb token. The signing key is loaded
from the environment into memory, never written to a file, artifact, log or
installer. Tests generate ephemeral keys at runtime and do not read the secret.

## Publication checks

The publisher:

1. Reads the latest non-draft, non-prerelease `vX.Y.Z` release in
   `evsikovas/consolecrypt-client`, and refuses a latest tag below another stable
   release. At most 99 historical releases and 32 assets per release are accepted;
   larger histories require an operator to review and extend the bound.
2. Reads `SHA256SUMS-X.Y.Z.txt`, requires exactly one Windows x64 EXE, macOS
   universal DMG and Android ARM64 APK, and checks their names, build numbers,
   sizes and available GitHub SHA-256 digests.
3. Verifies an existing signed feed with the embedded public anchor. It rejects
   version/build downgrades and changes to the identities of an already signed
   release version. A new release needs a new version and greater build numbers.
4. Leaves a complete current pair unchanged while both have at least 14 days
   remaining. Otherwise it streams and hashes all three installers, enforcing
   a 500 MiB bound per file. It follows only the exact repository's GitHub asset
   paths and its `release-assets.githubusercontent.com` CDN namespace; the API
   token never accompanies a public download.
5. Rechecks release metadata for races, then signs a 90-day pair and uploads it
   as public release assets using the short-lived `GITHUB_TOKEN` with
   `contents: write`.

`stable.json` uses `https://updates.consolecrypt.dev/releases/VERSION/NAME`.
`legacy-stable.json` uses `https://updates.consolecrypt.evsikov.net/releases/VERSION/NAME`.
The same version, build numbers, byte sizes, SHA-256 values and signing identity
are used in both. The legacy endpoint serves the latter document as its
`/stable.json`. Keeping old URLs alive is necessary for installed clients that
do not trust GitHub or the new domain.

## Serving the public files

The VPS needs only the **public** signed release assets and installers. Its
separate mirror must verify signatures, identities and hashes, retain the last
good files on verification or download failure, and atomically replace each feed
only after both feeds and all referenced installers have passed verification. Do not
put the signing private key on that server. The publisher does not deploy the
website or modify account data.

GitHub does not provide an atomic replacement of two release assets. All checks
finish before replacement starts; on an upload failure the publisher attempts
to restore the preceding feed from memory. A later run can repair a pair with
one surviving valid feed. The serving mirror must not delete its working files
because an asset is temporarily missing or a run fails. An expired signed
baseline is accepted for renewal, but installed clients still reject an expired
feed until renewal reaches the mirror.

This design requires editable release assets. If GitHub immutable releases are
enabled, the publisher fails closed; move the two feed assets to a separately
reviewed mutable distribution channel before enabling that setting.

Run deterministic publisher tests without any production credentials:

```sh
node --test client/scripts/test_publish_update_feeds.mjs
```

## Restoring a disabled schedule

GitHub automatically disables scheduled workflows in a **public repository after
60 days without repository activity**. Check the workflow when returning to an
inactive repository; a configured daily schedule is not a promise of indefinite
execution. [GitHub: disabling and enabling workflows](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/disable-and-enable-workflows).

Re-enable this existing workflow and request a check from the trusted default
branch:

```sh
gh workflow enable update-feeds.yml --repo evsikovas/consolecrypt-client
gh workflow run update-feeds.yml --repo evsikovas/consolecrypt-client --ref main
gh run list --repo evsikovas/consolecrypt-client --workflow update-feeds.yml --limit 3
```

Confirm the run succeeds and inspect the public feed expiry after the VPS mirror
runs. A still-valid pair with at least 14 days remaining is intentionally left
unchanged. An expired, correctly signed baseline can be renewed, but clients
cannot accept it until the renewed feed reaches the serving mirror. No keepalive
commits or separate scheduling service are required by this workflow.

## По-русски

Workflow **Signed update feeds** проверяет релиз каждый день. Подпись выполняется
только кодом из `main` на GitHub-hosted runner. Исходный приватный ключ хранится
в GitHub secret `UPDATE_SIGNING_KEY`, используется только в памяти и не попадает
на VPS. Существующие клиенты продолжают проверять прежний публичный ключ.

При остатке срока менее 14 дней workflow заново проверяет установщики и выпускает
новую подписанную пару со сроком 90 дней. Публикация релиза также запускает проверку.
Первую доверенную пару публикует оператор; без неё автоматическая подпись запрещена.
Проверенный публичный mirror на сервере сохраняет прежние рабочие файлы при сбое,
а старый домен обновлений продолжает обслуживать установленные версии клиента.

В публичном репозитории GitHub отключает расписание после 60 дней без активности.
После такого перерыва включите существующий workflow командой `gh workflow enable`
и запустите ручную проверку из `main`, как показано выше. Проверьте результат и
срок действия публичного feed после обновления mirror; просроченный документ
клиент принимает только после его повторной подписи и доставки.
