# Ключ безопасности (YubiKey, FIDO2)

## Русский

Хранилище можно открывать физическим USB-ключом безопасности — YubiKey или
другим ключом FIDO2 с расширением `hmac-secret` — и его PIN-кодом. Функция
необязательна и включается отдельно для каждого хранилища на каждом
устройстве. Парольная фраза и Recovery Key продолжают работать: если ключ
потерян или его PIN заблокирован, откройте хранилище парольной фразой.

**Где работает (предварительная версия):** Windows 10/11 (через системный
WebAuthn, PIN спрашивает окно Windows) и macOS (приложение само спрашивает
PIN). Linux и мобильные платформы пока не поддерживаются.

### Как включить

1. Разблокируйте хранилище парольной фразой.
2. **Настройки → Хранилище → Физический ключ безопасности** — включите.
3. Вставьте ключ, введите его FIDO2 PIN-код и дважды коснитесь ключа: первое
   касание регистрирует ключ, второе получает секрет для конверта.

Ключ должен иметь FIDO2 PIN. Если его нет, Windows предложит создать PIN при
первой регистрации; на macOS задайте его в Yubico Authenticator. После восьми
неверных PIN подряд FIDO на ключе блокируется — снять блокировку можно только
сбросом FIDO, при этом стираются все регистрации на ключе.

### Как разблокировать

На экране блокировки нажмите **Разблокировать ключом безопасности**, введите PIN
(на Windows — в системном окне) и коснитесь ключа.

### Как это устроено

* При включении ключ создаёт учётную запись для `consolecrypt.dev` (не
  резидентную: она живёт только как идентификатор в ConsoleCrypt). На случайную
  соль ключ отвечает 32 байтами HMAC — тем же значением, что даёт WebAuthn PRF.
  Этим ответом запечатывается ключ хранилища в локальный конверт.
* Конверт и соль лежат только в файле ключей профиля на этом устройстве: они не
  синхронизируются, не попадают в резервные копии и на сервер.
* Ни соль, ни идентификатор без самого ключа и его PIN не открывают хранилище.
  Другой ключ — даже того же производителя — ответит иначе.
* Выключение удаляет конверт; учётная запись на ключе остаётся (её можно удалить
  в «Параметры → Учётные записи → Ключ безопасности» на Windows или в Yubico
  Authenticator).

## English

The vault can be opened with a physical USB security key — a YubiKey or another
FIDO2 key with the `hmac-secret` extension — and its PIN. The feature is
optional and is switched on per vault on each device. The passphrase and the
Recovery Key keep working: if the key is lost or its PIN is blocked, unlock with
the passphrase.

**Supported (preview):** Windows 10/11 (system WebAuthn; Windows asks for the
PIN) and macOS (the app asks for the PIN). Linux and mobile are not supported
yet.

To switch it on, unlock with the passphrase, open **Settings → Vault → Physical
security key**, insert the key, enter its FIDO2 PIN and touch the key twice. On
the lock screen choose **Unlock with security key**.

The key registers a non-resident credential for `consolecrypt.dev`; its PRF
answer to a random salt seals the vault key in a local envelope. The envelope
and the salt stay in this device's profile key file: never synced, backed up or
sent to the server. Switching the feature off deletes the envelope.
