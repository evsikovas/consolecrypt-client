# Сборка ConsoleCrypt для iOS

iOS пока является предварительным портом. Flutter использует общее Rust-ядро,
нативный SSH, SQLCipher и защищённый Keychain. Минимальная версия проекта —
iOS 15. Настольный механизм скачивания и установки обновлений на iOS отключён.

## Требования

- macOS с полной установкой Xcode, выбранной через `xcode-select`, и iOS SDK;
- установленный в Xcode Simulator runtime;
- Flutter, CocoaPods, Rust/rustup и Python 3;
- инструменты сборки из [общего руководства](BUILDING.md).

Не запускайте Flutter-тесты или другую нативную сборку одновременно в том же
checkout. Они используют общие сгенерированные файлы. Private signing keys,
provisioning profiles и настройки личной команды не добавляются в Git.

## Simulator без Apple Account

Из корня репозитория:

```sh
client/scripts/build-ios.sh --simulator
```

Скрипт устанавливает Rust targets для ARM64 и Intel Simulator, резервирует
новый номер сборки, компилирует реальное ядро и проверяет версии в `Info.plist`.
Результаты находятся в `dist/ios/`: ZIP с `Runner.app`, SHA-256 и файл версии.
Неудачная попытка тоже расходует номер; не уменьшайте счётчик вручную.

Запустите нужный Simulator в Xcode. Для приложения, собранного этим скриптом:

```sh
xcrun simctl install booted client/flutter/build/ios/iphonesimulator/Runner.app
xcrun simctl launch booted io.consolecrypt.consolecrypt
```

При нескольких Simulator вместо `booted` укажите UDID нужного устройства.
Для ZIP сначала проверьте SHA-256, затем распакуйте `Runner.app` и установите
его той же командой. Этот ZIP не является IPA для установки на iPhone.

### Готовый архив из релиза 0.5.6

В [релизе v0.5.6](https://github.com/evsikovas/consolecrypt-client/releases/tag/v0.5.6)
скачайте [полный Simulator ZIP](https://github.com/evsikovas/consolecrypt-client/releases/download/v0.5.6/ConsoleCrypt-0.5.6%2B23015-ios-simulator-universal.zip)
и [SHA256SUMS-0.5.6.txt](https://github.com/evsikovas/consolecrypt-client/releases/download/v0.5.6/SHA256SUMS-0.5.6.txt).
Сохраните оба файла в одной папке. ZIP опубликован целиком; объединение частей
больше не требуется. Сборка — **0.5.6+23015**, размер — **154 939 478 байт**.

В Terminal перейдите в эту папку и проверьте архив:

```sh
awk '$2 == "ConsoleCrypt-0.5.6+23015-ios-simulator-universal.zip" { print; n++ } \
     END { if (n != 1) exit 1 }' SHA256SUMS-0.5.6.txt > ios.SHA256SUMS \
    && shasum -a 256 -c ios.SHA256SUMS
```

Продолжайте только после результата `OK`. Ожидаемая SHA-256:
`6394d2c15d6fd28ab58e1b28638dd75bb751dc3e487212ee90e44b001f5b1264`.

```sh
unzip ConsoleCrypt-0.5.6+23015-ios-simulator-universal.zip
xcrun simctl install booted Runner.app
xcrun simctl launch booted io.consolecrypt.consolecrypt
```

Перед установкой запустите нужный Simulator в Xcode. Если открыто несколько
Simulator, замените `booted` на UDID выбранного устройства. Этот ZIP предназначен
для Simulator и не является IPA для установки на физический iPhone.

Для нативного smoke test выберите UDID из `flutter devices`:

```sh
cd client/flutter
flutter test integration_test/ios_core_test.dart -d <SIMULATOR_UDID> \
  --dart-define=CC_MOCK=false
```

Перед такой нативной тестовой сборкой также зарезервируйте номер из корня:
`python3 client/scripts/bump-version.py`. Проверка использует отдельное
приватное тестовое хранилище и не требует учётных данных публичного сервера.

## GitHub Actions и iOS Simulator

В workflow `Client tag release` job `ios` зависит от macOS и использует
Mac runner, выбранный настройкой `MACOS_BUILD_RUNNER`. Она запускает
`ci-ios.sh --build-only`, устанавливает и открывает preview в отдельном Simulator.
Actions artifact содержит **полный Simulator ZIP**, его SHA-256 и файл версии;
срок хранения — 30 дней. После успешной проверки всех платформ job `publish`
сверяет квитанции версий и автоматически публикует семь установщиков и общие
контрольные суммы в GitHub Release. Push ветки сам по себе выпуск не запускает.
[Раннеры и номера сборок](GITHUB_ACTIONS.md).

Проверенный выпуск **0.5.6+23015** собран 20 октября 2026 года (UTC):
[iOS job](https://github.com/evsikovas/consolecrypt-client/actions/runs/39377995191/job/118489659918)
завершился успешно; журнал подтверждает установку и запуск
`io.consolecrypt.consolecrypt` с получением PID.

In GitHub Actions, `Client tag release` runs the `ios` job after macOS and uploads
the complete Simulator ZIP, checksum and version receipt. After all platforms
pass, the publisher checks the receipts and publishes the release automatically.
The artifact and released ZIP are for Simulator, not installable iPhone IPAs.

## SSH/SFTP regression и прежний GitLab CI

### Исторический GitLab artifact: только квитанции

В прежнем GitLab CI, начиная с 0.3, полный Simulator ZIP сохранялся на Mac
runner **вне checkout**:
`$HOME/.cache/consolecrypt/ios-preview/<полный source SHA>/<job ID>/`.
Каталог имеет права `0700`, ZIP — `0600`. Следующая очистка исходников не удаляет
этот архив. Сохранение проверяет native bundle ID/версию/job ID, CRC и SHA-256;
оно не является отдельной проверкой работы приложения на iPhone.

Исторический **GitLab CI artifact** содержит только три небольших файла:
`ConsoleCrypt-ios-preview.json`, `SHA256SUMS-ios-preview.txt` и
`README-ios-preview.txt`. В нём нет приложения. Получите полный ZIP у владельца
Mac runner и сверьте SHA-256 из квитанции перед распаковкой. Если job завершился
ошибкой после сохранения, квитанция ещё не означает успешную проверку запуска.

GitLab отправлял все artifact paths одним архивом. Разделение ZIP на части
внутри этого же artifact не уменьшало общий размер запроса и не решало ошибку
`413 Request Entity Too Large`, поэтому тот CI сохранял только квитанции.
Текущие GitHub artifact и release содержат полный ZIP. Локальная
`build-ios.sh --simulator` также выдаёт полный архив в `dist/ios/`.

The historical GitLab iOS preview **artifact contains metadata/checksums only**, not the app.
The full ZIP is retained privately on the macOS runner, outside its checkout,
under `$HOME/.cache/consolecrypt/ios-preview/<source SHA>/<job ID>/`. Obtain it
from the runner owner and verify the recorded SHA-256 before extracting it.
Splitting files within one CI artifact does not avoid its aggregate upload
limit. Public release downloads are a separate reviewed publication; these
previews are for Simulator and are not installable iPhone IPAs.

Для полного теста на Simulator нужен запущенный Docker. Из корня:

```sh
client/scripts/ci-ios.sh --test-only
```

Скрипт создаёт отдельный временный iPhone Simulator из установленного runtime
и резервирует номер одной нативной тестовой сборки. В этот запуск входят тест
хранилища и настоящий SSH/SFTP через OpenSSH-контейнер. Пароль и ключи хоста
создаются при запуске. Пароль передаётся только через временный endpoint на
localhost, не записывается в исходники, аргументы команд или `DART_DEFINES`.
Контейнер и созданный Simulator удаляются даже при ошибке. Уже открытый
Simulator пользователя и установленное в нём приложение не заменяются.

Тест проверяет UTF-8/кириллицу при вводе и вставке, выделение/копирование,
переключение двух вкладок, возврат Flutter из background, потерю и восстановление
SSH и передачу файла с кириллическим именем по SFTP. Тест инжектирует события
Flutter TextInput и lifecycle; это не проверка настоящей экранной клавиатуры
или приостановки приложения операционной системой на iPhone.

Для отдельной проверенной Simulator-сборки:

```sh
client/scripts/ci-ios.sh --build-only
```

Этот запуск компилирует продукт, устанавливает и запускает его в собственном
временном Simulator. Архив, SHA-256 и версия остаются в `dist/ios`.

Файл `client/ci/ios.yml` сохранён для прежних ручных GitLab-проверок.
Его правила веток и небольшие preview-квитанции не являются текущим процессом
релиза. Выпуски собираются по тегам в GitHub Actions, как описано выше.
Локальные `--test-only` и `--build-only` остаются отдельными режимами;
Apple signing secrets для Simulator не требуются.

## Настоящий iPhone

```sh
client/scripts/build-ios.sh --device
```

Результат — неподписанный ARM64 development bundle. Он проверяет компиляцию,
но не устанавливается на телефон как обычное приложение. Для установки
откройте `client/flutter/ios/Runner.xcworkspace` в Xcode, выберите свой телефон,
настройте Signing & Capabilities со своей Apple Account/Personal Team,
включите Developer Mode на телефоне и запустите Runner из Xcode. При необходимости
выберите уникальный bundle identifier для своей команды.

Правила подписи и доступности распространения определяет Apple:
[настройка iOS в Flutter](https://docs.flutter.dev/platform-integration/ios/setup),
[Apple Developer account](https://developer.apple.com/help/account/basics/about-your-developer-account).
Для TestFlight/App Store нужна соответствующая программа Apple; Simulator
не подтверждает готовность к публикации в магазине.

## Что уже проверено и что осталось

Ранее выполненный полный нативный прогон `0.1.20+47` на Simulator прошёл обе
интеграционные проверки. Проверены настоящее Rust-ядро, создание локального
профиля, SQLCipher, хранение хоста, сохранение Keychain после перезапуска ядра,
разблокировка парольной фразой и блокировка хранилища. SSH-проверка включает
кириллический ввод и Return через Flutter TextInput, выделение/копирование/
вставку, две вкладки, возврат Flutter lifecycle, UTF-8 файл по SFTP, потерю
соединения, переподключение с сохранением буфера и повторное открытие SFTP.
Закрытие активной вкладки после переподключения обязательно завершается за
пять секунд; финальная очистка прошла без таймаутов. Это отдельная тестовая
сборка: product ZIP создаётся следующим запуском `--build-only`.

Текущий готовый архив — **0.5.6+23015**. CI проверил bundle ID и версию,
установил приложение в отдельный временный Simulator и запустил процесс,
получив PID. Полный ZIP опубликован одним файлом; его размер и SHA-256
сверены с GitHub asset digest и общим файлом контрольных сумм.
Это проверка сборки, установки и запуска процесса, а не повтор полного
SSH/SFTP-прогона выше. Визуальная приёмка, работа RDP и настоящее устройство
этим запуском не проверялись.

В прежней приёмке фундамент Rust также проверяли компиляцией для физического
ARM64 iPhone. Полный прогон выше выполнен в Simulator и не подтверждает
установку или работу на настоящем телефоне.

На физическом устройстве ещё нужно проверить Face ID/Touch ID, блокировку
устройства, экранную клавиатуру терминала, SSH/SFTP по Wi-Fi и мобильной сети,
возврат из background, импорт/экспорт документов и доступность интерфейса.
При уходе приложения в background снимок переключателя приложений закрывается.
Порт не обещает универсального запрета пользовательских скриншотов на iOS.

Практический checklist для телефона после подписи в Xcode:

1. Создайте отдельный тестовый локальный профиль, настройте биометрическую
   разблокировку, проверьте отмену Face ID и возврат к парольной фразе.
2. Подключитесь к своему тестовому SSH-хосту. Введите кириллицу, удалите
   символы, используйте Return и Ctrl+C. Проверьте, что одного касания терминала
   хватает для ввода после смены вкладки и возврата в приложение.
3. Скопируйте выделение и вставьте команду. Для вставки с переводом строки
   подтвердите предупреждение; отмена должна оставить команду невыполненной.
4. Поверните телефон с открытой клавиатурой и измените размер шрифта.
   Проверьте видимость активной строки, панель служебных клавиш и safe areas.
5. Передайте через SFTP файл с кириллическим именем, скачайте его обратно
   и сравните содержимое. Повторите соединение на Wi-Fi и мобильной сети.
6. Заблокируйте телефон, откройте переключатель приложений и вернитесь.
   Снимок приложения не должен раскрывать терминал/хранилище. Отдельно
   проверьте установленный таймер блокировки и повторную разблокировку.
7. Отключите сеть, верните её и восстановите SSH/SFTP. Проверьте, что
   сохранённый ключ хоста не требует повторного принятия, а смена ключа
   блокирует подключение.
8. Проверьте импорт/экспорт зашифрованной резервной копии, VoiceOver,
   увеличенный системный текст и режим уменьшения движения.

Результат Simulator не заменяет этот checklist. Не публикуйте реальные
пароли, приватные SSH-ключи или содержимое рабочего терминала в отчёте.


## English

Download the complete **0.5.6+23015** Simulator ZIP and `SHA256SUMS-0.5.6.txt`
using the links above. Verify its single checksum entry, extract the ZIP, then install
`Runner.app` with `xcrun simctl install`. CI installed this build and launched a
process in an isolated Simulator. The published size and SHA-256 match GitHub
asset metadata and the release checksum manifest. This CI run did not repeat visual, RDP or physical-device
acceptance. The full SSH/SFTP regression above is a separate earlier test.
This is not an iPhone-installable IPA; a physical device requires Xcode signing
and separate keyboard, biometrics, network and background acceptance checks.
