# Поддержка платформ ConsoleCrypt 1.0.0

Основные данные и рабочие функции используют общее Flutter/Rust-ядро.
Системные возможности зависят от ОС, а успешная сборка не заменяет проверку
реального сканера отпечатка или USB-ключа. Ниже указан фактический набор выпуска.

| Возможность | Windows | macOS | Linux | Android | iOS Simulator |
| --- | --- | --- | --- | --- | --- |
| SSH, SFTP, RDP, подключения к БД | Да | Да | Да | Да | Предварительный порт |
| Kubernetes и Docker через SSH | Да | Да | Да | Да | Предварительный порт |
| Локальные терминалы и Docker на этом компьютере | Да | Да | Да | Нет | Нет |
| KDBX 3/4, пароль/ключевой файл, папки, записи, генератор | Да | Да | Да | Да | Да |
| WebDAV и автосохранение KDBX | Да | Да | Да | Да | Да |
| Поиск паролей в палитре и очистка буфера | Да | Да | Да | Да, экранная кнопка/клавиатура | Да |
| Меню записи и перенос между папками | ПКМ, перетаскивание | ПКМ, перетаскивание | ПКМ, перетаскивание | Кнопка меню/долгое нажатие | Кнопка меню/долгое нажатие |
| Локальные KDBX | Редактирование файла | Редактирование файла | Редактирование файла | Импорт в приложение + экспорт | Импорт в приложение + экспорт |
| Биометрия/PIN для KDBX | Windows Hello | Системная защита Secure Enclave | Пока не реализована | Android Keystore + системный запрос | Зависит от возможностей Simulator; пароль доступен всегда |
| FIDO2/PRF для KDBX и хранилища | Да | Да | Пока нет | Пока нет | Нет |
| ИИ через настроенный API/Ollama | Да | Да | Да | Да | Да |
| Установка встроенного локального ИИ | Да | Да | Да, CPU x64 | Пока нет | Нет |
| Сохранённые пробросы Docker/Kubernetes | Да | Да | Да | Да | Да |
| Зашифрованная синхронизация, команды, заметки, совместный доступ | Да | Да | Да | Да | Предварительный порт |

## Файлы выпуска

- Windows x64: установщик EXE и переносимый ZIP.
- macOS Apple Silicon/Intel: universal DMG.
- Linux x64: DEB и RPM; окружение рабочего стола с Secret Service.
- Android ARM64: подписанный APK, Android 11+.
- iOS: universal ZIP приложения для Simulator на Mac, iOS 15+.
  Это не IPA, TestFlight или приложение для физического iPhone.

Для каждого пакета публикуются версия/номер сборки и SHA-256. iOS Simulator
включён в обязательный CI выпуска 1.0.0: отсутствие его архива блокирует публикацию.

## Хранение и синхронизация паролей

На компьютерах редактируется выбранный локальный файл. На телефонах системный
picker может вернуть временную копию, поэтому ConsoleCrypt импортирует
зашифрованную KDBX в своё постоянное приватное хранилище. Автосохранение изменяет
эту копию; «Сохранить локальную копию» выгружает её через системный диалог.
Исходный документ телефона самопроизвольно не перезаписывается.

WebDAV работает одинаково на всех платформах: HTTPS URL с конечным `.kdbx`,
защита от конфликта версий, автоматическое сохранение, отмена открытия и таймауты.
Параметры WebDAV синхронизируются внутри E2EE, пароль — отдельным секретом.
Локальные пути, биометрические конверты и сохранённые Docker/Kubernetes-пробросы
не синхронизируются.

## English

The common Flutter/Rust core supplies password management, WebDAV, generators,
palette lookup and saved forwarding presets on all packaged platforms. OS
facilities differ: local terminals/Docker need desktop process/socket access;
system-protected KDBX unlock uses Windows Hello, Apple Secure Enclave or Android
Keystore. Linux does not claim biometric protection for an ordinary desktop
keyring. Physical FIDO2/PRF keys currently support Windows/macOS only.
Built-in model installation supports Windows, macOS and Linux x64 (CPU);
configured AI APIs remain available on mobile.

Desktop KDBX editing writes the chosen file. Mobile imports retain a durable,
encrypted in-app copy and export explicitly through the OS picker. WebDAV
edits the remote file directly, with the same conflict protection and timeouts.
WebDAV settings and separate credentials use E2EE sync; local paths, quick-unlock
envelopes and Docker/Kubernetes presets stay device-local.

Artifacts: Windows x64 EXE/ZIP; macOS universal DMG; Linux x64 DEB/RPM;
Android ARM64 APK (Android 11+); iOS universal Simulator ZIP (iOS 15+ on Mac).
The Simulator archive is required for 1.0 publication and is not an iPhone,
TestFlight or App Store build. Physical biometrics/keys require a real supported
device; package builds alone cannot certify their behaviour.
