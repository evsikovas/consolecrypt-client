# Android: публикация ConsoleCrypt в RuStore

Приложение сохраняет идентификатор `io.consolecrypt.consolecrypt` и основной набор
функций Android. Интеграция с платежами, Push SDK и другими SDK RuStore для этой
публикации не требуется: приложение бесплатное, обновление выполняет сам магазин.

## Два варианта Android

| Вариант | Команда Flutter | Обновления |
|---|---|---|
| `direct` | `flutter build apk --release --flavor direct --target-platform android-arm64` | Подписанная лента ConsoleCrypt и системный установщик APK |
| `rustore` | `flutter build appbundle --release --flavor rustore --target-platform android-arm64` | Только RuStore |

В `rustore` отсутствуют разрешение `REQUEST_INSTALL_PACKAGES` и FileProvider
встроенного обновления. Android-канал установки дополнительно заблокирован;
проверка ленты, скачивание APK и запуск установщика отключены в Dart.
В настройках остаётся кнопка открытия карточки приложения в RuStore.
Остальные платформы и канал `direct` сохраняют прежнее поведение.

В исправленной сборке магазина также удалены добавленные CameraX разрешения
`RECORD_AUDIO`, `READ_EXTERNAL_STORAGE` и `WRITE_EXTERNAL_STORAGE`. Камера нужна
для локального сканирования QR-кода восстановления, звук отключён. Файлы KDBX,
резервные копии и пользовательские документы выбираются через системный диалог
Android с доступом к выбранному URI. Проверка готовых APK и AAB запрещает все
четыре лишних разрешения, включая встроенную установку пакетов.

Команды Android теперь требуют явного `--flavor direct` или `--flavor rustore`.
Скрипт `client/scripts/build-android.sh` выбирает `direct` автоматически.

## Подпись первой публикации

Для AAB используются два постоянных ключа:

1. **Ключ приложения** подписывает APK, которые магазин установит пользователям.
   Его зашифрованный экспорт PEPK загружается в RuStore один раз.
2. **Ключ загрузки** подписывает AAB. В RuStore загружается его публичный
   сертификат PEM. Закрытый ключ загрузки остаётся у разработчика.

В разделе «Подпись приложения → Загрузить» нужно выбрать `pepk_out.zip` и
`upload_certificate.pem`. Затем в разделе «Версии» загрузить подписанный AAB.
Пароли, исходные JKS/PKCS12 и файлы `*-signing.properties` в консоль не загружаются.

Закрытые ключи и пароли необходимо сохранить в отдельной защищённой резервной
копии. Их нельзя добавлять в Git, публичные артефакты, скриншоты или логи CI.
При утрате ключа приложения нельзя просто создать новый и продолжить обычное
обновление уже установленного приложения.

Прежние Android preview APK подписывались отладочным ключом runner. Новый
production-ключ отличается: прямое обновление поверх такого APK не поддерживается.
Перед переходом на магазин сохраните резервную копию локального хранилища,
проверьте пароль восстановления и сохраните локальные KDBX-файлы. Не удаляйте
единственную копию данных ради установки новой подписи.

## Локальная сборка и конфигурация подписи

Gradle читает путь из `CC_ANDROID_SIGNING_PROPERTIES`. Формат файла:

```properties
storeFile=/absolute/private/path/consolecrypt-upload.jks
storePassword=PRIVATE_VALUE
keyAlias=consolecrypt-upload
keyPassword=PRIVATE_VALUE
```

Для release-варианта `rustore` файл обязателен: автоматическая подстановка
отладочного ключа запрещена. Для AAB указывается ключ загрузки, для проверочного
APK — ключ приложения.

Скрипт `client/scripts/build-android-rustore.sh` использует две переменные с
путями к таким файлам: `CC_ANDROID_APP_SIGNING_PROPERTIES` и
`CC_ANDROID_UPLOAD_SIGNING_PROPERTIES`. Он создаёт AAB и APK с одним номером
версии в новой папке `dist/android/rustore-<version>/`, без публикации.
Требуются обычное Android/Rust окружение проекта, `ANDROID_HOME`, `JAVA_HOME`
и уникальный `CC_BUILD_NUMBER`, превышающий предыдущие Android-загрузки.

## GitHub Actions

Workflow **Android RuStore packages** вызывается общей сборкой релиза и сохраняет
проверенные APK/AAB в артефакт `android-rustore`. Начиная с 1.1.0, без него общий
релиз не публикуется. Обычный Android APK и оба RuStore-пакета получают один
номер Android-сборки, но используют разные каналы обновления и подписи.

Для отдельной проверки workflow можно запустить вручную с ветки `main`, указав
`build_number`. Native-код собирается из исходников. Общий job публикации помещает
APK/AAB в релиз GitHub после успеха всех платформ; сам RuStore workflow не
отправляет файлы в магазин. Загрузка версии и отправка на модерацию выполняются
в RuStore Консоль с уже зарегистрированным ключом подписи.

Обязательные Repository Secrets:

- `ANDROID_APP_KEYSTORE_BASE64`: Base64 содержимого ключа приложения.
- `ANDROID_APP_KEYSTORE_PASSWORD`: пароль этого хранилища.
- `ANDROID_UPLOAD_KEYSTORE_BASE64`: Base64 содержимого ключа загрузки.
- `ANDROID_UPLOAD_KEYSTORE_PASSWORD`: пароль этого хранилища.

Необязательные `ANDROID_APP_KEY_ALIAS` и `ANDROID_UPLOAD_KEY_ALIAS` по умолчанию
равны `consolecrypt-app` и `consolecrypt-upload`. Если пароли ключей отличаются
от паролей хранилищ, задайте `ANDROID_APP_KEY_PASSWORD` и
`ANDROID_UPLOAD_KEY_PASSWORD`. Используются те же runner labels и переменные
`ANDROID_HOME`/`JAVA_HOME`, что в существующей Android-сборке.

Ключи разворачиваются в приватную временную папку runner; после задания папка
удаляется. В артефакт попадает только текущая папка APK/AAB и контрольные суммы.

## Проверки перед загрузкой

`client/scripts/verify-rustore.py` проверяет итоговые файлы, а не только исходные
манифесты: отсутствие запрещённого разрешения и debug-флага, production-подпись
APK, сертификат загрузки AAB, package ID и versionCode, структуру Bundle,
полный ARM64 payload, лицензии и совместимость ELF с 16 КиБ страницами.
Для проверки используется официальный Google bundletool с фиксированной SHA-256.

Системные и аппаратные функции сохраняют обычные ограничения Android:
биометрия зависит от устройства; новые FIDO2 и desktop-минимализм в Android
не добавляются. Подготовка подписи и проверки сборки не означают одобрения
модерации или проверки на каждом физическом устройстве.

Текст для повторной модерации:

> Удалено разрешение REQUEST_INSTALL_PACKAGES. В варианте RuStore отключены
> скачивание APK и запуск системного установщика из приложения. Обновления
> выполняются через RuStore. Сборка подписана production-ключом, debug-режим отключён.

Официальные инструкции: [загрузка AAB](https://www.rustore.ru/help/developers/publishing-and-verifying-apps/app-publication/new-version-app/upload-aab),
[подписи APK/AAB](https://www.rustore.ru/help/developers/publishing-and-verifying-apps/app-publication/apk-signature).
