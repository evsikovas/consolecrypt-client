# ConsoleCrypt 0.4.0

## Русский

- Android: режим тачпада для RDP. Палец перемещает курсор относительно его текущей
  позиции; касание нажимает левую кнопку, два пальца прокручивают, касание двумя
  пальцами нажимает правую кнопку. Доступны удержание для перетаскивания и отдельная
  кнопка экранной клавиатуры. Компактная панель сохраняет рабочую область телефона.
- Windows/macOS: установка локального ИИ из настроек. Qwen2.5-Coder 1.5B Q4_K_M
  скачивается с проверкой SHA-256; llama.cpp входит в macOS-пакет и отдельно
  устанавливается на Windows. Модель по умолчанию
  выбирается только на этом устройстве, запускается при запросе и выгружается
  после 15 минут бездействия. На Apple Silicon используется Metal, на Windows — CPU.
  Локальные запросы проходят существующие фильтры контекста и подтверждение команд.
- Исправлены автоматическая блокировка при бездействии и отмена генерации ИИ.
- Релизы клиента собираются только по тегам из `main`. Linux использует домашний
  k3s ARC-раннер, Android — Mac-раннер с прежним ключом подписи.

Локальный ИИ доступен на macOS ARM64/x64 и Windows x64. Загрузка модели — примерно
1,1 ГБ плюс движок; подключение к интернету нужно для установки, а не для ответов.
Модель небольшая и может ошибаться: команды выполняются только после подтверждения.
iOS-пакет остаётся предварительной сборкой для Simulator.

**Обновление старой установки:** прежний адрес `updates.consolecrypt.evsikov.net`
сейчас недоступен. Если встроенная проверка использует его, скачайте установщик
со [страницы загрузок](https://consolecrypt.dev/download?lang=ru), закройте
ConsoleCrypt и обновите существующую установку; на macOS замените приложение
в Applications. Сохраните резервную копию и комплект восстановления. Хранилище,
данные приложения и системную связку ключей удалять не нужно. Новые сборки
получают обновления с `updates.consolecrypt.dev`.

## English

Android RDP now offers a relative touchpad, two-finger scroll/right-click, drag and
an explicit software-keyboard toggle. Desktop Windows/macOS adds an optional,
verified Qwen2.5-Coder 1.5B / llama.cpp installation with device-only default choice,
on-demand startup and idle shutdown. Existing context filtering and command
approval apply. Idle vault locking and AI cancellation are fixed. Native releases
now build only from version tags on `main`.

Local AI supports macOS ARM64/x64 and Windows x64 (CPU); the iOS artifact remains
a Simulator preview. See [local AI](LOCAL_AI.md) and [RDP controls](RDP.md).

**Updating an older installation:** the previous address
`updates.consolecrypt.evsikov.net` is currently unavailable. If your in-app
updater uses it, download the installer from the
[download page](https://consolecrypt.dev/download?lang=en), quit ConsoleCrypt
and update the existing installation; on macOS, replace the app in Applications.
Save a backup and recovery kit. Keep the app's data, vault and system keychain;
do not uninstall them. New builds use `updates.consolecrypt.dev`.
