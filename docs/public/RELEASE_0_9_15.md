# ConsoleCrypt 0.9.15

## Русский

Выпуск для macOS, Windows, Linux и Android. Сервер синхронизации не меняется: подходит
любой поддерживаемый сервер (0.2.1 и новее).

**Исправлено: блокировка больше не закрывает сессии.**

- После автоматической блокировки хранилища (или блокировки вручную) все открытые
  сессии продолжают работать: SSH-терминалы и локальные терминалы, RDP-сеансы,
  SSH-туннели и сессии Kubernetes с терминалами подов и пробросами портов. Раньше после
  разблокировки они оказывались закрыты.
- Пока хранилище заблокировано, новые подключения не открываются, а RDP-вкладки стоят на
  паузе; после разблокировки всё продолжается с того же места.
- Если за время блокировки изменились настройки подключения к кластеру Kubernetes, его
  сессия закрывается и открывается заново, как и раньше при изменении настроек.
- Сессии по-прежнему закрываются при смене профиля и выходе. SFTP, Docker и подключения к
  базам данных при блокировке закрываются, как и раньше.

**Новое: физический ключ безопасности (YubiKey, FIDO2) — предварительная версия.**

- Хранилище можно открывать USB-ключом безопасности — YubiKey или другим ключом FIDO2 с
  расширением `hmac-secret` — и его PIN-кодом. Включается в **Настройки → Хранилище →
  Физический ключ безопасности**, на экране блокировки появляется кнопка
  «Разблокировать ключом безопасности».
- Работает на Windows 10/11 (PIN и касание спрашивает окно Windows, права администратора
  не нужны) и macOS (PIN спрашивает приложение, затем показывает «Коснитесь ключа»).
  Linux и мобильные платформы пока не поддерживаются.
- Функция необязательная и действует для этого хранилища на этом устройстве: ключ
  запечатывает ключ хранилища в локальный конверт, который не синхронизируется, не
  попадает в резервные копии и на сервер. Парольная фраза и Recovery Key продолжают
  работать. Подробнее — [Ключ безопасности](SECURITY_KEY.md).

**Безопасность.** `libz-sys` 1.1.31 (RUSTSEC-2026-0418: скрипт сборки мог записать
переменные окружения сборки в каталог `target/`). Сборки ConsoleCrypt затронуты не были —
в них использовался `cc` 1.5.1, а утечка возникает с `cc` 1.6.0.

## English

Release for macOS, Windows, Linux and Android. The sync server is unchanged: any supported
server (0.2.1 or later) works.

**Fixed: locking no longer closes sessions.**

- After the vault locks (automatically after idle time or by hand) every open session keeps
  running: SSH and local terminals, RDP sessions, SSH tunnels and Kubernetes sessions with
  their pod terminals and port forwards. Before, they were closed after unlocking.
- While the vault is locked nothing new connects and RDP tabs pause; after unlocking
  everything continues where it was.
- If the connection settings of a Kubernetes cluster changed while locked, its session is
  closed and opened again, as on any settings change.
- Sessions still close when you switch profiles or quit. SFTP, Docker and database
  connections still close on lock.

**New: physical security key (YubiKey, FIDO2) — preview.**

- The vault can be opened with a USB security key — a YubiKey or another FIDO2 key with the
  `hmac-secret` extension — and its PIN. Switch it on in **Settings → Vault → Physical
  security key**; the lock screen then offers **Unlock with security key**.
- Works on Windows 10/11 (Windows asks for the PIN and the touch; no administrator rights)
  and macOS (the app asks for the PIN, then shows "Touch the security key"). Linux and
  mobile are not supported yet.
- Optional, for this vault on this device: the key seals the vault key in a local envelope
  that is never synced, backed up or sent to the server. The passphrase and the Recovery
  Key keep working. See [Security key](SECURITY_KEY.md).

**Security.** `libz-sys` 1.1.31 (RUSTSEC-2026-0418: the build script could write build
environment variables to `target/`). ConsoleCrypt builds were not affected: they used
`cc` 1.5.1, and the leak needs `cc` 1.6.0.
