# ConsoleCrypt 0.3.1

Косметические правки.

- Увеличены отступы между верхней панелью, заголовками и содержимым страниц. Первые и последние строки больше не скрываются эффектом прокрутки, когда список находится у края.
- Общая кнопка «+» открывает список сохранённых SSH- и RDP-хостов. В списке можно найти подключение по имени или адресу и добавить новый хост.
- В разделе «Удалённый рабочий стол» убрана дублирующая кнопка справа сверху. Кнопка в центре и «+» у вкладок предлагают выбрать сохранённый RDP-хост.

## Известное ограничение RDP

Смена перенаправленной папки или её прав во время RDP-сеанса в отдельных случаях может завершить соединение. Перед изменением сохраните работу на удалённом компьютере; при необходимости подключитесь заново. Это ограничение версии 0.3.0 пока сохраняется.

## English

Cosmetic fixes.

- More space between the toolbar, page headings and content. Scroll fades no longer hide the first or last rows at a list boundary.
- The global “+” opens saved SSH and RDP hosts, with search by name or address and an option to add a host.
- Remote desktop has one central connection action and the tab-strip “+”, both opening saved RDP hosts. The duplicate upper-right button was removed.

Changing a redirected folder or its permissions during an RDP session may terminate the connection in some cases. Save remote work before making changes and reconnect if needed. This known 0.3.0 limitation remains.
