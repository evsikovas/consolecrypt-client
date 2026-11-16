# Карантинная распаковка dependency-архивов

Подготовительный компонент будущего установщика ANS-05. Сейчас он **не
подключён к UI, RPC или загрузке по сети**, capability установки не объявляется.
Проверка vendored lock уже работает отдельно; этот модуль ещё не делает её
полноценным установщиком и не добавляет автоматическую обработку архивов проекта.

`runner/src/consolecrypt_runner/dependency_archive.py` получает неизменяемые
gzip/tar-байты, ожидаемый SHA-256 и новый каталог карантина. Его родитель должен
быть приватным и принадлежать non-root пользователю Unix worker. Caller задаёт
явный `strip_prefix`; архив не выбирает назначения или полномочия.

До создания staging проверяются SHA-256, compressed budget и отмена. Поток gzip
полностью распаковывается в ограниченный образ до tar parsing: это ограничивает
также PAX/GNU metadata и проверяет CRC/EOF. После чтения tar проверяется настоящий
нулевой конец, чтобы терпимое завершение stdlib на повреждённом позднем header
не стало успешной частичной установкой. `extractall` и восстановление владельца,
mtime, setuid/setgid и специальных файлов не используются. Справка stdlib:
[Python 3.12 tarfile](https://docs.python.org/3.12/library/tarfile.html#hints-for-further-verification).

Поддерживаются обычные файлы и каталоги. Абсолютные/непереносимые пути,
traversal, symlink/hardlink, FIFO/device, sparse, повторённые пути, конфликт
файла с каталогом, case/Unicode aliases и подмена Git provenance отклоняются.
Открытие всех назначений — через directory FD от нового корня, `O_NOFOLLOW`
и exclusive creation. Существующая директория никогда не заменяется. Mode
нормализуется в 0600/0700; исполняемые файлы не запускаются как install hooks.

Предельные бюджеты: 32 МиБ compressed, 64 МиБ содержимого, 72 МиБ tar stream
с headers/padding, 8 МиБ на файл, 4096 реальных/созданных entries. Caller может
только уменьшить эти значения. Остаются внешние лимиты процесса/памяти, timeout,
отмена и ограничения namespace worker; library не заменяет sandbox.

Возвращается только `ProjectSnapshot` готового дерева. Ошибка/отмена оставляет
приватный непубликованный карантин для cleanup caller. Перед публикацией или
копированием caller обязан повторно сверить дерево, общий project budget и
актуальность одобрения. Этот компонент не публикует файлы в пользовательский
проект, не удаляет его данные и не подтверждает доверие к сетевому издателю.

Проверены Unicode/PAX, binary/CRLF, пустые каталоги, portable modes, независимый
snapshot, traversal/links/devices, aliases, квоты, gzip bomb, повреждённый поздний
header, trailing archive, отмена и запрет замены существующих данных. Отдельный
реальный namespace-тест подтвердил отсутствие host writes; журналы основного
workspace: `automation-artifact-staging-real-tests.log` и
`automation-archive-real-namespace-test.log`.
