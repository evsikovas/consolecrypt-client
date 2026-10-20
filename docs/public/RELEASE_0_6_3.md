# ConsoleCrypt 0.6.3

## Русский

Выпуск доступен для macOS, Windows, Linux и Android. Сборка и проверка iOS отложены.

Kubernetes становится отдельным рабочим разделом: импорт и зашифрованная
синхронизация kubeconfig, живые ресурсы, YAML с проверкой изменений, логи,
shell в Pod, port-forward, Helm, GitOps и ИИ по данным кластера.
Подключения можно передавать проверенным устройствам коллег, отдельно выбирая,
включать ли учётные данные.

Production-контексты защищены ограничениями в ядре. Терминалы `kubectl` и
Flux на компьютере используют локальный прокси с отзывным доступом. На
телефоне доступны native shell, логи и туннели; Android показывает активные
туннели в уведомлении с кнопкой остановки.

Одновременно исправлена кнопка загрузки обновления в уведомлении: действие
открывает существующий процесс загрузки без пустой панели. Исправлено
закрытие терминальной сессии, когда у потока событий нет слушателя.
Повтор reconcile в Flux учитывает изменение версии ресурса контроллером,
проверяя, что объект и его источник остались прежними.

Мобильный RDP-тачпад, читаемый Recovery Kit, импорт `known_hosts`, локальные
терминалы и скачанные локальные модели ИИ сохраняются.

Подробности и ограничения: [руководство Kubernetes](KUBERNETES.md).

## English

This release targets macOS, Windows, Linux and Android. The iOS build and testing are deferred.

Kubernetes becomes a dedicated workspace with kubeconfig import and encrypted
sync, live resources, reviewed YAML editing, logs, Pod shell, port forwarding,
Helm, GitOps and cluster-aware AI. Connections can be shared with verified
colleagues’ devices, with credentials included only by explicit choice.

Production policy is enforced by the native core. Desktop kubectl and Flux
terminals use a revocable local proxy. Mobile supports native Pod shell, logs
and tunnels; Android shows active tunnels in a foreground notification with a
stop action.

This release also fixes the update-notification download button so it opens
the existing download flow without an empty panel, and fixes terminal-session
closure when the event stream has no listener.
Flux reconciliation handles controller updates to resource versions while
checking that the object and its source have not changed.

Mobile RDP touchpad, readable Recovery Kit, known_hosts import, local terminals
and downloaded local AI models are preserved.

See the [Kubernetes guide](KUBERNETES.md) for usage and limitations.
