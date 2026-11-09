# ConsoleCrypt 1.1.1

## Русский

Исправления безопасности и полноэкранного режима macOS.

- **SQLite / SQLCipher:** устранена причина RUSTSEC-2026-0459
  (CVE-2026-119375). При несовпадении размеров страниц базы и WAL
  непрочитанный остаток страницы теперь обнуляется. Официальное исправление
  SQLite перенесено в оба встроенных движка; обычное обновление зависимости
  не закрывало бы проблему в используемом SQLCipher.
- **macOS:** нативная панель больше не занимает лишнюю полосу над приложением
  при переходе во весь экран. При выходе возвращается её прежнее состояние.
  Обработка применяется к обычному полноэкранному окну и сохраняет
  восстановление окна после полноэкранного RDP.
- **Проверки:** регрессия чтения WAL проверяет размеры страниц и границы
  буфера; исходная функция служит отрицательным контролем. Аудит проверяет
  хеши исправленного C-кода, заголовков, Rust-обёртки и build script.
  Проверяются шифрование, смена ключа, повторное открытие базы и поиск.

Форматы локальных баз и синхронизации не меняются. Обновление сервера
синхронизации для этого выпуска не требуется.

Пакеты: Windows, macOS, Linux DEB/RPM, Android, отдельные APK/AAB для RuStore
и iOS Simulator для Mac. Simulator не устанавливается на iPhone.

## English

Security and macOS fullscreen fixes.

- **SQLite / SQLCipher:** fixed the underlying RUSTSEC-2026-0459
  (CVE-2026-119375) issue. If database and WAL page sizes differ, the unread
  page tail is now zero-initialized. The upstream SQLite fix is backported
  to both bundled engines; a dependency upgrade alone would leave the
  SQLCipher build affected.
- **macOS:** the native toolbar no longer reserves an extra band above the
  application in fullscreen. Its previous visibility is restored on exit.
  This covers ordinary OS fullscreen while preserving RDP window restoration.
- **Checks:** the WAL regression exercises page sizes and buffer boundaries,
  using the original reader as a negative control. The audit verifies hashes
  of the patched C sources, headers, Rust wrapper and build script.
  Encryption, rekeying, database reopening and search are checked as well.

Local database and sync formats are unchanged. No sync server upgrade is
required for this release.

Packages target Windows, macOS, Linux DEB/RPM, Android, separate RuStore
APK/AAB files and iOS Simulator on Mac. The Simulator archive is not an
iPhone installer.
