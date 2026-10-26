# Локальный ИИ / Local AI — 0.5

## Русский

На Windows и macOS откройте «Настройки» → «Встроенный локальный ИИ».
В серии 0.5 доступен выбор трёх моделей Q4_K_M:

| Вариант | Модель | Загрузка GGUF | Ориентир по общей ОЗУ |
| --- | --- | --- | --- |
| Лёгкая | Qwen2.5-Coder 1.5B Instruct | 1 117 320 768 байт, около 1,1 ГБ | 4 ГиБ |
| Средняя | Qwen3.5 4B | 2 740 937 888 байт, около 2,7 ГБ | 8 ГиБ |
| Тяжёлая | Qwen3.5 9B | 5 680 522 464 байта, около 5,7 ГБ | 16 ГиБ |

Звезда рекомендует наиболее крупный вариант, соответствующий общей памяти
компьютера (4/8/16 ГиБ). Если память неизвестна или меньше 4 ГиБ, звезды нет.
Рекомендация не меняет выбранную модель или флажок default. Это ориентир, а не
гарантия скорости: оставьте память системе и другим программам. На Apple Silicon
используется Metal, на Intel Mac и Windows x64 — CPU. Android, iOS и Linux
не показывают раздел встроенной установки; настройка других провайдеров
сохраняется.

### Установка и переключение

1. Выберите модель и нажмите «Установить локальный ИИ». Большие GGUF-файлы не
   входят в установщик приложения. macOS содержит небольшой подписанный движок
   llama.cpp, Windows скачивает его отдельно.
2. Дождитесь загрузки и проверки размера и SHA-256. Прогресс виден в настройках;
   загрузку можно отменить и продолжить позднее. Установка не начинается сама
   при запуске приложения или смене рекомендации.
3. Флажок «Использовать по умолчанию после установки» изначально включён и
   сохраняется только на этом устройстве. Облачные провайдеры и их
   синхронизируемый выбор остаются. Снятие флажка возвращает обычный выбор.
4. Откройте чат или палитру ИИ. Модель запускается при первом запросе; доступны
   кнопки запуска и остановки. Через 15 минут без запросов процесс выгружается,
   а активная генерация удерживает его.
5. При выборе другой модели текущий процесс останавливается; уже скачанные
   модели сохраняются. Установите новую, если её ещё нет. «Удалить» удаляет
   только выбранную модель и её незавершённые загрузки, сохраняя другие модели,
   движок и предпочтения.

После установки локальной модели при включённом default выбор другой,
ещё не скачанной модели приводит к предложению установить её: он не передаёт запрос облачному провайдеру
незаметно для пользователя. Выбор модели и её установка — разные действия.

### Обновление с 0.4.1

Сохраняются прежние имя файла, ID и SHA-256 Qwen2.5-Coder 1.5B. Модели лежат в
постоянном каталоге данных пользователя вне заменяемого пакета приложения.
Проверенный совместимый GGUF используется повторно при обновлении или ремонте
движка, без повторного скачивания. Выбранная модель и настройка default
сохраняются; новая рекомендуемая модель не заменяет их.

На macOS 0.5 запускается без App Sandbox для обычной локальной оболочки, но
использует прежние заполненные каталоги контейнера на месте. Не удаляйте контейнер,
модели, vault или системную связку ключей. При двух разных заполненных каталогах
или ошибке доступа запуск останавливается с объяснением; данные не объединяются
и не подменяются пустым профилем. Подробнее: [локальные терминалы](LOCAL_TERMINALS.md).

### Контекст и доступ

Запросы идут на `127.0.0.1` со случайным токеном текущего запуска, без системного
HTTP-прокси. Контекст и фильтрация секретов проходят существующее ядро
ConsoleCrypt. Путь к модели и токен процесса не синхронизируются через vault.
Блокировка хранилища, смена профиля и выход останавливают модель. Интернет нужен
при установке, а ответы вычисляются на компьютере.

Проверяйте предложенные команды. В [локальном терминале](LOCAL_TERMINALS.md)
доступна вставка без Enter, затем ручной запуск после проверки; для SSH
сохраняется отдельное подтверждение выполнения. Чат хранится в памяти сессии.

[Руководство со снимками RU](https://consolecrypt.dev/guide?lang=ru#local-ai) ·
[English guide](https://consolecrypt.dev/guide?lang=en#local-ai).
Снимки — настоящие виджеты с демонстрационными состояниями; они не показывают
реальную загрузку или замер скорости модели.

### Источники и лицензии

- [llama.cpp b11739](https://github.com/ggml-org/llama.cpp/releases/tag/b11739), MIT.
- [Qwen2.5-Coder-1.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF):
  официальный GGUF Qwen, Apache-2.0, ревизия `f86cb2c1fa58255f8052cc32aeede1b7482d4361`.
- [Qwen3.5-4B-GGUF](https://huggingface.co/unsloth/Qwen3.5-4B-GGUF):
  модель Qwen, GGUF-квантование и распространение Unsloth, Apache-2.0,
  ревизия `e87f176479d0855a907a41277aca2f8ee7a09523`.
- [Qwen3.5-9B-GGUF](https://huggingface.co/unsloth/Qwen3.5-9B-GGUF):
  модель Qwen, GGUF-квантование и распространение Unsloth, Apache-2.0,
  ревизия `3885219b6810b007914f3a7950a8d1b469d598a5`.

Точные URL, размеры и SHA-256 находятся в
`client/flutter/lib/ai/local/local_ai_catalog.dart`. В настройках доступен текст
«Источники и лицензии моделей», включая лицензию без подключения к интернету.
Для Qwen3.5 движок настроен без отдельного reasoning-ответа. CUDA-пакет пока не
реализован.

### Своя модель (GGUF)

«Добавить свою модель (GGUF)…» выбирает файл на компьютере. ConsoleCrypt проверяет
заголовок GGUF (версии 2 и 3), копирует файл в свою папку моделей, считая SHA-256 по
пути, и выбирает модель; оригинал не меняется, его можно удалить. Повторное
добавление того же файла использует уже сделанную копию. Модель называется по имени
файла и помечена «Своя»; ориентир по ОЗУ — оценка по размеру файла (не меньше 4 ГиБ).
Происхождение и лицензию своей модели проверяйте сами. Перед каждым первым запуском
за сеанс копия сверяется с SHA-256. «Удалить» удаляет копию и убирает модель из
списка, выбранной становится Qwen2.5-Coder 1.5B. Свою модель приложение никогда не
скачивает: если копия пропала, удалите модель и добавьте файл снова. Подходят
модели, которые читает встроенный llama.cpp b11739, с шаблоном чата внутри GGUF.

## English

In Windows/macOS Settings, open **Built-in local AI**. The 0.5 series offers
Light Qwen2.5-Coder 1.5B (about 1.1 GB, 4 GiB RAM), Medium Qwen3.5 4B (about
2.7 GB, 8 GiB RAM) and Strong Qwen3.5 9B (about 5.7 GB, 16 GiB RAM), all Q4_K_M.
The star recommends the largest tier fitting total system RAM; unknown RAM or
less than 4 GiB shows no star. It never changes your selection or default.
Memory guidance does not guarantee speed. Apple Silicon uses Metal; Intel Mac
and Windows x64 use CPU. Built-in installation is unavailable on Linux/mobile.

Select a model, then install it. The app verifies exact size and SHA-256 and
supports cancellation/resume. Models are optional downloads outside the app
package. macOS bundles the small signed runtime; Windows installs it separately.
The device-only default starts enabled after installation; your cloud provider
configuration remains. After a local model has been installed, selecting another
model that is not downloaded asks for installation instead of silently falling
back to a cloud provider.

Switching models stops the current process and retains downloaded models.
Remove deletes only the selected model and its partial downloads, preserving
other models, the runtime and preferences. A verified compatible GGUF is reused
after a runtime repair or application update. The original 0.4.1 model ID,
filename, checksum and device default are preserved. macOS reuses populated
legacy container storage in place and stops on conflicting or unreadable data.
Keep app data, models, vault and Keychain when updating.

The model starts on demand and stops after 15 idle minutes, vault lock, profile
switch or quit. Inference uses authenticated loopback with system HTTP proxies
disabled; context filtering still applies. Local-terminal commands support
Insert without Enter, followed by your manual review and execution. SSH keeps
its execution approval. The current chat is session-only.

**Your own model (GGUF).** “Add your own model (GGUF)…” picks a file on the computer.
ConsoleCrypt checks the GGUF header (versions 2 and 3), copies the file into its models
folder while computing SHA-256 and selects it; the original is untouched and can be
deleted. Adding the same file again reuses the copy. The model is named after the file
and marked Custom; the RAM guidance is estimated from the file size (at least 4 GiB).
Check the origin and license of your own model yourself. The copy is checked against
its SHA-256 before the first start of each session. Remove deletes the copy and drops
the model from the list, selecting Qwen2.5-Coder 1.5B. A custom model is never
downloaded: if the copy is gone, remove the model and add the file again. Any model the
built-in llama.cpp b11739 reads, with a chat template inside the GGUF, will do. A CUDA
package is not available yet.

Qwen publishes the 1.5B GGUF; Unsloth quantizes/distributes the Qwen3.5 4B/9B GGUFs.
All three models use Apache-2.0; llama.cpp uses MIT. Pinned revisions are listed
above and hashes live in the catalog source. Settings includes offline model
source/license notices. Screenshots use real widgets and synthetic states,
not download or inference measurements.

## Проверка разработчиком / Developer checks

Unit/widget tests do not download models. For an explicit real-engine check:

```sh
cd client/flutter
dart run test/ai/local/local_ai_smoke.dart --run --directory=/absolute/test/cache
```

The smoke check exercises installation, authorization, an answer and idle
lifecycle. Add `--runtime-only` to check just the runtime. Use a separate test
directory, never the user's installed model directory. Native packaging and
large-model validation are separate release checks.
