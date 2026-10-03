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

### Готовый архив из релиза 0.3.0

В [релизе v0.3.0](https://git.evsikov.net/publics/consolecrypt/-/releases/v0.3.0)
скачайте [часть 1](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1377-ios-simulator-universal.zip.001),
[часть 2](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/ConsoleCrypt-0.3.0%2B1377-ios-simulator-universal.zip.002)
и [SHA256SUMS-0.3.0.txt](https://git.evsikov.net/api/v4/projects/14/packages/generic/consolecrypt/0.3.0/SHA256SUMS-0.3.0.txt).
Сохраните все три файла в одной папке. Архив опубликован двумя
частями; объединение восстанавливает проверенный ZIP без изменения приложения. Отдельные части распаковывать не нужно.

В Terminal перейдите в эту папку и проверьте обе части:

```sh
awk '$2 == "ConsoleCrypt-0.3.0+1377-ios-simulator-universal.zip.001" || \
     $2 == "ConsoleCrypt-0.3.0+1377-ios-simulator-universal.zip.002" { print; n++ } \
     END { if (n != 2) exit 1 }' SHA256SUMS-0.3.0.txt > ios-parts.SHA256SUMS \
    && shasum -a 256 -c ios-parts.SHA256SUMS
```

В списке контрольных сумм указаны две опубликованные части. Продолжайте
только после двух результатов `OK` — по одному для `.001` и `.002`:

```sh
cat ConsoleCrypt-0.3.0+1377-ios-simulator-universal.zip.001 \
    ConsoleCrypt-0.3.0+1377-ios-simulator-universal.zip.002 \
    > ConsoleCrypt-0.3.0+1377-ios-simulator-universal.zip
unzip ConsoleCrypt-0.3.0+1377-ios-simulator-universal.zip
xcrun simctl install booted Runner.app
xcrun simctl launch booted io.consolecrypt.consolecrypt
```

Перед установкой запустите нужный Simulator в Xcode. Если открыто несколько
Simulator, замените `booted` на UDID выбранного устройства. Эти части и
восстановленный ZIP предназначены для Simulator и не являются iPhone IPA.

Для нативного smoke test выберите UDID из `flutter devices`:

```sh
cd client/flutter
flutter test integration_test/ios_core_test.dart -d <SIMULATOR_UDID> \
  --dart-define=CC_MOCK=false
```

Перед такой нативной тестовой сборкой также зарезервируйте номер из корня:
`python3 client/scripts/bump-version.py`. Проверка использует отдельное
приватное тестовое хранилище и не требует учётных данных публичного сервера.

## SSH/SFTP regression и GitLab CI

### Что содержит артефакт iOS preview в GitLab

Начиная с 0.3, CI сохраняет полный Simulator ZIP на Mac runner **вне checkout**:
`$HOME/.cache/consolecrypt/ios-preview/<полный source SHA>/<job ID>/`.
Каталог имеет права `0700`, ZIP — `0600`. Следующая очистка исходников не удаляет
этот архив. Сохранение проверяет native bundle ID/версию/job ID, CRC и SHA-256;
оно не является отдельной проверкой работы приложения на iPhone.

Скачанный **CI artifact** содержит только три небольших файла:
`ConsoleCrypt-ios-preview.json`, `SHA256SUMS-ios-preview.txt` и
`README-ios-preview.txt`. В нём нет приложения. Получите полный ZIP у владельца
Mac runner и сверьте SHA-256 из квитанции перед распаковкой. Если job завершился
ошибкой после сохранения, квитанция ещё не означает успешную проверку запуска.

GitLab отправляет все artifact paths одним архивом. Разделение ZIP на части
внутри этого же artifact не уменьшает общий размер запроса и не решает ошибку
`413 Request Entity Too Large`. Полный ZIP не отправляется в CI artifact;
публичный релиз с отдельно проверенными частями публикуется отдельным процессом.
Локальная `build-ios.sh --simulator` по-прежнему выдаёт полный архив в `dist/ios/`.

The iOS preview **CI artifact contains metadata/checksums only**, not the app.
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

В GitLab файл `client/ci/ios.yml` предоставляет задания `ios-regression` и
`build-ios-preview`. Regression запускается вручную; preview автоматически
запускается только на точной ветке `codex/rdp-0.3-source`, на остальных ветках —
вручную. Runner должен иметь теги `macos` и `arm64`, Xcode/runtime, Flutter,
Rust, CocoaPods, Python; для regression также нужен Docker. Оба задания делят
`resource_group: consolecrypt-macos` с macOS-сборкой. Каждое компилирует только
один нативный bundle и использует свой `CI_JOB_ID` как нижнюю границу номера.
Небольшие CI-квитанции preview хранятся 90 дней; срок хранения полного ZIP
в отдельном локальном кэше контролирует владелец runner. Apple signing secrets
не требуются.

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

Текущий готовый архив — **0.3.0+1377**. Проверены версия, целостность ZIP,
SHA-256, подпись и ARM64/x86-64 Simulator slices в Runner, App, Flutter и
Rust bridge. Обе части побайтно восстанавливают исходный ZIP. Приложение
установлено и запущено в отдельном временном Simulator: подтверждён экран
приветствия без подключения аккаунта, хранилища или SSH-хоста. Это проверка
запуска текущего продукта, а не повтор полного SSH/SFTP-прогона выше и не
проверка настоящего iPhone.

Фундамент Rust также проверен компиляцией для физического ARM64 iPhone.
Полный прогон выше выполнен в Simulator и не подтверждает
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

Download both **0.3.0+1377** Simulator ZIP parts and `SHA256SUMS-0.3.0.txt` using
the links above. Verify both published parts against their checksum entries,
join them, then install
`Runner.app` with `xcrun simctl install`. The current archive contains ARM64 and
x86-64 Simulator slices and passed an isolated welcome-screen launch check.
The full SSH/SFTP regression described above is a separate earlier test.
This is not an iPhone-installable IPA; a physical device requires Xcode signing
and separate keyboard, biometrics, network and background acceptance checks.
