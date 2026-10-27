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
**Отпечаток ключей моего устройства** доступен в заголовке раздела. Переход между вкладками
или импорт пакета сам по себе не выдаёт доступ.

## Начать совместную работу

1. Коллега входит на тот же сервер.
2. В меню своего хоста или сниппета выберите **Поделиться**. Посмотрите,
   какие именно поля будут отправлены, найдите коллегу по почте и отметьте
   **Дать доступ этому устройству**. Ключи устройства закрепляются на вашем
   устройстве со статусом «Выбран (не проверен)».
3. Нажмите **Проверить** рядом с устройством (клиент 0.9.2 и новее): сверьте
   с коллегой **код безопасности** — 12 групп цифр, вычисленных из ключей
   обоих устройств. Коллега открывает такую же проверку для вашего устройства и
   диктует свой код по телефону или лично, вы вводите его (вставка отключена).
   Лично можно отсканировать **QR-код** с экрана коллеги. Адрес почты и список
   устройств с сервера сами по себе не подтверждают подлинность ключей.
4. Выберите права: **Читатель** по умолчанию; **Редактор** может изменять общий
   элемент. Нажмите **Поделиться** и отправьте очередь.
5. Коллега открывает **Совместный доступ → Доступно мне**, при желании проверяет
   владельца тем же способом и принимает элемент. Получение сниппета никогда не
   запускает команду.

### Проверка ключей коллег (клиент 0.9.2)

- Статусы устройства коллеги: **Выбран (не проверен)**, **Проверен <дата>** и
  **Ключи изменились**. Если сервер сообщает для известного устройства другие
  ключи, новый доступ этому устройству невозможен до повторной проверки;
  существующие элементы для новых ключей не перешифровываются.
- Настройка хранилища **Совместный доступ: только проверенным** (Настройки →
  Хранилище): по умолчанию секреты, kube-доступы и подключения к БД выдаются
  только проверенным устройствам, хосты, группы и сниппеты — и выбранным.
  Политика действует на новые выдачи и новые устройства при изменении доступа;
  у существующих получателей доступ сохраняется.
- После обновления все участники существующих элементов отмечены как «Выбран
  (не проверен)». Один раз показывается список получателей секретов, kube-доступов
  и БД, которых стоит проверить.
- Элементы от непроверенного владельца помечены «Владелец не проверен»: их хосты
  не подключаются автоматически, у сниппетов показан источник.
- Закрепления хранятся в зашифрованной базе профиля на каждом устройстве
  (синхронизация между своими устройствами — в следующих версиях).

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

## RDP-хосты (клиент 0.9.7)

RDP-хостом можно поделиться так же, как SSH-хостом: **Поделиться…** в меню
хоста. В карточку входят адрес, порт, пользователь, домен и метки; заметки и
сохранённый пароль — только по отдельной отметке (пароль — только проверенным
устройствам). Сервер и протокол не меняются, клиенты до 0.9.7 такую карточку
не показывают. Подробнее — [RDP.md](RDP.md).

## Свои устройства

С 0.9.7 устройство, которое сервер выдаёт за ваше, не становится получателем
секрета, подключения Kubernetes или базы данных: свои устройства получают общий
доступ только через подтверждённое подключение («Добавить это устройство»).
Поиск по адресу собственного аккаунта отклоняется.

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
   **Подтвердить устройство** и введите код (6 групп по 5 цифр) с экрана нового
   устройства — сам код на этом шаге не показывается и не подставляется.
4. Верните подтверждённый пакет новому устройству и выберите
   **Другие действия → Продолжить добавление устройства**. Введите код с экрана
   подтверждающего устройства.
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
verification. **My device key fingerprint** remains available in the page header.
Switching tabs or importing a packet does not grant access by itself.

The public server at `https://sync.consolecrypt.dev` enables selective sharing,
collections, individual secrets and verified enrollment of additional devices.
Enabling support does not publish personal items automatically. In client **0.2.0
and later**, use **Sharing → Refresh shared data** after support is enabled or a
connection fails. Connection and authentication errors are shown separately from
unavailable support. Upgrade an older client if its refresh button stays disabled.

Share selected hosts, snippets, collections or individual secrets with verified
devices on the same compatible server. Choosing a colleague's device pins its
keys on your device ("Chosen (not verified)"). In client **0.9.2** and later,
**Verify** compares a 12-group **safety number** computed from both devices'
keys: your colleague opens the same check and reads their number out by phone
or in person, you type it (pasting is disabled); in person you may scan their
**QR code** instead. Other keys reported later for a known device show
**Keys changed** and block new access until verified again. The vault setting
**Sharing: verified recipients only** (on by default for secrets, Kubernetes
contexts and database connections) applies to new access and new devices;
existing recipients keep theirs, and a one-time banner lists unverified
recipients of secrets after the update. Items from an unverified owner are
marked; their hosts never connect automatically. Reader is the default; editors may
change the item. Receiving a snippet never executes it. Your personal vault key
is never shared. Hosts exclude passwords, private keys and personal credential
references; recipients use their own credentials.

Kubernetes contexts and database connections are offered only when the server
supports them; an older server keeps these actions hidden. Credentials are
excluded by default, and a route through a personal SSH host, jump profile,
proxy, tunnel or context is chosen by the recipient when saving a copy.

**RDP hosts (client 0.9.7)** are shared like SSH hosts with **Share…** in the host
menu: address, port, user, domain and tags; the notes and the saved password only
when ticked (the password only to verified devices). Neither the server nor the
protocol changes; clients before 0.9.7 do not show such a card. See RDP.md.

**Your own devices.** From 0.9.7 a device the server presents as yours never
becomes a recipient of a secret, a Kubernetes or database connection: your own
devices get shared items only through a verified enrollment. Looking up your own
account's address is refused.

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
Manual acceptance is the default. The full signed request code (6 groups of 5
digits since 0.9.2) is typed on each device from the other device's screen. The original owner must stay online and unlocked to complete the
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
