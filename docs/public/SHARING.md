# Общий доступ ConsoleCrypt

Общий доступ относится к выбранным хостам, сниппетам, коллекциям и отдельным
секретам. Ваше личное хранилище и его ключ не передаются коллегам. Участники
должны использовать один сервер синхронизации; он должен поддерживать и
разрешать соответствующие функции. Локальный профиль не отправляет данные.

## Проверить поддержку сервера

В публичном сервере `https://sync.consolecrypt.dev` доступны выбранные хосты,
сниппеты, общие коллекции, отдельные секреты и подтверждённое добавление собственных
устройств. В своём сервере администратор включает эти возможности отдельно:
[настройки Docker Compose и Helm](https://github.com/evsikovas/consolecrypt-server/blob/main/docs/public/HOSTING.md). Включение функции не публикует
личные данные: отправка выбранного элемента требует явного действия владельца.

В клиенте **0.2.0 и новее** нажмите **Совместный доступ → Обновить общие данные**,
чтобы повторно проверить возможности сервера после их включения или сетевой
ошибки. Кнопка доступна для разблокированного профиля синхронизации. Приложение
показывает отдельно ошибки соединения, авторизации и защищённого хранилища ОС;
они не означают, что функция отключена. В старой версии кнопка может быть
неактивна после ранее неудачной проверки — сначала обновите клиент.

## Навигация в клиенте

В **0.2.2 и новее** вкладка **Доступно мне** показывает полученные элементы, а
**Я поделился** — элементы, доступом к которым вы управляете. Блок устройств
расположен отдельно: **Добавить это устройство** запускает запрос с нового
устройства, **Подтвердить устройство** используется на уже проверенном.
**Запросы устройств** открывают ожидающие действия; продолжение добавления и
проверка подписанной истории находятся в **Других действиях**.
**Код моего устройства** доступен в заголовке раздела. Переход между вкладками
или импорт пакета сам по себе не выдаёт доступ.

## Начать совместную работу

1. Коллега входит на тот же сервер и открывает **Совместный доступ → Код моего устройства**.
2. В меню своего хоста или сниппета выберите **Поделиться**. Посмотрите,
   какие именно поля будут отправлены, и выберите устройство коллеги.
3. Сравните весь код устройства лично или по независимому каналу. Отметьте
   подтверждение только при полном совпадении. Адрес почты и список устройств
   с сервера сами по себе не подтверждают подлинность ключей.
4. Выберите права: **Читатель** по умолчанию; **Редактор** может изменять общий
   элемент. Нажмите **Поделиться** и отправьте очередь.
5. Коллега открывает **Совместный доступ → Доступно мне**, сравнивает код владельца
   и принимает элемент. Получение сниппета никогда не запускает команду.

Хост отправляется без пароля, приватного ключа, личных ссылок на учётные данные
и локальных команд. При создании личной копии получатель выбирает свои учётные
данные либо вводит пароль при подключении. Изменённый адрес или SSH-порт общего
хоста нужно отдельно проверить перед новым подключением, включая jump-хосты.

## Коллекции и секреты

Общая коллекция содержит ссылки на явно выбранные общие элементы. Она не
раздаёт права на дочерние объекты: каждый хост, сниппет или секрет имеет свой
состав участников. Недоступный дочерний элемент показывается закрытым.
Добавление личного хоста в личную группу не публикует его автоматически.

Секрет передаётся отдельным действием в учётных данных с предупреждением и
подтверждением. Значение скрыто до **Показать** и очищается из экрана по таймеру,
при блокировке и закрытии. Копирование повторно проверяет доступ на сервере.
Парольная фраза SSH-ключа передаётся отдельно; она не прикрепляется к ключу
автоматически. Секреты не попадают в ИИ или в общий просмотр метаданных.

## Подключения Kubernetes и баз данных

Контексты Kubernetes и подключения к базам данных передаются только при
поддержке сервера; старый сервер скрывает эти действия. Учётные данные по
умолчанию не передаются, а маршрут через личный SSH-хост, профиль переходов,
прокси, туннель или контекст получатель выбирает свой при сохранении копии.
Подробности — в разделах Kubernetes и «Базы данных».

## Общие заметки

Если сервер поддерживает общие заметки (`supports_notes_v1`), в правой панели
«Заметки» в режиме «Объект» у общего хоста, базы данных или контекста Kubernetes
появляется блок «Общие заметки». Их видят все участники объекта; писать может
любой участник, изменить заметку — только её автор, удалить — автор или
владелец. Заметка к Docker-контейнеру общего SSH-хоста или к ресурсу общего
контекста видна только у этого контейнера или ресурса. Задачи и напоминания
остаются личными.

Заметки шифруются на устройстве. Ключ заметок выдаёт устройство владельца и
подписывает его; сервер не может подменить ключ или прочитать текст. Пока
владелец ни разу не открыл панель заметок для объекта, участники видят
сообщение об ожидании. После исключения участника устройство владельца при
следующем открытии панели перешифровывает заметки новым ключом; исключённый
участник новые заметки больше не получает.

## Несколько устройств коллеги

Личное рабочее место синхронизируется обычным подтверждением устройств. Доступ
к общим элементам проверяется отдельно. Владелец исходного общего элемента
открывает **Новые собственные устройства** и выбирает уже проверенное устройство
коллеги, максимальные права, срок и число новых устройств. Ручной режим выбран
по умолчанию. Автоматический режим нужно включить явно; владелец должен
оставаться онлайн с разблокированным приложением.

1. Передайте публичный пакет разрешения на новое устройство коллеги.
2. Там выберите **Добавить это устройство**, откройте пакет и создайте запрос.
3. Передайте запрос на уже проверенное устройство коллеги, выберите
   **Подтвердить устройство** и сравните полный код на обоих устройствах.
4. Верните подтверждённый пакет новому устройству и выберите
   **Другие действия → Продолжить добавление устройства**. Повторно подтвердите совпадение кода.
5. Владелец начинает проверку, новое устройство отвечает из **Запросов устройств**,
   затем владелец принимает запрос. В автоматическом режиме приложение владельца
   выполняет эти действия для уже подтверждённого запроса. Проверка кода остаётся
   обязательной в обоих режимах.

Пакеты содержат публичные ключи и подписанные подтверждения, без паролей,
приватных ключей и содержимого элемента. Импорт пакета не выдаёт доступ.
Новое устройство не может повысить свою роль или пригласить другого пользователя.
После перезапуска владельца может потребоваться новая проверка владения ключом.
Отключение разрешения сохраняется локально даже при недоступном сервере.

## Удаление, выход и восстановление

Удалить общий элемент может только его владелец: начиная с сервера **0.1.17**
сервер отклоняет удаление от редактора (`owner_only`), в том числе от старых
клиентов, поэтому данные владельца не теряются. Получатели теряют доступ сразу.
Сервер хранит последнее зашифрованное содержимое удалённого элемента
**30 дней**: в клиенте **0.8.7** и новее откройте **Я поделился → Недавно
удалённые** и нажмите **Восстановить**. Элемент публикуется заново с теми же
устройствами, кроме вышедших из доступа; получатели снова принимают его, сравнив
код владельца. Через 30 дней остаётся только подписанная отметка удаления.
Удалённые элементы в пределах этого срока учитываются в лимите общих элементов.

Получатель может в любой момент выбрать **Выйти из общего доступа** в карточке
элемента или во входящем приглашении. Все его устройства сразу теряют доступ на
сервере, элемент исчезает из списка, а сохранённые личные копии остаются.
Владелец видит, что участник вышел; при следующем изменении прав этот участник
исключается из подписанного списка. Повторно добавить его можно только новым
приглашением. Эти действия доступны, если сервер сообщает о поддержке; на
старом сервере кнопки скрыты.

## Отзыв, конфликты и восстановление

В **Я поделился → Управление доступом** исходное устройство владельца может
изменить права и удалить устройство. Изменение участников создаёт новый ключ
шифрования и новую версию элемента. Уже прочитанные данные и личные копии
отозвать нельзя. Если переданный пароль или SSH-ключ больше не должен работать,
смените его также на целевом сервере.

Сетевая ошибка оставляет подписанное изменение в отдельной очереди. Просмотрите
очередь и отправьте её повторно. Конфликт прав или версии блокирует старое
изменение; приложение не переносит его молча в новую эпоху. Удаление записи из
очереди не отменяет изменение, уже принятое сервером.

Контрольные отметки защищённого хранилища ОС обнаруживают восстановление старой
локальной базы. Для восстановления используйте явную сверку с владельцем и
проверку подписанной истории. Не удаляйте Keychain или системные ключи, чтобы
обойти блокировку. Сервер способен задерживать новые состояния; клиент
обнаруживает откат ниже уже наблюдавшегося состояния, но не обещает мгновенную
глобальную свежесть при недоступном или недобросовестном сервере.

Если сервер вернул повреждённую запись или запись не прошла проверку подписей
и шифрования, она скрывается, а остальной список работает; над списком
появляется «N записей не удалось прочитать».

## English

In **0.2.2 and later**, **Available to me** lists received items and
**Shared by me** lists items whose access you manage. A separate device card
offers **Add this device**, **Confirm another device** and **Device requests**.
**More actions** contains **Continue adding a device** and signed history
verification. **My device verification code** remains available in the page header.
Switching tabs or importing a packet does not grant access by itself.

The public server at `https://sync.consolecrypt.dev` enables selective sharing,
collections, individual secrets and verified enrollment of additional devices.
Enabling support does not publish personal items automatically. In client **0.2.0
and later**, use **Sharing → Refresh shared data** after support is enabled or a
connection fails. Connection and authentication errors are shown separately from
unavailable support. Upgrade an older client if its refresh button stays disabled.

Share selected hosts, snippets, collections or individual secrets with verified
devices on the same compatible server. Compare the entire device code over an
independent channel before approving it. Reader is the default; editors may
change the item. Receiving a snippet never executes it. Your personal vault key
is never shared. Hosts exclude passwords, private keys and personal credential
references; recipients use their own credentials.

Kubernetes contexts and database connections are offered only when the server
supports them; an older server keeps these actions hidden. Credentials are
excluded by default, and a route through a personal SSH host, jump profile,
proxy, tunnel or context is chosen by the recipient when saving a copy.

Shared notes appear under **Notes → Object** for a shared host, database
connection or Kubernetes context when the server supports them
(`supports_notes_v1`). Every member reads them; any member writes, only the
author edits, the author or the owner deletes. A note about a Docker container
of a shared SSH host or a resource of a shared context stays with that container
or resource. Tasks and reminders remain personal. Notes are encrypted on the
device under a notes key that the owner device issues and signs, so the server
can neither read them nor substitute the key. Members wait until the owner has
opened the notes panel for the object once. After a member is removed, the
owner's device moves the notes to a new key the next time the panel opens.

Collections reference independently shared items and do not grant inherited
access. Secret sharing requires a separate explicit confirmation. Values remain
masked until revealed; copying rechecks current access. An SSH key's passphrase
is shared separately and is never attached automatically.

An item's original owner may explicitly allow a verified colleague's device to
endorse that colleague's new devices, within a role ceiling, expiry and quota.
Manual acceptance is the default. Both devices must compare the full signed
request code. The original owner must stay online and unlocked to complete the
encrypted possession challenge and rotate the item key. Optional automatic
acceptance processes only already endorsed requests covered by an explicit
Automatic grant. Public pairing packets contain no private keys or item values;
import alone does not establish trust.

Only the owner can delete a shared item: from server **0.1.17** an editor's
delete is rejected (`owner_only`), including from older clients, so the owner's
data is never lost. Recipients lose access immediately. The server keeps the
last encrypted content of a deleted item for **30 days**; in client **0.8.7**
and later use **Shared by me → Recently deleted → Restore**. The item is shared
again with the same devices except those who left, and recipients accept it
again after comparing the owner code. Restorable items count toward the shared
item quota. A recipient can **Leave share** from the item or an incoming
invitation at any time: all their devices lose server access, the item leaves
their list and saved personal copies stay. The owner sees that the member left;
the next access change drops them from the signed manifest. Both actions appear
only when the server advertises support.

Revocation prevents access to future keys and server requests. It cannot erase
data already read or copied, or rotate credentials on the destination SSH server.
Queued conflicts and restored older local databases fail closed until explicitly
reviewed. Keep your encrypted backups and recovery kit, and do not reset OS
secure-storage keys to bypass a verification error.

A shared entry that arrives malformed or fails signature or encryption checks
is hidden while the rest of the list keeps working; the list then shows how
many entries could not be read.
