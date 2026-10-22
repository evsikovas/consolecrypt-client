// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Russian (`ru`).
class AppLocalizationsRu extends AppLocalizations {
  AppLocalizationsRu([String locale = 'ru']) : super(locale);

  @override
  String get commonCancel => 'Отмена';

  @override
  String get commonSave => 'Сохранить';

  @override
  String get commonDelete => 'Удалить';

  @override
  String get commonEdit => 'Изменить';

  @override
  String get commonRename => 'Переименовать';

  @override
  String get commonClose => 'Закрыть';

  @override
  String get commonCopy => 'Копировать';

  @override
  String get commonBack => 'Назад';

  @override
  String get commonNext => 'Далее';

  @override
  String get commonContinue => 'Продолжить';

  @override
  String get commonDone => 'Готово';

  @override
  String get commonRetry => 'Повторить';

  @override
  String get commonAdd => 'Добавить';

  @override
  String get commonCreate => 'Создать';

  @override
  String get commonOpen => 'Открыть';

  @override
  String get commonRemove => 'Удалить';

  @override
  String get commonChange => 'Изменить';

  @override
  String get commonShow => 'Показать';

  @override
  String get commonHide => 'Скрыть';

  @override
  String get commonSearch => 'Поиск';

  @override
  String get commonOk => 'ОК';

  @override
  String get commonReview => 'Просмотреть';

  @override
  String get commonConnect => 'Подключиться';

  @override
  String get commonNever => 'Никогда';

  @override
  String get commonNone => 'Нет';

  @override
  String get commonUnknown => 'Неизвестно';

  @override
  String get commonName => 'Имя';

  @override
  String get commonDescription => 'Описание';

  @override
  String get commonOptional => 'Необязательно';

  @override
  String get tagsLabel => 'Теги';

  @override
  String get tagsHint => 'Введите тег и нажмите Enter';

  @override
  String get tagsAdd => 'Добавить тег';

  @override
  String copiedNotice(String what) {
    return 'Скопировано: $what.';
  }

  @override
  String copiedSecretNotice(String what, int seconds) {
    return 'Скопировано: $what. Буфер обмена будет очищен через $seconds с.';
  }

  @override
  String get copyWhatSecret => 'секрет';

  @override
  String get copyWhatText => 'текст';

  @override
  String get copyWhatCommand => 'команда';

  @override
  String get copyWhatPublicKey => 'открытый ключ';

  @override
  String get copyWhatPassword => 'пароль';

  @override
  String get copyWhatRecoveryWords => 'слова восстановления';

  @override
  String get copyWhatFingerprint => 'отпечаток';

  @override
  String get timeJustNow => 'только что';

  @override
  String timeMinutesAgo(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count мин назад',
      many: '$count мин назад',
      few: '$count мин назад',
      one: '$count мин назад',
    );
    return '$_temp0';
  }

  @override
  String timeHoursAgo(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count ч назад',
      many: '$count ч назад',
      few: '$count ч назад',
      one: '$count ч назад',
    );
    return '$_temp0';
  }

  @override
  String timeDaysAgo(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count дня назад',
      many: '$count дней назад',
      few: '$count дня назад',
      one: '$count день назад',
    );
    return '$_temp0';
  }

  @override
  String get timeExpired => 'срок истёк';

  @override
  String get timeInLessThanMinute => 'менее чем через минуту';

  @override
  String timeInMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'через $count мин',
      many: 'через $count мин',
      few: 'через $count мин',
      one: 'через $count мин',
    );
    return '$_temp0';
  }

  @override
  String timeInHours(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'через $count ч',
      many: 'через $count ч',
      few: 'через $count ч',
      one: 'через $count ч',
    );
    return '$_temp0';
  }

  @override
  String timeInDays(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'через $count дня',
      many: 'через $count дней',
      few: 'через $count дня',
      one: 'через $count день',
    );
    return '$_temp0';
  }

  @override
  String get unitBytes => 'Б';

  @override
  String get unitKibibytes => 'КиБ';

  @override
  String get unitMebibytes => 'МиБ';

  @override
  String get unitGibibytes => 'ГиБ';

  @override
  String get unitTebibytes => 'ТиБ';

  @override
  String formatSize(String value, String unit) {
    return '$value $unit';
  }

  @override
  String get languageLabel => 'Язык';

  @override
  String get languageSystem => 'Как в системе';

  @override
  String languageSystemResolved(String language) {
    return 'Как в системе ($language)';
  }

  @override
  String get languageSwitcherTooltip => 'Язык / Language';

  @override
  String get languageSettingsHelp =>
      'Применяется сразу. «Как в системе» следует языку ОС; если он не поддерживается — английский.';

  @override
  String get riskReadOnly => 'Только чтение';

  @override
  String get riskModifying => 'Изменяющая';

  @override
  String get riskDestructive => 'Разрушительная';

  @override
  String get riskUnknown => 'Неизвестно';

  @override
  String riskSemantics(String level) {
    return 'Риск: $level';
  }

  @override
  String verificationCodeSemantics(String groups) {
    return 'Код проверки $groups';
  }

  @override
  String get strengthVeryWeak => 'Очень слабая';

  @override
  String get strengthWeak => 'Слабая';

  @override
  String get strengthFair => 'Средняя';

  @override
  String get strengthStrong => 'Надёжная';

  @override
  String get strengthVeryStrong => 'Очень надёжная';

  @override
  String strengthLabelWithHint(String label, String hint) {
    return '$label — $hint';
  }

  @override
  String strengthHintEmpty(int min) {
    return 'Используйте не менее 4 случайных слов или от $min символов';
  }

  @override
  String get strengthHintSequence => 'Избегайте последовательностей клавиш и букв алфавита';

  @override
  String get strengthHintCommon => 'Избегайте распространённых паролей и известных фраз';

  @override
  String strengthHintTooShort(int min) {
    return 'Используйте не менее $min символов';
  }

  @override
  String get strengthHintGood => 'Хорошо. Храните её в надёжном месте — сбросить её за вас никто не сможет.';

  @override
  String get strengthHintAddMore => 'Добавьте ещё случайных слов или символов';

  @override
  String get backupFrequencyDaily => 'Ежедневно';

  @override
  String get backupFrequencyWeekly => 'Еженедельно';

  @override
  String get aiProviderKindOpenaiCompatible => 'OpenAI-совместимый';

  @override
  String get privacyProfileStrict => 'Строгий';

  @override
  String get privacyProfileStandard => 'Стандартный';

  @override
  String get privacyProfileLocal => 'Локальный';

  @override
  String get privacyProfileStrictDescription => 'Скрывать секреты, а также IP-адреса, имена хостов, пользователей и БД';

  @override
  String get privacyProfileStandardDescription => 'Скрывать секреты, сохранять метаданные хостов';

  @override
  String get privacyProfileLocalDescription =>
      'Для локальных моделей: больше контекста; секреты по-прежнему скрываются';

  @override
  String get credentialKindPassword => 'Пароль';

  @override
  String get credentialKindSshKey => 'SSH-ключ';

  @override
  String get credentialKindSshCertificate => 'SSH-сертификат';

  @override
  String get credentialKindOsSshAgent => 'SSH-агент ОС';

  @override
  String get credentialKindFido2 => 'Ключ безопасности FIDO2';

  @override
  String get credentialKindExternalAgent => 'Внешний агент';

  @override
  String get hostKeyPolicyAsk => 'Спрашивать';

  @override
  String get hostKeyPolicyStrict => 'Строгая';

  @override
  String get hostKeyPolicyAcceptNew => 'Принимать новые';

  @override
  String get hostKeyPolicyAskDescription =>
      'Подтверждать неизвестные ключи хостов; изменённый ключ всегда блокирует подключение';

  @override
  String get hostKeyPolicyStrictDescription => 'Принимать только ключи из списка известных хостов';

  @override
  String get hostKeyPolicyAcceptNewDescription =>
      'Автоматически сохранять неизвестные ключи; изменённый ключ всегда блокирует подключение';

  @override
  String get sshBackendNative => 'Встроенный (russh)';

  @override
  String get sshBackendOpenSsh => 'Системный OpenSSH (совместимость)';

  @override
  String get knownHostSourceTofu => 'Принят при первом подключении';

  @override
  String get knownHostSourceManual => 'Добавлен вручную';

  @override
  String get knownHostSourceImported => 'Импортирован из known_hosts';

  @override
  String get knownHostSourceCertAuthority => 'Центр сертификации';

  @override
  String get historyModeLocalOnly => 'Только локально';

  @override
  String get historyModeEncryptedSync => 'Зашифрованная синхронизация';

  @override
  String get historyModeDisabled => 'Отключена';

  @override
  String get historyModeLocalOnlyDescription => 'История хранится только на этом устройстве (по умолчанию)';

  @override
  String get historyModeEncryptedSyncDescription => 'История синхронизируется со сквозным шифрованием';

  @override
  String get historyModeDisabledDescription => 'История команд не записывается';

  @override
  String get profileKindLocal => 'Только локально';

  @override
  String get profileKindSynced => 'Синхронизируется';

  @override
  String profileSyncedSubtitle(String account, String server) {
    return '$account · $server';
  }

  @override
  String get profileUnknownAccount => 'аккаунт';

  @override
  String get profileUnknownServer => 'сервер';

  @override
  String get enableSyncStepAuthenticating => 'Вход и регистрация этого устройства';

  @override
  String get enableSyncStepCreatingRemoteVault => 'Создание хранилища на сервере (тот же ID хранилища)';

  @override
  String get enableSyncStepReconnecting => 'Хранилище уже есть на сервере — объединение';

  @override
  String get enableSyncStepUploading => 'Выгрузка зашифрованных объектов';

  @override
  String get enableSyncStepFinishing => 'Завершение';

  @override
  String get enableSyncStepDone => 'Синхронизация включена';

  @override
  String get enableSyncStepFailed => 'Ошибка';

  @override
  String get tunnelKindLocal => 'Локальный';

  @override
  String get tunnelKindRemote => 'Удалённый';

  @override
  String get tunnelKindDynamic => 'Динамический (SOCKS5)';

  @override
  String get tunnelKindLocalDescription => 'Пробросить локальный порт на хост, доступный с сервера';

  @override
  String get tunnelKindRemoteDescription => 'Открыть локальный сервис на порту сервера';

  @override
  String get tunnelKindDynamicDescription => 'Локальный SOCKS5-прокси через сервер';

  @override
  String get snippetSourceUser => 'Пользователь';

  @override
  String get snippetSourceAi => 'ИИ';

  @override
  String get snippetSourceImported => 'Импорт';

  @override
  String get snippetSourceHistory => 'История';

  @override
  String get platformSystemAuthentication => 'системная аутентификация';

  @override
  String get platformDeviceNameMac => 'Мой Mac';

  @override
  String get platformDeviceNamePc => 'Мой ПК';

  @override
  String get platformDeviceNameOther => 'Это устройство';

  @override
  String get commandPalette => 'Палитра команд';

  @override
  String get commandNewTerminalTab => 'Новая вкладка терминала';

  @override
  String get commandCloseTerminalTab => 'Закрыть вкладку терминала';

  @override
  String get commandNewHost => 'Новый хост';

  @override
  String get commandLockVault => 'Заблокировать хранилище';

  @override
  String get commandOpenSettings => 'Настройки';

  @override
  String get commandNewTabPickerTitle => 'Новая вкладка терминала';

  @override
  String get menuFile => 'Файл';

  @override
  String get menuView => 'Вид';

  @override
  String get menuWindow => 'Окно';

  @override
  String get navHosts => 'Хосты';

  @override
  String get navGroups => 'Группы';

  @override
  String get navCredentials => 'Учётные данные';

  @override
  String get navTerminal => 'Терминал';

  @override
  String get navSftp => 'SFTP';

  @override
  String get navTunnels => 'Туннели';

  @override
  String get navSnippets => 'Сниппеты';

  @override
  String get notesShared => 'Общие заметки';

  @override
  String get notesSharedRefresh => 'Обновить общие заметки';

  @override
  String get notesSharedNew => 'Новая общая заметка';

  @override
  String get notesSharedFailed => 'Не удалось загрузить общие заметки. Проверьте подключение к серверу синхронизации.';

  @override
  String get notesSharedWaiting =>
      'Владелец ещё не открыл общие заметки для этого объекта. Они появятся, когда он откроет панель заметок.';

  @override
  String notesSharedPending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count заметки станут доступны, когда владелец откроет панель заметок',
      many: '$count заметок станут доступны, когда владелец откроет панель заметок',
      few: '$count заметки станут доступны, когда владелец откроет панель заметок',
      one: '$count заметка станет доступна, когда владелец откроет панель заметок',
    );
    return '$_temp0';
  }

  @override
  String get notesSharedEmpty => 'Общих заметок пока нет.';

  @override
  String get notesSharedHint => 'Заметку видят все, с кем вы поделились объектом. Изменить её можете только вы.';

  @override
  String get notesSharedReadOnly => 'Изменить заметку может только автор.';

  @override
  String notesSharedDeleteMessage(String title) {
    return 'Заметка «$title» будет удалена у всех, у кого есть доступ к объекту.';
  }

  @override
  String get notesAuthorYou => 'Вы';

  @override
  String get notesAuthorOwner => 'Владелец';

  @override
  String get notesAuthorMember => 'Участник';

  @override
  String get navNotes => 'Заметки';

  @override
  String get notesThisObject => 'Объект';

  @override
  String get notesAll => 'Все';

  @override
  String get notesTasks => 'Задачи';

  @override
  String get notesSearchHint => 'Поиск по заметкам';

  @override
  String get notesNew => 'Новая заметка';

  @override
  String get notesEmptyTitle => 'Заметок пока нет';

  @override
  String get notesEmptyMessage =>
      'Записывайте задачи, напоминания и всё, что важно помнить о серверах, базах и контейнерах.';

  @override
  String notesEmptyObject(String name) {
    return 'О «$name» пока ничего нет';
  }

  @override
  String get notesNothingFound => 'Ничего не найдено';

  @override
  String notesCompleted(int count) {
    return 'Выполненные ($count)';
  }

  @override
  String get notesTitle => 'Заголовок';

  @override
  String get notesBody => 'Текст (Markdown)';

  @override
  String get notesBodyEmpty => 'Текста нет';

  @override
  String get notesEdit => 'Правка';

  @override
  String get notesPreview => 'Просмотр';

  @override
  String get notesTags => 'Метки через запятую';

  @override
  String get notesTask => 'Задача';

  @override
  String get notesDue => 'Срок';

  @override
  String get notesDone => 'Выполнено';

  @override
  String get notesReminder => 'Напоминание';

  @override
  String get notesAttachedTo => 'Привязка';

  @override
  String get notesNoTarget => 'Без привязки';

  @override
  String get notesTargetMissing => 'Объект не найден';

  @override
  String get notesWhenNone => 'Нет';

  @override
  String get notesWhenInHour => 'Через час';

  @override
  String get notesWhenThisEvening => 'Сегодня в 18:00';

  @override
  String get notesWhenTomorrow => 'Завтра в 09:00';

  @override
  String get notesWhenNextWeek => 'Через неделю';

  @override
  String get notesWhenCustom => 'Дата и время…';

  @override
  String get notesWhenCustomHint => 'ГГГГ-ММ-ДД ЧЧ:ММ';

  @override
  String get notesInvalidTime => 'Введите дату в виде ГГГГ-ММ-ДД ЧЧ:ММ';

  @override
  String notesOverdueAt(String when) {
    return 'Просрочено · $when';
  }

  @override
  String get notesPin => 'Закрепить';

  @override
  String get notesUnpin => 'Открепить';

  @override
  String get notesSnooze => 'Напомнить через час';

  @override
  String notesAttachHere(String name) {
    return 'Привязать к «$name»';
  }

  @override
  String get notesDetach => 'Отвязать';

  @override
  String get notesDelete => 'Удалить заметку';

  @override
  String notesDeleteMessage(String title) {
    return 'Заметка «$title» будет удалена на всех ваших устройствах.';
  }

  @override
  String get notesMarkDone => 'Отметить выполненной';

  @override
  String get notesMarkOpen => 'Снять отметку о выполнении';

  @override
  String get notesBack => 'К списку заметок';

  @override
  String get notesSaveFailed => 'Не сохранено';

  @override
  String notesReminderToast(String title) {
    return 'Напоминание: $title';
  }

  @override
  String get notesOpen => 'Открыть';

  @override
  String get notesHostNotesTitle => 'Заметка из карточки хоста';

  @override
  String get notesHostNotesMove => 'Перенести в заметки';

  @override
  String notesFromHostTitle(String name) {
    return 'Заметки: $name';
  }

  @override
  String get navAiChat => 'ИИ-чат';

  @override
  String get navDevices => 'Устройства';

  @override
  String get navSync => 'Синхронизация';

  @override
  String get navBackups => 'Резервные копии';

  @override
  String get navSettings => 'Настройки';

  @override
  String shellLockVaultTooltip(String shortcut) {
    return 'Заблокировать хранилище ($shortcut)';
  }

  @override
  String get shellSearchPlaceholder => 'Поиск хостов, сниппетов или вопрос ИИ…';

  @override
  String get shellOfflineTooltip =>
      'Сервер синхронизации недоступен. Хосты, ключи и SSH продолжают работать; изменения поставлены в очередь.';

  @override
  String shellNewTabTooltip(String shortcut) {
    return 'Новая вкладка терминала ($shortcut)';
  }

  @override
  String shellApprovalBannerTitle(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count запроса на подтверждение устройств',
      many: '$count запросов на подтверждение устройств',
      few: '$count запроса на подтверждение устройств',
      one: '$count запрос на подтверждение устройства',
    );
    return '$_temp0';
  }

  @override
  String shellApprovalBannerMessage(String device, String platform, String ago) {
    return 'Устройство «$device» ($platform) запросило доступ к вашему хранилищу $ago. Одобряйте только после сравнения кодов проверки.';
  }

  @override
  String get syncStateLocalOnly => 'Только локально';

  @override
  String get syncStateSynced => 'Синхронизировано';

  @override
  String get syncStateSyncedShort => 'Синх.';

  @override
  String syncStateSyncedAgo(String ago) {
    return 'Синхронизировано $ago';
  }

  @override
  String get syncStateSyncing => 'Синхронизация…';

  @override
  String get syncStateOffline => 'Офлайн';

  @override
  String syncStateOfflinePending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Офлайн · $count в очереди',
      many: 'Офлайн · $count в очереди',
      few: 'Офлайн · $count в очереди',
      one: 'Офлайн · $count в очереди',
    );
    return '$_temp0';
  }

  @override
  String get syncStateError => 'Ошибка синхронизации';

  @override
  String get syncStatePaused => 'Синхронизация приостановлена';

  @override
  String get syncIndicatorLocalTooltip => 'Этот профиль не синхронизируется. Включите синхронизацию в настройках.';

  @override
  String syncIndicatorPendingTooltip(String state, int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$state — $count локального изменения ожидают отправки',
      many: '$state — $count локальных изменений ожидают отправки',
      few: '$state — $count локальных изменения ожидают отправки',
      one: '$state — $count локальное изменение ожидает отправки',
    );
    return '$_temp0';
  }

  @override
  String get errorUnknown => 'Что-то пошло не так. Попробуйте ещё раз.';

  @override
  String get errorInvalidCredentials => 'Неверный e-mail или пароль.';

  @override
  String get errorServerUnreachable => 'Сервер недоступен. Проверьте URL и подключение к сети.';

  @override
  String get errorIncompatibleServer => 'Версия сервера несовместима с приложением. Обновите приложение или сервер.';

  @override
  String get errorRegistrationClosed => 'Регистрация на этом сервере закрыта.';

  @override
  String get errorEmailTaken => 'Аккаунт с таким e-mail уже существует.';

  @override
  String get errorWrongPassphrase => 'Неверная парольная фраза.';

  @override
  String get errorInvalidRecoveryKey => 'Этот ключ восстановления не открывает хранилище.';

  @override
  String get errorDeviceNotTrusted => 'Это устройство ещё не является доверенным для хранилища.';

  @override
  String get errorOsAuthFailed =>
      'Системная аутентификация не пройдена или отменена. Разблокируйте хранилище парольной фразой.';

  @override
  String get errorVerificationMismatch => 'Коды проверки не совпадают — подтверждение отклонено.';

  @override
  String get errorRequestExpired => 'Срок действия запроса истёк.';

  @override
  String get errorValidation => 'Проверьте введённые значения.';

  @override
  String get errorNotFound => 'Не найдено. Возможно, объект был удалён.';

  @override
  String get errorConflict => 'Объект был изменён в другом месте. Обновите данные и повторите попытку.';

  @override
  String get errorOffline => 'Нет подключения. Изменение будет повторено после восстановления связи.';

  @override
  String get errorCancelled => 'Отменено.';

  @override
  String get errorHostKeyRejected => 'Ключ хоста отклонён.';

  @override
  String get errorHostKeyChanged => 'Ключ хоста изменился. Подключение заблокировано.';

  @override
  String get errorAuthFailed => 'Ошибка SSH-аутентификации.';

  @override
  String get errorUnsupported => 'Это действие здесь не поддерживается.';

  @override
  String get errorInternal => 'Внутренняя ошибка. Попробуйте ещё раз.';

  @override
  String get errorSessionExpired => 'Сеанс истёк. Войдите снова.';

  @override
  String get errorDeviceRevoked => 'Это устройство отозвано. Войдите снова, чтобы зарегистрировать его.';

  @override
  String get errorEmailNotVerified => 'Сначала подтвердите адрес e-mail.';

  @override
  String get errorForbidden => 'Нет доступа.';

  @override
  String get errorRateLimited => 'Слишком много попыток. Повторите позже.';

  @override
  String errorRateLimitedRetry(int seconds) {
    String _temp0 = intl.Intl.pluralLogic(
      seconds,
      locale: localeName,
      other: 'Слишком много попыток. Повторите через $seconds с.',
      many: 'Слишком много попыток. Повторите через $seconds с.',
      few: 'Слишком много попыток. Повторите через $seconds с.',
      one: 'Слишком много попыток. Повторите через $seconds с.',
    );
    return '$_temp0';
  }

  @override
  String get errorPayloadTooLarge => 'Данные слишком велики для сервера.';

  @override
  String get errorInvalidProof => 'Криптографическая проверка не пройдена. Запрос отклонён.';

  @override
  String get errorServerUnavailable => 'Сервер временно недоступен. Повторите позже.';

  @override
  String get errorReasonNameRequired => 'Введите имя.';

  @override
  String get errorReasonProfileNameRequired => 'Введите имя профиля.';

  @override
  String get errorReasonPasswordRequired => 'Введите пароль.';

  @override
  String get errorReasonKeyPassphraseRequired => 'Введите парольную фразу ключа.';

  @override
  String get errorReasonAgentPathRequired => 'Укажите путь к сокету агента или имя канала (pipe).';

  @override
  String get errorReasonInvalidEmail => 'Введите корректный адрес e-mail.';

  @override
  String errorReasonAccountPasswordTooShort(int min) {
    String _temp0 = intl.Intl.pluralLogic(
      min,
      locale: localeName,
      other: 'Пароль аккаунта должен содержать не менее $min символа.',
      many: 'Пароль аккаунта должен содержать не менее $min символов.',
      few: 'Пароль аккаунта должен содержать не менее $min символов.',
      one: 'Пароль аккаунта должен содержать не менее $min символа.',
    );
    return '$_temp0';
  }

  @override
  String get errorReasonWeakPassphrase => 'Выберите более надёжную парольную фразу.';

  @override
  String get errorReasonInvalidBaseUrl => 'Введите корректный базовый URL.';

  @override
  String get errorReasonChatModelRequired => 'Введите имя модели для чата.';

  @override
  String get errorReasonTemplateRequired => 'Введите шаблон команды.';

  @override
  String errorReasonMissingVariables(String names) {
    return 'Заполните: $names';
  }

  @override
  String get errorReasonInvalidKey => 'Это не корректный закрытый ключ.';

  @override
  String get errorReasonNotACertificate => 'Это не строка сертификата OpenSSH.';

  @override
  String get errorReasonNotAgentCredential => 'Это не учётные данные агента.';

  @override
  String get errorReasonKeyHasNoPassphrase => 'У этого ключа нет парольной фразы.';

  @override
  String get errorReasonUnsupportedKeyAlgorithm => 'Генерируйте ключи Ed25519 или RSA 3072/4096.';

  @override
  String get errorReasonGroupCycle => 'Группа не может находиться внутри самой себя.';

  @override
  String get errorReasonJumpChainEmpty => 'Профилю jump-хостов нужен хотя бы один узел.';

  @override
  String get errorReasonHostNotFound => 'Хост не найден.';

  @override
  String get errorReasonCredentialNotFound => 'Учётные данные больше не существуют.';

  @override
  String get errorReasonProfileNotFound => 'Профиль не найден.';

  @override
  String get errorReasonDeviceNotFound => 'Устройство не найдено.';

  @override
  String get errorReasonTunnelNotFound => 'Туннель не найден.';

  @override
  String get errorReasonProviderNotFound => 'ИИ-провайдер не найден.';

  @override
  String get errorReasonRequestNotFound => 'Запрос больше не существует.';

  @override
  String get errorReasonSecretNotStored => 'Для этих учётных данных секрет не сохранён.';

  @override
  String get errorReasonSessionClosed => 'Сеанс закрыт.';

  @override
  String errorReasonDirectoryNotFound(String path) {
    return 'Каталог не найден: $path';
  }

  @override
  String get errorReasonDirectoryNotEmpty => 'Каталог не пуст.';

  @override
  String errorReasonAlreadyExists(String name) {
    return '«$name» уже существует.';
  }

  @override
  String get errorReasonSelectFile => 'Выберите файл.';

  @override
  String get errorReasonVaultLocked => 'Хранилище заблокировано.';

  @override
  String get errorReasonNoActiveProfile => 'Нет активного профиля.';

  @override
  String get errorReasonNoVault => 'У этого профиля ещё нет хранилища.';

  @override
  String get errorReasonVaultExists => 'Хранилище уже существует.';

  @override
  String get errorReasonNotABackup => 'Это не резервная копия ConsoleCrypt (или файл отсутствует).';

  @override
  String errorReasonBackupExtension(String extension) {
    return 'Файл резервной копии должен иметь расширение .$extension';
  }

  @override
  String get errorReasonBackupFolderRequired => 'Сначала выберите папку для резервных копий.';

  @override
  String get errorReasonKeepAtLeastOne => 'Храните хотя бы одну резервную копию.';

  @override
  String get errorReasonSyncedProfileRequired => 'Для этого нужен синхронизируемый профиль.';

  @override
  String get errorReasonLocalProfileNoAccount => 'У локальных профилей нет аккаунта.';

  @override
  String get errorReasonNoDeviceRegistered => 'Устройство не зарегистрировано.';

  @override
  String get errorReasonTrustedDeviceRequired => 'Это может сделать только разблокированное доверенное устройство.';

  @override
  String get errorReasonCurrentPassphraseWrong => 'Текущая парольная фраза неверна.';

  @override
  String get errorReasonCurrentPasswordWrong => 'Текущий пароль неверен.';

  @override
  String get errorReasonAccountMismatch => 'Этот профиль принадлежит другому аккаунту.';

  @override
  String get validationFieldName => 'Имя';

  @override
  String get validationFieldAddress => 'Адрес';

  @override
  String get validationFieldPort => 'Порт';

  @override
  String get validationFieldBindHost => 'Адрес привязки';

  @override
  String get validationFieldBindPort => 'Порт привязки';

  @override
  String get validationFieldTargetHost => 'Целевой хост';

  @override
  String get validationFieldTargetPort => 'Целевой порт';

  @override
  String validationRequired(String field) {
    return '$field: обязательное поле';
  }

  @override
  String validationNoWhitespace(String field) {
    return '$field: не должно содержать пробелов';
  }

  @override
  String validationPortRange(String field) {
    return '$field: допустимо 1–65535';
  }

  @override
  String validationInvalid(String field) {
    return '$field: недопустимое значение';
  }

  @override
  String get validationJumpChainSelf => 'Хост не может использовать себя как jump-хост.';

  @override
  String get serverUrlEmpty => 'Введите URL вашего сервера ConsoleCrypt';

  @override
  String get serverUrlNotAUrl => 'Введите полный URL, например https://sync.example.org';

  @override
  String get serverUrlInsecure => 'Используйте https:// (обычный http разрешён только для localhost)';

  @override
  String get loginTitle => 'Подключение к вашему серверу';

  @override
  String get loginTitleReauth => 'Войдите снова';

  @override
  String get loginSubtitle => 'Укажите URL вашего собственного (self-hosted) сервера ConsoleCrypt.';

  @override
  String get loginSubtitleReauth => 'Сеанс этого профиля завершён. Локальные данные остаются на этом устройстве.';

  @override
  String get loginServerUrlLabel => 'URL сервера';

  @override
  String get loginCheckServer => 'Проверить';

  @override
  String loginServerInfoRegistrationOpen(String serverVersion, String protocolVersion) {
    return 'Сервер ConsoleCrypt $serverVersion · протокол $protocolVersion · регистрация открыта';
  }

  @override
  String loginServerInfoInviteOnly(String serverVersion, String protocolVersion) {
    return 'Сервер ConsoleCrypt $serverVersion · протокол $protocolVersion · только по приглашениям';
  }

  @override
  String get loginModeSignIn => 'Вход';

  @override
  String get loginModeRegister => 'Регистрация';

  @override
  String get loginEmailLabel => 'E-mail';

  @override
  String get loginEmailRequired => 'Введите адрес e-mail';

  @override
  String get loginPasswordLabel => 'Пароль аккаунта';

  @override
  String get loginPasswordHelperRegister => 'Не менее 10 символов. Должен отличаться от парольной фразы хранилища.';

  @override
  String get loginPasswordRequired => 'Введите пароль';

  @override
  String get loginPasswordTooShort => 'Не менее 10 символов';

  @override
  String get loginPasswordRepeatLabel => 'Повторите пароль';

  @override
  String get loginPasswordsDoNotMatch => 'Пароли не совпадают';

  @override
  String get loginDeviceNameLabel => 'Имя этого устройства';

  @override
  String get loginDeviceNameHelper => 'Отображается в списке ваших устройств, например «Рабочий MacBook».';

  @override
  String get loginDeviceNameRequired => 'Введите имя устройства';

  @override
  String get loginSubmitSignIn => 'Войти';

  @override
  String get loginSubmitRegister => 'Создать аккаунт';

  @override
  String get loginForgotPassword => 'Забыли пароль аккаунта?';

  @override
  String get loginForgotNeedsServerAndEmail => 'Сначала введите URL сервера и ваш e-mail';

  @override
  String get loginResetEmailSent =>
      'Если такой аккаунт существует, письмо для сброса пароля уже отправлено. Сброс восстанавливает только доступ к аккаунту — для хранилища по-прежнему нужна ваша парольная фраза или ключ восстановления.';

  @override
  String get loginPassphraseNeverLeaves =>
      'Парольная фраза хранилища никогда не покидает это устройство; сервер хранит только зашифрованные данные.';

  @override
  String get profileSwitcherTooltip => 'Сменить профиль';

  @override
  String get profileSwitcherAddProfileMenu => 'Добавить профиль…';

  @override
  String get profileSwitcherManageProfiles => 'Управление профилями';

  @override
  String profileSwitcherSwitchTo(String name) {
    return 'Переключиться на «$name»';
  }

  @override
  String get profileSwitcherAddProfile => 'Добавить профиль';

  @override
  String get backupsTitle => 'Резервные копии';

  @override
  String get backupsSubtitle =>
      'Зашифрованные файлы .ccbackup содержат только шифротекст и конверты ключей — их можно восстановить на любом компьютере с помощью парольной фразы или ключа восстановления.';

  @override
  String get backupsRestoreAction => 'Восстановить…';

  @override
  String get backupsExportAction => 'Экспортировать резервную копию…';

  @override
  String backupsSavedSnack(String fileName, String size) {
    return 'Резервная копия сохранена: $fileName ($size)';
  }

  @override
  String get backupsNoCloudCopyTitle => 'У этого профиля нет облачной копии';

  @override
  String get backupsNoCloudCopyMessage =>
      'Резервные копии — единственный способ вернуть данные, если это устройство будет потеряно.';

  @override
  String get backupsSyncedMessage =>
      'Этот профиль синхронизируется, поэтому на вашем сервере хранится зашифрованная копия. Офлайн-копии всё равно защищают от потери сервера или случайного удаления.';

  @override
  String get backupsRecentTitle => 'Последние резервные копии';

  @override
  String get backupsRecentEmpty => 'Из этого профиля ещё не создано ни одной резервной копии.';

  @override
  String backupsObjectCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count объекта',
      many: '$count объектов',
      few: '$count объекта',
      one: '$count объект',
    );
    return '$_temp0';
  }

  @override
  String get backupsAutomatic => 'автоматическая';

  @override
  String get backupsAutoTitle => 'Автоматическое резервное копирование';

  @override
  String get backupsAutoSubtitle =>
      'Записывает зашифрованную резервную копию в выбранную вами папку (например, на внешний диск или в синхронизируемую папку).';

  @override
  String get backupsFolderLabel => 'Папка';

  @override
  String get backupsFolderNotChosen => 'Не выбрана';

  @override
  String get backupsChooseFolder => 'Выбрать…';

  @override
  String get backupsFrequencyLabel => 'Периодичность';

  @override
  String get backupsKeepLabel => 'Хранить';

  @override
  String backupsKeepLast(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'последние $count резервной копии',
      many: 'последние $count резервных копий',
      few: 'последние $count резервные копии',
      one: 'последняя $count резервная копия',
    );
    return '$_temp0';
  }

  @override
  String get backupsLastRunLabel => 'Последний запуск';

  @override
  String get backupsNextRunLabel => 'Следующий запуск';

  @override
  String get backupsLastRunFailed => 'Последнее автоматическое резервное копирование не удалось';

  @override
  String backupsWrittenTo(String folder) {
    return 'Резервная копия записана в $folder';
  }

  @override
  String get backupsBackUpNow => 'Сделать резервную копию сейчас';

  @override
  String get restoreTitle => 'Восстановление из резервной копии';

  @override
  String get restoreSubtitle =>
      'Восстанавливает зашифрованный файл .ccbackup в новый локальный профиль на этом устройстве. Существующие профили не затрагиваются.';

  @override
  String get restoreFileLabel => 'Файл резервной копии';

  @override
  String get restoreBrowse => 'Обзор…';

  @override
  String get restoreVaultIdLabel => 'ID хранилища';

  @override
  String get restoreCreatedLabel => 'Дата создания';

  @override
  String get restoreObjectsLabel => 'Объекты';

  @override
  String restoreObjectsValue(String count, String size) {
    return '$count ($size)';
  }

  @override
  String get restoreWrittenByLabel => 'Создана в';

  @override
  String restoreWrittenByValue(String appVersion, int formatVersion) {
    return 'ConsoleCrypt $appVersion · формат v$formatVersion';
  }

  @override
  String get restoreUnlockPassphrase => 'Парольная фраза хранилища';

  @override
  String get restoreUnlockRecoveryKey => 'Ключ восстановления';

  @override
  String get restorePassphraseLabel => 'Парольная фраза хранилища из резервной копии';

  @override
  String get restoreRecoveryInputLabel => '24 слова или текст QR-кода';

  @override
  String get restoreNewPassphraseLabel => 'Новая парольная фраза хранилища';

  @override
  String get restoreProfileNameLabel => 'Имя профиля';

  @override
  String get restoreProfileNameHint => 'например, Личный (восстановлен)';

  @override
  String get restoreOpenBackup => 'Открыть резервную копию';

  @override
  String get restoreIntoNewProfile => 'Восстановить в новый профиль';

  @override
  String get welcomeDefaultProfileName => 'Личный';

  @override
  String get welcomeTitle => 'Добро пожаловать в ConsoleCrypt';

  @override
  String get welcomeTitleAddProfile => 'Добавить профиль';

  @override
  String get welcomeSubtitle =>
      'SSH-клиент со сквозным шифрованием. Хосты, ключи, пароли и сниппеты шифруются на этом устройстве ключом, который есть только у вас.';

  @override
  String get welcomeLocalTitle => 'Работать локально (без аккаунта)';

  @override
  String get welcomeLocalBody =>
      'Все данные хранятся на этом устройстве в зашифрованном виде. Без сервера, без аккаунта, без сетевого трафика синхронизации. Синхронизацию можно включить позже.';

  @override
  String get welcomeProfileNameLabel => 'Имя профиля';

  @override
  String get welcomeLocalAction => 'Работать локально';

  @override
  String get welcomeServerTitle => 'Подключиться к серверу';

  @override
  String get welcomeServerBody =>
      'Синхронизация со сквозным шифрованием между вашими компьютерами через собственный (self-hosted) сервер ConsoleCrypt. Сервер никогда не видит ваши данные и ключи.';

  @override
  String get welcomeServerAction => 'Войти или создать аккаунт';

  @override
  String get welcomeRestoreFromBackup => 'Восстановить из резервной копии (.ccbackup)';

  @override
  String get welcomeProfilesHint =>
      'Можно держать несколько профилей (например, личное локальное хранилище и рабочее синхронизируемое) и переключаться между ними на боковой панели.';

  @override
  String get credentialsTitle => 'Учётные данные';

  @override
  String get credentialsSubtitle =>
      'Пароли и закрытые ключи хранятся как отдельные зашифрованные секреты и не отображаются, пока вы явно их не покажете.';

  @override
  String get credentialsNew => 'Новые учётные данные';

  @override
  String get credentialsNewPassword => 'Пароль';

  @override
  String get credentialsNewGenerateKey => 'Сгенерировать SSH-ключ…';

  @override
  String get credentialsNewImportKey => 'Импортировать ключ OpenSSH…';

  @override
  String get credentialsNewCertificate => 'SSH-сертификат…';

  @override
  String get credentialsNewAgent => 'SSH-агент…';

  @override
  String get credentialsEmptyTitle => 'Учётных данных пока нет';

  @override
  String get credentialsEmptyMessage =>
      'Сгенерируйте ключ Ed25519, импортируйте существующий ключ OpenSSH или сохраните пароль.';

  @override
  String credentialsListUser(String username) {
    return 'пользователь $username';
  }

  @override
  String get credentialsListPassphraseRemembered => 'парольная фраза сохранена';

  @override
  String get credentialsListPassphraseProtected => 'защищён парольной фразой';

  @override
  String credentialsListUsedBy(int count) {
    return 'используется: $count';
  }

  @override
  String credentialsDeleteTitle(String name) {
    return 'Удалить «$name»?';
  }

  @override
  String get credentialsDeleteMessage =>
      'Учётные данные и их секрет будут удалены из хранилища. Хостам, которые их используют, понадобятся другие учётные данные.';

  @override
  String get credentialsDetailsType => 'Тип';

  @override
  String get credentialsDetailsUsername => 'Имя пользователя';

  @override
  String get credentialsDetailsAlgorithm => 'Алгоритм';

  @override
  String get credentialsDetailsFingerprint => 'Отпечаток';

  @override
  String get credentialsDetailsPassphraseRemembered => 'Сохранена (хранится как отдельный зашифрованный секрет)';

  @override
  String get credentialsDetailsPassphraseAsked => 'Запрашивается при подключении — ключ сохраняет собственную защиту';

  @override
  String get credentialsDetailsAgentSocket => 'Сокет агента';

  @override
  String get credentialsDetailsCreated => 'Создано';

  @override
  String get credentialsDetailsPublicKey => 'Открытый ключ';

  @override
  String get credentialsCopyPublicKey => 'Копировать открытый ключ';

  @override
  String get credentialsDetailsCertificate => 'Сертификат';

  @override
  String get credentialsReveal => 'Показать';

  @override
  String get credentialsRevealHint =>
      'Показанные значения снова скрываются через 20 с; скопированные значения автоматически удаляются из буфера обмена.';

  @override
  String get credentialDialogNewPasswordTitle => 'Новый пароль';

  @override
  String get credentialDialogUsernameOptional => 'Имя пользователя (необязательно)';

  @override
  String get credentialDialogPasswordLabel => 'Пароль';

  @override
  String get credentialDialogKeyPassphrase => 'Парольная фраза ключа';

  @override
  String get credentialDialogRememberPassphrase => 'Запомнить парольную фразу SSH-ключа';

  @override
  String get credentialDialogAgentTitle => 'SSH-агент';

  @override
  String get credentialDialogAgentDefaultName => 'SSH-агент';

  @override
  String get credentialDialogAgentPathLabel => 'Путь к сокету или имя канала (pipe)';

  @override
  String credentialDialogAgentPathHint(String unixPath, String windowsPath) {
    return '$unixPath  или  $windowsPath';
  }

  @override
  String get credentialDialogAgentNotice => 'Ключи остаются в агенте; ConsoleCrypt никогда их не видит.';

  @override
  String get credentialGenerateTitle => 'Генерация SSH-ключа';

  @override
  String get credentialGenerateAction => 'Сгенерировать';

  @override
  String credentialGenerateRecommended(String algorithm) {
    return '$algorithm (рекомендуется)';
  }

  @override
  String get credentialGenerateRsaNotice =>
      'RSA нужен для совместимости со старыми серверами; генерация занимает несколько секунд.';

  @override
  String get credentialGenerateCommentLabel => 'Комментарий (необязательно)';

  @override
  String get credentialGeneratePassphraseLabel => 'Парольная фраза ключа (необязательно)';

  @override
  String get credentialGeneratePassphraseHelper => 'Защищает сам ключ — в дополнение к шифрованию хранилища.';

  @override
  String get credentialGenerateRepeatPassphrase => 'Повторите парольную фразу ключа';

  @override
  String get credentialGeneratePassphraseMismatch => 'Парольные фразы не совпадают';

  @override
  String get credentialGenerateRememberSubtitle =>
      'Сохраняется в хранилище как отдельный зашифрованный секрет, поэтому при подключении её не нужно вводить.';

  @override
  String get credentialGenerateDoneTitle => 'Ключ сгенерирован';

  @override
  String get credentialGenerateDoneMessage => 'Добавьте этот открытый ключ в ~/.ssh/authorized_keys на ваших серверах:';

  @override
  String get credentialImportTitle => 'Импорт ключа OpenSSH';

  @override
  String get credentialImportTitleWithCertificate => 'Импорт ключа и сертификата';

  @override
  String get credentialImportAction => 'Импортировать';

  @override
  String get credentialImportPrivateKey => 'Закрытый ключ';

  @override
  String get credentialImportUnknownAlgorithm => 'Ключ';

  @override
  String get credentialImportEncryptedNotice => 'Защищён парольной фразой: в хранилище он остаётся зашифрованным.';

  @override
  String get credentialImportInvalidKey => 'Некорректный ключ';

  @override
  String get credentialImportRememberSubtitle =>
      'Выкл.: она запрашивается при каждом подключении. Вкл.: хранится как отдельный зашифрованный секрет.';

  @override
  String get credentialImportCertificateLabel => 'Сертификат OpenSSH';

  @override
  String approveDeviceTitle(String name) {
    return 'Одобрить устройство «$name»?';
  }

  @override
  String approveDeviceStepShowsCode(String name) {
    return '1. На устройстве «$name» ConsoleCrypt показывает код проверки.';
  }

  @override
  String get approveDeviceStepCompare => '2. Сравните его с кодом, вычисленным здесь, — группу за группой:';

  @override
  String approveDeviceCodesMatchCheckbox(String name) {
    return 'Все 6 групп в точности совпадают с кодом на экране устройства «$name»';
  }

  @override
  String get approveDeviceWarning =>
      'Одобрение передаёт этому устройству ключ к вашему хранилищу. Никогда не одобряйте устройство, которое вы не настраиваете сами прямо сейчас.';

  @override
  String get approveDeviceCodesDontMatch => 'Коды не совпадают';

  @override
  String get approveDeviceApprove => 'Одобрить';

  @override
  String approveDeviceApproved(String name) {
    return 'Устройство «$name» теперь может разблокировать ваше хранилище';
  }

  @override
  String get approveDeviceRejectedTitle => 'Запрос отклонён';

  @override
  String get approveDeviceRejectedMessage =>
      'Коды не совпали, поэтому устройство НЕ одобрено. Возможно, кто-то — например, скомпрометированный сервер — пытался добавить своё устройство в ваш аккаунт.\n\nЕсли запрос отправляли не вы, смените пароль аккаунта и проверьте список устройств.';

  @override
  String get devicesTitle => 'Устройства';

  @override
  String devicesRevokeTitle(String name) {
    return 'Отозвать устройство «$name»?';
  }

  @override
  String get devicesRevokeMessage =>
      'Устройство сразу выходит из аккаунта, теряет доступ ко всем вашим хранилищам и больше не может синхронизироваться. Это действие необратимо: чтобы снова пользоваться устройством, его нужно одобрить как новое.\n\nОтзыв защищает будущее, но не прошлое: данные, которые устройство уже расшифровало, нельзя стереть дистанционно.';

  @override
  String get devicesRevokeConfirm => 'Отозвать устройство';

  @override
  String devicesRevoked(String name) {
    return 'Устройство «$name» отозвано';
  }

  @override
  String get devicesRenameTitle => 'Переименовать устройство';

  @override
  String get devicesLocalTitle => 'Устройства есть только у синхронизируемых профилей';

  @override
  String get devicesLocalMessage =>
      'Этот профиль хранится только локально: он есть на этом устройстве и больше нигде. Включите синхронизацию, чтобы пользоваться хранилищем на нескольких компьютерах с подтверждением устройств.';

  @override
  String get devicesEnableSync => 'Включить синхронизацию…';

  @override
  String devicesSubtitle(String email) {
    return 'Устройства, на которых выполнен вход в аккаунт $email. Расшифровать ваше хранилище могут только доверенные устройства.';
  }

  @override
  String get devicesSubtitleNoEmail =>
      'Устройства, на которых выполнен вход в ваш аккаунт. Расшифровать ваше хранилище могут только доверенные устройства.';

  @override
  String get devicesRefresh => 'Обновить';

  @override
  String devicesPendingTitle(String name) {
    return 'Устройство «$name» запрашивает доступ к вашему хранилищу';
  }

  @override
  String devicesPendingMeta(String platform, String ago, String remaining) {
    return '$platform · запрошено $ago · истекает $remaining';
  }

  @override
  String devicesPendingMetaExpired(String platform, String ago) {
    return '$platform · запрошено $ago · срок истёк';
  }

  @override
  String get devicesPendingHint =>
      'Одобряйте, только если вы сами настраиваете это устройство прямо сейчас и коды проверки совпадают.';

  @override
  String get devicesRequestRejected => 'Запрос отклонён';

  @override
  String get devicesReject => 'Отклонить';

  @override
  String get devicesReviewApprove => 'Проверить и одобрить';

  @override
  String get devicesStatusRevoked => 'Отозвано';

  @override
  String get devicesStatusTrusted => 'Доверенное';

  @override
  String get devicesStatusAwaitingApproval => 'Ожидает подтверждения';

  @override
  String get devicesStatusNotTrusted => 'Не доверенное для этого хранилища';

  @override
  String get devicesThisDevice => 'Это устройство';

  @override
  String devicesLastSeen(String ago) {
    return 'последняя активность: $ago';
  }

  @override
  String devicesAdded(String date) {
    return 'добавлено $date';
  }

  @override
  String devicesRevokedOn(String date) {
    return 'отозвано $date';
  }

  @override
  String get devicesRevokeMenu => 'Отозвать…';

  @override
  String get syncScreenTitle => 'Синхронизация';

  @override
  String get syncScreenLocalSubtitle => 'У этого профиля нет сервера и аккаунта. Данные не покидают это устройство.';

  @override
  String get syncScreenLocalBody =>
      'Включите синхронизацию, чтобы пользоваться этим хранилищем на других компьютерах. Хранилище выгружается со сквозным шифрованием на ваш собственный сервер; его ID, парольная фраза и комплект восстановления остаются прежними.';

  @override
  String get syncScreenEnableSync => 'Включить синхронизацию…';

  @override
  String get syncScreenBackups => 'Резервные копии';

  @override
  String syncScreenServer(String url) {
    return 'Сервер: $url';
  }

  @override
  String get syncScreenSyncNow => 'Синхронизировать';

  @override
  String get syncScreenState => 'Состояние';

  @override
  String get syncScreenStateIdle => 'синхронизировано';

  @override
  String get syncScreenStateSyncing => 'синхронизация';

  @override
  String get syncScreenStateOffline => 'офлайн';

  @override
  String get syncScreenStateError => 'ошибка';

  @override
  String get syncScreenStatePaused => 'приостановлено';

  @override
  String get syncScreenLastSync => 'Последняя успешная синхронизация';

  @override
  String get syncScreenPendingChanges => 'Изменения в очереди';

  @override
  String get syncScreenServerSequence => 'Порядковый номер на сервере';

  @override
  String get syncScreenNextRetry => 'Следующая попытка';

  @override
  String get syncScreenOfflineMessage =>
      'Сервер недоступен. Хосты, ключи, SSH, SFTP и туннели продолжают работать; изменения сохраняются локально и будут отправлены, когда сервер снова станет доступен.';

  @override
  String get syncScreenProblems => 'Проблемы';

  @override
  String get syncScreenIssueRetryable => 'Сбой синхронизации — повтор будет выполнен автоматически';

  @override
  String get syncScreenIssue => 'Сбой синхронизации';

  @override
  String get syncScreenDisconnectTitle => 'Отключение синхронизации';

  @override
  String get syncScreenDisconnectBody => 'Прекратить синхронизацию этого профиля и сохранить его данные локально.';

  @override
  String get syncScreenDisconnect => 'Отключить…';

  @override
  String get syncScreenDeveloper => 'Для разработчиков (mock-бэкенд)';

  @override
  String get syncScreenSimulateOutage => 'Имитировать недоступность сервера';

  @override
  String get syncDisconnectTitle => 'Отключить синхронизацию?';

  @override
  String get syncDisconnectMessage =>
      'Синхронизация остановится, и профиль станет только локальным. Все данные останутся на этом устройстве. Зашифрованная копия на сервере не изменится.';

  @override
  String get syncDisconnectRevoke => 'Также отозвать это устройство на сервере';

  @override
  String get syncDisconnectConfirm => 'Отключить (сохранить данные локально)';

  @override
  String get enableSyncDialogTitle => 'Включить синхронизацию';

  @override
  String get enableSyncDialogStepServer => 'Шаг 1 из 3 · Ваш сервер';

  @override
  String get enableSyncDialogStepAccount => 'Шаг 2 из 3 · Аккаунт';

  @override
  String get enableSyncDialogStepUpload => 'Шаг 3 из 3 · Выгрузка';

  @override
  String get enableSyncDialogIntro =>
      'Хранилище выгружается со сквозным шифрованием: сервер хранит только шифротекст и никогда не видит вашу парольную фразу и ключи. Хранилище сохраняет свой ID, а комплект восстановления остаётся действительным.';

  @override
  String get enableSyncDialogServerUrl => 'URL сервера';

  @override
  String enableSyncDialogServerInfo(String serverVersion, String protocolVersion) {
    return 'Сервер ConsoleCrypt $serverVersion · протокол $protocolVersion';
  }

  @override
  String get enableSyncDialogCreateAccount => 'Создать аккаунт';

  @override
  String get enableSyncDialogSignIn => 'Войти';

  @override
  String get enableSyncDialogEmail => 'E-mail';

  @override
  String get enableSyncDialogPassword => 'Пароль аккаунта';

  @override
  String get enableSyncDialogPasswordHelper => 'Не менее 10 символов. Это не парольная фраза хранилища.';

  @override
  String get enableSyncDialogDeviceName => 'Имя этого устройства';

  @override
  String get enableSyncDialogCredentialsRequired => 'Введите e-mail и пароль';

  @override
  String enableSyncDialogUploadedObjects(int uploaded, int total) {
    String _temp0 = intl.Intl.pluralLogic(
      total,
      locale: localeName,
      other: '$uploaded из $total зашифрованного объекта',
      many: '$uploaded из $total зашифрованных объектов',
      few: '$uploaded из $total зашифрованных объектов',
      one: '$uploaded из $total зашифрованного объекта',
    );
    return '$_temp0';
  }

  @override
  String get enableSyncDialogDone =>
      'Теперь этот профиль синхронизируется. Чтобы добавить другие устройства, войдите в аккаунт на них и одобрите их здесь по коду проверки.';

  @override
  String get enableSyncDialogFailed => 'Не удалось включить синхронизацию.';

  @override
  String get enableSyncDialogFailedTitle => 'Не удалось включить синхронизацию';

  @override
  String get enableSyncDialogStart => 'Включить синхронизацию';

  @override
  String get enableSyncDialogTryAgain => 'Повторить попытку';

  @override
  String get glassSetting => 'Эффект стекла';

  @override
  String get glassModeClear => 'Прозрачное';

  @override
  String get glassModeStandard => 'По умолчанию';

  @override
  String get glassModeTinted => 'Тонированное';

  @override
  String get glassModeSolid => 'Непрозрачное';

  @override
  String get glassCapsLockOn => 'Включён Caps Lock';

  @override
  String get glassDismiss => 'Закрыть';

  @override
  String get glassCloseTab => 'Закрыть вкладку';

  @override
  String get glassNewTab => 'Новая вкладка';

  @override
  String get glassTabConnected => 'Подключено';

  @override
  String get glassTabReconnecting => 'Переподключение…';

  @override
  String get glassTabDisconnected => 'Отключено';

  @override
  String get glassVerificationCode => 'Код проверки';

  @override
  String glassCodeGroup(int number, String digits) {
    return 'Группа $number: $digits';
  }

  @override
  String glassClearsIn(int seconds) {
    return 'Очистка через $seconds с';
  }

  @override
  String get glassMoreActions => 'Ещё';

  @override
  String get hostsTitle => 'Хосты';

  @override
  String hostsSubtitle(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count хоста в этом хранилище',
      many: '$count хостов в этом хранилище',
      few: '$count хоста в этом хранилище',
      one: '$count хост в этом хранилище',
    );
    return '$_temp0';
  }

  @override
  String get hostsNewHost => 'Новый хост';

  @override
  String get hostsSearchHint => 'Поиск по имени, адресу, тегу или группе';

  @override
  String get hostsEmptyTitle => 'Хостов пока нет';

  @override
  String get hostsNoMatches => 'Подходящих хостов нет';

  @override
  String get hostsEmptyMessage => 'Добавьте первый SSH-хост, чтобы подключиться.';

  @override
  String get hostsAddHost => 'Добавить хост';

  @override
  String get hostsViaJumpHostsTooltip => 'Подключение через jump-хосты';

  @override
  String get hostsOpenSshTooltip => 'Используется системный OpenSSH';

  @override
  String get hostsAssignGroup => 'Добавить в группу…';

  @override
  String get hostsGroupHelp =>
      'Выберите одну группу для хоста. Не заданные у хоста параметры подключения наследуются от группы.';

  @override
  String get hostsGroupSearchHint => 'Найти группу…';

  @override
  String get hostsGroupNoMatches => 'Группы не найдены';

  @override
  String get hostsMoreTooltip => 'Ещё';

  @override
  String get hostsOpenSftp => 'Открыть SFTP';

  @override
  String hostsDeleteTitle(String name) {
    return 'Удалить «$name»?';
  }

  @override
  String get hostsDeleteMessage =>
      'Хост будет удалён из этого хранилища (и с других ваших устройств после синхронизации). Цепочки jump-хостов, в которых он используется, будут обновлены.';

  @override
  String get hostsDeleted => 'Хост удалён';

  @override
  String get hostsNewCredentialEntry => '+ Новые учётные данные…';

  @override
  String get hostEditorLoadingTitle => 'Хост';

  @override
  String get hostEditorNewTitle => 'Новый хост';

  @override
  String hostEditorEditTitle(String name) {
    return 'Редактирование «$name»';
  }

  @override
  String get hostEditorEditTitleFallback => 'Редактирование хоста';

  @override
  String get hostEditorConnectionSection => 'Подключение';

  @override
  String get hostEditorNameRequired => 'Введите имя';

  @override
  String get hostEditorAddressLabel => 'Адрес';

  @override
  String get hostEditorAddressHint => 'имя хоста или IP';

  @override
  String get hostEditorAddressRequired => 'Введите адрес';

  @override
  String get hostEditorAddressNoSpaces => 'Пробелы не допускаются';

  @override
  String get hostEditorPortLabel => 'Порт';

  @override
  String get hostEditorPortHint => 'наследуется';

  @override
  String get hostEditorUsernameLabel => 'Имя пользователя';

  @override
  String get hostEditorUsernameHint => 'наследуется от группы';

  @override
  String get hostEditorGroupLabel => 'Группа';

  @override
  String get hostEditorNoGroup => 'Без группы';

  @override
  String get hostEditorJumpSection => 'Jump-хосты';

  @override
  String get hostEditorJumpSubtitle =>
      'Несколько узлов: каждый узел открывает канал direct-tcpip к следующему. Число узлов не ограничено.';

  @override
  String get hostEditorJumpInherit => 'Наследовать / напрямую';

  @override
  String get hostEditorJumpProfile => 'Профиль jump-хостов';

  @override
  String get hostEditorJumpCustom => 'Своя цепочка';

  @override
  String get hostEditorJumpInheritHelp =>
      'Используется профиль jump-хостов группы (если он задан), иначе подключение идёт напрямую.';

  @override
  String hostEditorJumpProfileItem(int count, String name) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$name ($count узла)',
      many: '$name ($count узлов)',
      few: '$name ($count узла)',
      one: '$name ($count узел)',
    );
    return '$_temp0';
  }

  @override
  String get hostEditorThisHost => 'этот хост';

  @override
  String get hostEditorSecuritySection => 'Безопасность';

  @override
  String get hostEditorHostKeyPolicy => 'Политика ключей хостов';

  @override
  String get hostEditorSshBackend => 'SSH-бэкенд';

  @override
  String get hostEditorKeepalive => 'Интервал keepalive (с)';

  @override
  String get hostEditorKeepaliveHint => 'по умолчанию';

  @override
  String get hostEditorAgentForwarding => 'Проброс агента';

  @override
  String get hostEditorAgentForwardingHelp =>
      'Позволяет этому серверу использовать ваши ключи для дальнейших подключений. Включайте только для хостов, которым доверяете.';

  @override
  String get hostEditorOrganisationSection => 'Организация';

  @override
  String get hostEditorNotes => 'Заметки';

  @override
  String get hostEditorEffectiveTitle => 'Итоговые настройки';

  @override
  String get hostEditorEffectiveSubtitle =>
      'Вычислены по этому хосту и цепочке его групп — именно они будут использованы при подключении.';

  @override
  String get hostEditorEffectiveGroup => 'Группа';

  @override
  String get hostEditorEffectiveUsername => 'Пользователь';

  @override
  String get hostEditorEffectivePort => 'Порт';

  @override
  String get hostEditorEffectiveCredential => 'Учётные данные';

  @override
  String get hostEditorEffectiveRoute => 'Маршрут';

  @override
  String get hostEditorRouteDirect => 'Напрямую';

  @override
  String get hostEditorSourceHost => 'задано на этом хосте';

  @override
  String hostEditorSourceFrom(String name) {
    return 'источник: $name';
  }

  @override
  String hostEditorSourceCredential(String name) {
    return 'из учётных данных «$name»';
  }

  @override
  String hostEditorSourceGroup(String name) {
    return 'унаследовано от группы «$name»';
  }

  @override
  String hostEditorSourceJumpProfile(String name) {
    return 'профиль jump-хостов «$name»';
  }

  @override
  String get hostEditorSourceDefault => 'по умолчанию';

  @override
  String get hostEditorSourceUnset => 'не задано';

  @override
  String get hostEditorCredentialPrompt => 'Пароль — запрашивается при подключении';

  @override
  String get hostEditorProblemJumpHostDeleted => 'Один из jump-хостов цепочки был удалён';

  @override
  String get hostEditorProblemSelfJump => 'Хост не может использовать себя как jump-хост';

  @override
  String get hostEditorProblemNoCredential => 'Нет учётных данных: пароль будет запрошен при подключении';

  @override
  String get hostEditorProblemCredentialMissing => 'Выбранные учётные данные больше не существуют';

  @override
  String get hostEditorProblemNoUsername => 'Не задано имя пользователя: укажите его на хосте или в группе';

  @override
  String get hostAuthModePassword => 'Пароль';

  @override
  String get hostAuthModeSshKey => 'SSH-ключ';

  @override
  String get hostAuthModeAgent => 'Агент';

  @override
  String get hostAuthModeInherit => 'Из группы';

  @override
  String get hostAuthTitle => 'Аутентификация';

  @override
  String get hostAuthUseExisting => 'Выбрать учётные данные…';

  @override
  String get hostAuthLinkedDeleted => 'Связанные учётные данные удалены';

  @override
  String hostAuthLinkedShared(String name, String kind) {
    return '$name · общие учётные данные: $kind';
  }

  @override
  String get hostAuthUnlink => 'Ввести пароль';

  @override
  String get hostAuthChangeLinked => 'Изменить…';

  @override
  String get hostAuthPasswordSaved => 'Пароль сохранён ';

  @override
  String get hostAuthPasswordLabel => 'Пароль';

  @override
  String get hostAuthNewPasswordLabel => 'Новый пароль';

  @override
  String get hostAuthPasswordHelper => 'Оставьте пустым, чтобы вводить пароль при подключении.';

  @override
  String get hostAuthSavePassword => 'Сохранить пароль в хранилище';

  @override
  String get hostAuthSavePasswordOn =>
      'Хранится как отдельный секрет со сквозным шифрованием и больше никогда не показывается.';

  @override
  String get hostAuthSavePasswordOff => 'Ничего не сохраняется — терминал запрашивает пароль при каждом подключении.';

  @override
  String get hostAuthKeepSaved => 'Оставить сохранённый пароль';

  @override
  String get hostAuthKeyLabel => 'Ключ';

  @override
  String get hostAuthGenerateKey => 'Сгенерировать Ed25519…';

  @override
  String get hostAuthImportKey => 'Импортировать ключ…';

  @override
  String get hostAuthPassphraseRemembered => 'Парольная фраза ключа сохранена';

  @override
  String get hostAuthForget => 'Забыть';

  @override
  String get hostAuthPassphraseForgotten => 'Сохранённая парольная фраза удалена';

  @override
  String get hostAuthKeyPassphraseLabel => 'Парольная фраза ключа';

  @override
  String get hostAuthKeyPassphraseHelper =>
      'Ключ остаётся защищён парольной фразой. Без «Запомнить» она запрашивается при подключении.';

  @override
  String get hostAuthRememberPassphrase => 'Запомнить парольную фразу';

  @override
  String get hostAuthRememberPassphraseHelper =>
      'Хранится как отдельный зашифрованный секрет этого ключа (общий для всех его хостов).';

  @override
  String get hostAuthAgentPathLabel => 'Путь к сокету или имя канала (pipe)';

  @override
  String hostAuthAgentPathHint(String unixPath, String windowsPath) {
    return '$unixPath  или  $windowsPath';
  }

  @override
  String get hostAuthAgentNote => 'Ключи остаются в агенте; ConsoleCrypt их не видит.';

  @override
  String get hostAuthInheritNoGroup =>
      'Хост не входит ни в одну группу, поэтому наследовать нечего: терминал запросит пароль. Выберите группу или другой режим.';

  @override
  String hostAuthInheritFromGroup(String group) {
    return 'Имя пользователя и учётные данные берутся из группы «$group» и её родительских групп — см. «Итоговые настройки».';
  }

  @override
  String hostAuthPreviewPasswordShared(String name) {
    return 'Пароль · $name';
  }

  @override
  String get hostAuthPreviewPasswordDeleted => 'Пароль · учётные данные удалены';

  @override
  String get hostAuthPreviewPasswordSaved => 'Пароль · сохранён в хранилище';

  @override
  String get hostAuthPreviewPasswordPrompt => 'Пароль · запрашивается при подключении';

  @override
  String hostAuthPreviewSshKey(String name) {
    return 'SSH-ключ · $name';
  }

  @override
  String hostAuthPreviewAgentAt(String path) {
    return 'Агент: $path';
  }

  @override
  String get hostAuthErrorKeyRequired => 'Выберите, сгенерируйте или импортируйте SSH-ключ';

  @override
  String get hostAuthErrorAgentPathRequired => 'Укажите путь к сокету агента или имя канала (pipe)';

  @override
  String get hostAuthPickerTitle => 'Выбор учётных данных';

  @override
  String get hostAuthPickerEmpty => 'Учётных данных пока нет';

  @override
  String hostAuthPickerUser(String username) {
    return 'пользователь $username';
  }

  @override
  String hostAuthPickerUsedBy(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'используется на $count хоста',
      many: 'используется на $count хостах',
      few: 'используется на $count хостах',
      one: 'используется на $count хосте',
    );
    return '$_temp0';
  }

  @override
  String get hostAuthPickerNew => 'Новые учётные данные…';

  @override
  String get hostPickerDefaultTitle => 'Подключиться к хосту';

  @override
  String get hostPickerSearchHint => 'Поиск хостов';

  @override
  String get hostPickerEmpty => 'Нет хостов';

  @override
  String get jumpChainDeletedHost => '(хост удалён)';

  @override
  String get jumpChainThisDevice => 'Это устройство';

  @override
  String get jumpChainTarget => 'целевой хост';

  @override
  String get jumpChainEmpty => 'Узлов нет — добавьте первый jump-хост ниже.';

  @override
  String get jumpChainMoveUp => 'Переместить выше';

  @override
  String get jumpChainRemoveHop => 'Удалить узел';

  @override
  String get jumpChainAddTooltip => 'Добавить jump-хост';

  @override
  String get jumpChainAddHop => 'Добавить узел';

  @override
  String get groupsTitle => 'Группы';

  @override
  String get groupsSubtitle =>
      'Хосты наследуют имя пользователя, порт, учётные данные и профиль jump-хостов от цепочки своих групп.';

  @override
  String get groupsNewGroup => 'Новая группа';

  @override
  String get groupsEditGroup => 'Редактирование группы';

  @override
  String get groupsEmpty => 'Групп пока нет';

  @override
  String groupsHostCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count хоста',
      many: '$count хостов',
      few: '$count хоста',
      one: '$count хост',
    );
    return '$_temp0';
  }

  @override
  String get groupsHasDefaults => 'есть значения по умолчанию';

  @override
  String groupsDeleteTitle(String name) {
    return 'Удалить группу «$name»?';
  }

  @override
  String get groupsDeleteMessage =>
      'Подгруппы и хосты переместятся в родительскую группу. Хосты, наследовавшие настройки от этой группы, будут наследовать их от родительской.';

  @override
  String get groupsAddSubgroup => 'Добавить подгруппу';

  @override
  String get groupsSourceOwn => 'задано в этой группе';

  @override
  String groupsSourceInherited(String name) {
    return 'унаследовано от «$name»';
  }

  @override
  String get groupsSourceUnset => 'не задано';

  @override
  String get groupsUsername => 'Имя пользователя';

  @override
  String get groupsPort => 'Порт';

  @override
  String get groupsCredential => 'Учётные данные';

  @override
  String get groupsJumpProfile => 'Профиль jump-хостов';

  @override
  String get groupsHostsSection => 'Хосты в этой группе';

  @override
  String get groupsNoHosts => 'Непосредственно в этой группе хостов нет.';

  @override
  String get groupsParentGroup => 'Родительская группа';

  @override
  String get groupsNoParent => 'Нет (верхний уровень)';

  @override
  String get groupsDefaultsHeading =>
      'Значения по умолчанию для хостов (оставьте пустым, чтобы наследовать от родительской группы)';

  @override
  String get groupsInherit => 'Наследовать';

  @override
  String get groupsJumpInherit => 'Наследовать / напрямую';

  @override
  String get groupsJumpProfilesSection => 'Профили jump-хостов';

  @override
  String get groupsNewJumpProfile => 'Новый профиль jump-хостов';

  @override
  String get groupsEditJumpProfile => 'Редактирование профиля jump-хостов';

  @override
  String get groupsJumpProfilesEmpty => 'Упорядоченные цепочки jump-хостов для повторного использования.';

  @override
  String get settingsTitle => 'Настройки';

  @override
  String get settingsRenameProfileTitle => 'Переименовать профиль';

  @override
  String settingsRemoveProfileTitle(String name) {
    return 'Удалить профиль «$name» с этого устройства?';
  }

  @override
  String get settingsRemoveProfileLocalMessage =>
      'Локальная база данных и ключи этого профиля будут удалены с этого устройства. Без резервной копии данные будут потеряны навсегда.';

  @override
  String get settingsRemoveProfileSyncedMessage =>
      'Локальные данные этого профиля будут удалены с этого устройства. Зашифрованное хранилище на вашем сервере не затрагивается.';

  @override
  String get settingsRemoveProfileConfirm => 'Удалить профиль';

  @override
  String get settingsProfilesTitle => 'Профили';

  @override
  String get settingsProfilesSubtitle =>
      'Каждый профиль — отдельное зашифрованное хранилище (например, локальное личное и синхронизируемое рабочее).';

  @override
  String get settingsAddProfile => 'Добавить профиль';

  @override
  String settingsProfileActive(String name) {
    return '$name  (активный)';
  }

  @override
  String get settingsProfileSwitch => 'Переключиться';

  @override
  String get settingsSyncTitle => 'Синхронизация';

  @override
  String get settingsSyncLocalSubtitle => 'Только локально: без сервера и аккаунта.';

  @override
  String get settingsSyncLocalBody =>
      'Загрузите это хранилище со сквозным шифрованием на собственный сервер (self-hosted), чтобы пользоваться им на других устройствах.';

  @override
  String get settingsEnableSync => 'Включить синхронизацию…';

  @override
  String get settingsAccountSyncTitle => 'Аккаунт и синхронизация';

  @override
  String get settingsServerLabel => 'Сервер';

  @override
  String get settingsAccountLabel => 'Аккаунт';

  @override
  String get settingsThisDeviceLabel => 'Это устройство';

  @override
  String get settingsChangeAccountPassword => 'Изменить пароль аккаунта';

  @override
  String get settingsDisconnect => 'Отключиться (сохранить данные локально)…';

  @override
  String get settingsSignOut => 'Выйти';

  @override
  String get settingsDifferentServerHint =>
      'Чтобы использовать другой сервер, добавьте ещё один профиль: у каждого профиля свой сервер и своё хранилище.';

  @override
  String get settingsVaultTitle => 'Хранилище';

  @override
  String get settingsVaultNameLabel => 'Название';

  @override
  String get settingsVaultIdLabel => 'ID хранилища';

  @override
  String get settingsAutoLockLabel => 'Автоблокировка';

  @override
  String settingsAutoLockAfterMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'через $count минуты',
      many: 'через $count минут',
      few: 'через $count минуты',
      one: 'через $count минуту',
    );
    return '$_temp0';
  }

  @override
  String settingsAutoLockAfterHours(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'через $count часа',
      many: 'через $count часов',
      few: 'через $count часа',
      one: 'через $count час',
    );
    return '$_temp0';
  }

  @override
  String get settingsAutoLockNever => 'никогда';

  @override
  String get settingsChangePassphrase => 'Изменить парольную фразу…';

  @override
  String get settingsNewRecoveryKit => 'Новый комплект восстановления…';

  @override
  String get settingsBackups => 'Резервные копии…';

  @override
  String get settingsLockNow => 'Заблокировать сейчас';

  @override
  String get settingsAiProvidersTitle => 'ИИ-провайдеры';

  @override
  String get settingsAiProvidersSubtitle =>
      'ИИ может читать разрешённые вами сниппеты и метаданные хостов — но никогда не пароли, ключи или ключ хранилища.';

  @override
  String settingsAiProviderDefault(String name) {
    return '$name  · по умолчанию';
  }

  @override
  String get settingsAiProviderApiKeyStored => 'API-ключ сохранён';

  @override
  String get settingsDefaultPrivacyLabel => 'Приватность по умолчанию';

  @override
  String get settingsKeepAiConversations => 'Сохранять диалоги с ИИ в хранилище';

  @override
  String get settingsKeepAiConversationsHelp =>
      'Хранятся в зашифрованном виде (в синхронизируемых профилях также синхронизируются). Если выключено, диалоги не сохраняются.';

  @override
  String get settingsTerminalTitle => 'Терминал';

  @override
  String get settingsCommandHistoryLabel => 'История команд';

  @override
  String settingsHistoryPendingSync(String description) {
    return '$description (вступит в силу после включения синхронизации)';
  }

  @override
  String get settingsFontSizeLabel => 'Размер шрифта';

  @override
  String get settingsScrollbackLabel => 'Буфер прокрутки';

  @override
  String settingsScrollbackLines(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count строки',
      many: '$count строк',
      few: '$count строки',
      one: '$count строка',
    );
    return '$_temp0';
  }

  @override
  String get settingsTerminalBuffersNote =>
      'Буферы терминала хранятся только на этом устройстве и никогда не синхронизируются.';

  @override
  String get settingsAppearanceSecurityTitle => 'Оформление и безопасность';

  @override
  String get settingsThemeLabel => 'Тема';

  @override
  String get settingsThemeSystem => 'Системная';

  @override
  String get settingsThemeLight => 'Светлая';

  @override
  String get settingsThemeDark => 'Тёмная';

  @override
  String get settingsClearClipboardLabel => 'Очищать буфер обмена';

  @override
  String settingsClearClipboardAfter(int seconds) {
    return '$seconds с после копирования секрета';
  }

  @override
  String get settingsKnownHostsTitle => 'Известные хосты';

  @override
  String get settingsKnownHostsSubtitle =>
      'Ключи хостов, которым вы доверяете. Изменённый ключ всегда блокирует подключение, пока вы не удалите здесь старый.';

  @override
  String get settingsKnownHostsEmpty => 'Известных хостов пока нет.';

  @override
  String settingsRemoveKnownHostTitle(String host) {
    return 'Удалить «$host»?';
  }

  @override
  String get settingsRemoveKnownHostMessage => 'При следующем подключении вам снова нужно будет проверить ключ хоста.';

  @override
  String get settingsDialogPassphraseChanged => 'Парольная фраза хранилища изменена';

  @override
  String get settingsDialogChangePassphraseTitle => 'Изменить парольную фразу хранилища';

  @override
  String get settingsDialogCurrentPassphrase => 'Текущая парольная фраза';

  @override
  String get settingsDialogNewPassphrase => 'Новая парольная фраза';

  @override
  String get settingsDialogRepeatPassphrase => 'Повторите новую парольную фразу';

  @override
  String get settingsDialogChangePassphraseConfirm => 'Изменить парольную фразу';

  @override
  String get settingsDialogNewKitTitle => 'Новый комплект восстановления (Recovery Kit)';

  @override
  String get settingsDialogNewKitWarning =>
      'Новый ключ восстановления заменит текущий: старый комплект восстановления перестанет работать. Сохраните новый комплект, прежде чем закрывать это окно.';

  @override
  String get settingsDialogShowKit => 'Показать комплект восстановления';

  @override
  String get settingsDialogKitSaved => 'Комплект сохранён';

  @override
  String get settingsDialogGenerateKit => 'Создать новый комплект';

  @override
  String get settingsDialogAccountPasswordChanged => 'Пароль аккаунта изменён';

  @override
  String get settingsDialogAccountPasswordTitle => 'Изменить пароль аккаунта';

  @override
  String get settingsDialogCurrentPassword => 'Текущий пароль';

  @override
  String get settingsDialogNewPassword => 'Новый пароль';

  @override
  String settingsDialogPasswordMinLength(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Не менее $count символа',
      many: 'Не менее $count символов',
      few: 'Не менее $count символов',
      one: 'Не менее $count символа',
    );
    return '$_temp0';
  }

  @override
  String get settingsDialogVaultPassphraseUnchanged => 'Парольная фраза хранилища при этом не меняется.';

  @override
  String get aiProviderDialogAddTitle => 'Добавить ИИ-провайдер';

  @override
  String aiProviderDialogEditTitle(String name) {
    return 'Изменить «$name»';
  }

  @override
  String get aiProviderDialogProviderLabel => 'Провайдер';

  @override
  String get aiProviderDialogNameLabel => 'Название';

  @override
  String get aiProviderDialogBaseUrlLabel => 'Базовый URL';

  @override
  String get aiProviderDialogInsecureHttp =>
      'Незашифрованный http к удалённому хосту: API-ключ и запросы будут передаваться открытым текстом. Используйте https.';

  @override
  String get aiProviderDialogChatModelLabel => 'Модель для чата';

  @override
  String get aiProviderDialogEmbeddingModelLabel => 'Модель эмбеддингов (необязательно)';

  @override
  String get aiProviderDialogApiKeyLabel => 'API-ключ';

  @override
  String get aiProviderDialogApiKeyStoredHint => 'Ключ сохранён — введите новый, чтобы заменить его';

  @override
  String get aiProviderDialogApiKeyOptionalHint => 'Необязателен для локальных провайдеров';

  @override
  String get aiProviderDialogApiKeyHelp =>
      'Сохраняется в хранилище как зашифрованный секрет. Подсистема ИИ не может его прочитать: ключ только подставляется в HTTP-заголовок запросов.';

  @override
  String get aiProviderDialogRemoveApiKey => 'Удалить сохранённый API-ключ';

  @override
  String get aiProviderDialogPrivacyProfileLabel => 'Профиль приватности';

  @override
  String aiProviderDialogLocalProfileWarning(String profile) {
    return 'Профиль «$profile» передаёт больше контекста. Используйте его только с моделями, работающими на компьютерах под вашим контролем.';
  }

  @override
  String get aiProviderDialogTimeoutLabel => 'Тайм-аут (с)';

  @override
  String get aiProviderDialogStreaming => 'Потоковый вывод';

  @override
  String get aiProviderDialogToolCalling => 'Вызов инструментов';

  @override
  String get aiProviderDialogDefault => 'По умолчанию';

  @override
  String get aiProviderDialogHealthOk => 'Проверка подключения пройдена';

  @override
  String get aiProviderDialogHealthFailed => 'Проверка подключения не пройдена';

  @override
  String aiProviderDialogHealthModels(String models) {
    return 'Модели: $models';
  }

  @override
  String get aiProviderDialogSaveAndTest => 'Сохранить и проверить';

  @override
  String get runFlowVariablesPreview => 'Предпросмотр';

  @override
  String get runFlowConfirmTitle => 'Выполнить команду?';

  @override
  String runFlowLocalRules(String reasons) {
    return 'Локальные правила: $reasons.';
  }

  @override
  String runFlowDeclaredVsLocal(String declared, String effective) {
    return 'Заявленный уровень риска — «$declared»; по локальным правилам — «$effective».';
  }

  @override
  String runFlowAckDestructive(String host) {
    return 'Я понимаю, что эта команда может удалить или уничтожить данные на хосте $host';
  }

  @override
  String runFlowAckModifying(String host) {
    return 'Я понимаю, что эта команда изменяет состояние хоста $host';
  }

  @override
  String get runFlowAckUnknown => 'Команда проверена мной; её действие неизвестно локальным правилам';

  @override
  String get runFlowRun => 'Выполнить';

  @override
  String get runFlowPickHostTitle => 'На каком хосте выполнить?';

  @override
  String get runFlowOpenTerminalFirst => 'Сначала откройте вкладку терминала.';

  @override
  String get snippetEditorTitleNew => 'Новый сниппет';

  @override
  String get snippetEditorTitleEdit => 'Изменить сниппет';

  @override
  String get snippetEditorAiDraftBanner =>
      'Черновик подготовлен ИИ. Перед сохранением проверьте команду и уровень риска.';

  @override
  String get snippetEditorTemplateLabel => 'Шаблон команды';

  @override
  String get snippetEditorShellLabel => 'Оболочка / диалект (необязательно)';

  @override
  String get snippetEditorVariables => 'Переменные';

  @override
  String get snippetEditorVariableDefault => 'По умолчанию';

  @override
  String get snippetEditorRisk => 'Уровень риска';

  @override
  String get snippetEditorEffective => 'Итоговый риск: ';

  @override
  String snippetEditorLocalRules(String detail) {
    return 'Локальные правила: $detail';
  }

  @override
  String get snippetPickerTitle => 'Выполнить сниппет';

  @override
  String get snippetPickerSearchHint => 'Поиск сниппетов';

  @override
  String get snippetsTypeLabel => 'Тип';

  @override
  String snippetsSubtitle(String syntax) {
    return 'Многоразовые команды с $syntax. Перед выполнением всегда показываются команда, хост и уровень риска.';
  }

  @override
  String get snippetsNewButton => 'Новый сниппет';

  @override
  String get snippetsSearchHint => 'Поиск по тексту или смыслу, например «место на диске»';

  @override
  String get snippetsAllTypes => 'Все типы';

  @override
  String get snippetsEmptyTitle => 'Сниппетов пока нет';

  @override
  String get snippetsEmptyMessage => 'Сохраняйте часто используемые команды — или поручите ИИ подготовить их.';

  @override
  String get snippetsNothingFound => 'Ничего не найдено';

  @override
  String get snippetsDraftedByAi => 'Подготовлен ИИ';

  @override
  String get snippetsSemanticMatch => 'Совпадение по смыслу';

  @override
  String snippetsUsedCount(int count) {
    return 'использований: $count';
  }

  @override
  String get snippetsInsertIntoTerminal => 'Вставить в терминал';

  @override
  String get snippetsRun => 'Выполнить';

  @override
  String snippetsDeleteTitle(String name) {
    return 'Удалить «$name»?';
  }

  @override
  String get snippetsDeleteMessage => 'Сниппет будет удалён из этого хранилища.';

  @override
  String get aiChatSubtitle =>
      'Ответы никогда не выполняются сами: «Вставить» и «Выполнить…» всегда сначала спрашивают вас.';

  @override
  String get aiChatProviderLabel => 'ИИ-провайдер';

  @override
  String get aiChatNewConversation => 'Новый диалог';

  @override
  String aiChatRemoteProviderNotice(String profile, String description) {
    return 'Удалённый провайдер · профиль приватности «$profile»: $description. Пароли и ключи никогда не отправляются.';
  }

  @override
  String get aiChatLocalModelNotice => 'Локальная модель · данные не покидают этот компьютер.';

  @override
  String get aiChatNoProviderBanner => 'ИИ-провайдер не настроен.';

  @override
  String get aiChatEmptyTitle => 'Спросите что угодно о своих серверах';

  @override
  String get aiChatEmptyMessage => 'например, «найди самые большие файлы в /var/log» или «безопасно перезапусти nginx»';

  @override
  String get aiChatInputHint => 'Сообщение (Enter — отправить, Shift+Enter — новая строка)';

  @override
  String get aiChatStop => 'Остановить';

  @override
  String get aiChatSend => 'Отправить';

  @override
  String get aiChatIncludeSelection => 'Добавить выделенный текст терминала';

  @override
  String get aiChatSelectToInclude => 'Выделите текст в терминале, чтобы добавить его';

  @override
  String get aiChatErrorNoProvider => 'ИИ-провайдер не настроен. Добавьте его в разделе «Настройки → ИИ-провайдеры».';

  @override
  String get aiChatErrorRequestFailed => 'Запрос к ИИ не выполнен.';

  @override
  String get aiChatStoppedSuffix => '…(остановлено)';

  @override
  String paletteGenerateCommand(String query) {
    return 'Сгенерировать команду: «$query»';
  }

  @override
  String get paletteGenerateSubtitle => 'Спросить ИИ (никогда не выполняется автоматически)';

  @override
  String get paletteNewHost => 'Новый хост';

  @override
  String get paletteOpenAiChat => 'Открыть ИИ-чат';

  @override
  String get paletteLockVault => 'Заблокировать хранилище';

  @override
  String get paletteSearchHint => 'Найдите хост по имени или IP, сниппет, действие или опишите команду…';

  @override
  String get paletteAskSelectionHint => 'Спросите о выделенном выводе терминала…';

  @override
  String get paletteAiSuggestion => 'Предложение ИИ';

  @override
  String get paletteThinking => 'ИИ думает…';

  @override
  String get paletteErrorNoProvider =>
      'ИИ-провайдер не настроен. Добавьте Ollama, LM Studio или DeepSeek в настройках.';

  @override
  String paletteRiskMismatch(String aiRisk, String localRisk) {
    return 'ИИ оценил риск как «$aiRisk»; по локальным правилам — «$localRisk».';
  }

  @override
  String get codeBlocksInsert => 'Вставить';

  @override
  String get codeBlocksRun => 'Выполнить…';

  @override
  String get codeBlocksSaveAsSnippet => 'Сохранить как сниппет';

  @override
  String codeBlocksSnippetSaved(String name) {
    return 'Сниппет «$name» сохранён';
  }

  @override
  String codeBlocksProcessedLocally(String profile) {
    return 'Обработано локальной моделью · профиль «$profile»';
  }

  @override
  String codeBlocksSanitized(int count, String profile) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Перед отправкой с устройства применён профиль «$profile» · скрыто $count элемента',
      many: 'Перед отправкой с устройства применён профиль «$profile» · скрыто $count элементов',
      few: 'Перед отправкой с устройства применён профиль «$profile» · скрыто $count элемента',
      one: 'Перед отправкой с устройства применён профиль «$profile» · скрыт $count элемент',
    );
    return '$_temp0';
  }

  @override
  String get terminalEmptyTitle => 'Нет открытых сеансов';

  @override
  String terminalEmptyMessage(String shortcut) {
    return 'Подключитесь к хосту, чтобы открыть вкладку терминала ($shortcut).';
  }

  @override
  String get terminalConnectToHost => 'Подключиться к хосту';

  @override
  String get terminalCloseTab => 'Закрыть вкладку';

  @override
  String terminalNewTabTooltip(String shortcut) {
    return 'Новая вкладка ($shortcut)';
  }

  @override
  String get terminalSplitViewLater => 'Разделение окна появится в одном из следующих выпусков';

  @override
  String get terminalSnippets => 'Сниппеты';

  @override
  String get terminalAskAi => 'Спросить ИИ';

  @override
  String terminalHostKeyChangedTitle(String host) {
    return 'КЛЮЧ ХОСТА ИЗМЕНИЛСЯ: $host';
  }

  @override
  String terminalHostKeyChangedMessage(String keyType, String fingerprint) {
    return 'Проверка ключа хоста не пройдена: ключ сервера ИЗМЕНИЛСЯ. Это может означать атаку «человек посередине» (MITM). Подключение заблокировано.\nСервер предъявил ключ: $keyType $fingerprint. Если сервер действительно был переустановлен, удалите старый ключ в разделе «Настройки → Известные хосты» и переподключитесь.';
  }

  @override
  String terminalUnknownHostTitle(String host) {
    return 'Неизвестный хост $host';
  }

  @override
  String get terminalUnknownHostMessage =>
      'Подлинность этого хоста не может быть установлена. Прежде чем доверять ему, сверьте отпечаток ключа с администратором сервера.';

  @override
  String get terminalHostKeyReject => 'Отклонить';

  @override
  String get terminalHostKeyAcceptOnce => 'Принять один раз';

  @override
  String get terminalHostKeyAcceptAndSave => 'Принять и сохранить';

  @override
  String get terminalConnecting => 'Подключение';

  @override
  String terminalConnectingRoute(String route) {
    return 'Подключение: $route';
  }

  @override
  String get terminalReconnecting => 'Переподключение';

  @override
  String terminalReconnectingRoute(String route) {
    return 'Переподключение: $route';
  }

  @override
  String get terminalSessionEnded => 'Сеанс завершён';

  @override
  String get terminalDisconnected => 'Отключено';

  @override
  String get terminalConnectionClosed => 'Подключение закрыто.';

  @override
  String get terminalReconnect => 'Переподключиться';

  @override
  String terminalPasswordPromptTitle(String user, String host) {
    return 'Пароль для $user@$host';
  }

  @override
  String get terminalPasswordRetry => 'Доступ запрещён, попробуйте ещё раз.';

  @override
  String get terminalPasswordLabel => 'Пароль';

  @override
  String get terminalPasswordHelper =>
      'Используется только для этого подключения и не сохраняется. Чтобы пропустить этот шаг, сохраните пароль в настройках хоста.';

  @override
  String get sftpOpenPickerTitle => 'Открыть SFTP';

  @override
  String get sftpSubtitle => 'Просмотр и передача файлов по SSH.';

  @override
  String get sftpDisconnect => 'Отключиться';

  @override
  String get sftpNotConnectedTitle => 'Нет подключения';

  @override
  String get sftpNotConnectedMessage => 'Выберите хост, чтобы просматривать, передавать и редактировать его файлы.';

  @override
  String get sftpLocalPaneTitle => 'Это устройство';

  @override
  String get sftpUploadTooltip => 'Загрузить выбранный файл на сервер';

  @override
  String get sftpDownloadTooltip => 'Скачать выбранный файл';

  @override
  String get sftpNewFolder => 'Новый каталог';

  @override
  String sftpDeleteTitle(String name) {
    return 'Удалить «$name»?';
  }

  @override
  String get sftpDeleteFolderMessage => 'Каталог и всё его содержимое будут удалены на сервере.';

  @override
  String get sftpDeleteFileMessage => 'Файл будет удалён на сервере.';

  @override
  String get sftpRefresh => 'Обновить';

  @override
  String get sftpUp => 'На уровень выше';

  @override
  String sftpTransferDone(String size) {
    return 'Готово · $size';
  }

  @override
  String sftpTransferFailed(String detail) {
    return 'Ошибка: $detail';
  }

  @override
  String get sftpTransferCancelled => 'Отменено';

  @override
  String sftpTransferProgress(String done, String total) {
    return '$done из $total';
  }

  @override
  String sftpTransferSpeed(String size) {
    return '$size/с';
  }

  @override
  String get sftpToolbarQuickLook => 'Быстрый просмотр';

  @override
  String get sftpToolbarActions => 'Действия';

  @override
  String get sftpToolbarTransfers => 'Передачи';

  @override
  String get sftpToolbarEditing => 'Редактирование';

  @override
  String get sftpToolbarShowLocal => 'Показать локальные файлы рядом';

  @override
  String get sftpToolbarHideLocal => 'Скрыть локальные файлы';

  @override
  String get sftpToolbarSwitchHost => 'Подключиться к другому хосту';

  @override
  String get sftpSearchHint => 'Поиск в каталоге';

  @override
  String get sftpSearchClear => 'Очистить поиск';

  @override
  String sftpConnecting(String host) {
    return 'Подключение к $host…';
  }

  @override
  String get sftpPathHint => 'Введите путь, например /var/www или ~/logs';

  @override
  String sftpPathEditTooltip(String shortcut) {
    return 'Щёлкните или нажмите $shortcut, чтобы ввести путь';
  }

  @override
  String get sftpCopyPath => 'Скопировать путь';

  @override
  String get sftpCopyWhatPath => 'путь';

  @override
  String get sftpCopyWhatName => 'имя';

  @override
  String get sftpColumnName => 'Имя';

  @override
  String get sftpColumnSize => 'Размер';

  @override
  String get sftpColumnKind => 'Тип';

  @override
  String get sftpColumnModified => 'Изменён';

  @override
  String get sftpColumnPermissions => 'Права';

  @override
  String get sftpColumnOwner => 'Владелец';

  @override
  String get sftpColumnGroup => 'Группа';

  @override
  String get sftpFoldersFirst => 'Каталоги сначала';

  @override
  String get sftpShowHidden => 'Показывать скрытые файлы';

  @override
  String get sftpResetColumns => 'Сбросить столбцы';

  @override
  String get sftpExpand => 'Развернуть';

  @override
  String get sftpCollapse => 'Свернуть';

  @override
  String get sftpEmptyFolder => 'Каталог пуст';

  @override
  String sftpNoMatches(String query) {
    return 'В этом каталоге нет совпадений с «$query»';
  }

  @override
  String sftpSymlinkTooltip(String target) {
    return 'Символическая ссылка → $target';
  }

  @override
  String get sftpDanglingLink => 'Битая ссылка';

  @override
  String get sftpKindFolder => 'Каталог';

  @override
  String get sftpKindText => 'Текст';

  @override
  String get sftpKindDocument => 'Документ';

  @override
  String sftpKindDocumentFormat(String format) {
    return 'Документ $format';
  }

  @override
  String get sftpKindExecutable => 'Исполняемый файл';

  @override
  String get sftpKindShellScript => 'Скрипт оболочки';

  @override
  String sftpKindImage(String format) {
    return 'Изображение $format';
  }

  @override
  String sftpKindArchive(String format) {
    return 'Архив $format';
  }

  @override
  String get sftpKindLog => 'Журнал';

  @override
  String get sftpKindConfig => 'Конфигурация';

  @override
  String get sftpKindPdf => 'Документ PDF';

  @override
  String get sftpKindAudio => 'Аудио';

  @override
  String get sftpKindVideo => 'Видео';

  @override
  String get sftpKindFont => 'Шрифт';

  @override
  String get sftpKindKey => 'Ключ или сертификат';

  @override
  String get sftpKindDatabase => 'База данных';

  @override
  String get sftpKindSymlink => 'Символическая ссылка';

  @override
  String sftpKindSymlinkTo(String kind) {
    return 'Ссылка → $kind';
  }

  @override
  String get sftpKindSpecial => 'Специальный файл';

  @override
  String sftpStatusFolders(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count каталога',
      many: '$count каталогов',
      few: '$count каталога',
      one: '$count каталог',
    );
    return '$_temp0';
  }

  @override
  String sftpStatusFiles(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count файла',
      many: '$count файлов',
      few: '$count файла',
      one: '$count файл',
    );
    return '$_temp0';
  }

  @override
  String sftpStatusSummary(String folders, String files, String size) {
    return '$folders, $files, $size';
  }

  @override
  String sftpStatusSelection(String selected, String total, String size) {
    return 'Выбрано $selected из $total, $size';
  }

  @override
  String sftpStatusEditing(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Редактируются $count файла',
      many: 'Редактируются $count файлов',
      few: 'Редактируются $count файла',
      one: 'Редактируется $count файл',
    );
    return '$_temp0';
  }

  @override
  String get sftpStatusSecure => 'Зашифрованное SFTP-соединение по SSH';

  @override
  String get sftpApplications => 'Программы';

  @override
  String get sftpChooseApplication => 'Выбрать программу';

  @override
  String errorEditorLaunchFailed(String detail) {
    return 'Не удалось открыть редактор. Выберите другую программу через «Открыть в программе…». Сообщение системы: $detail';
  }

  @override
  String errorApplicationPickerFailed(String detail) {
    return 'Не удалось открыть выбор программы. Сообщение системы: $detail';
  }

  @override
  String get sftpActionOpenWith => 'Открыть в программе…';

  @override
  String get sftpActionDownload => 'Скачать…';

  @override
  String get sftpActionUploadHere => 'Загрузить сюда…';

  @override
  String get sftpActionNewFile => 'Новый файл';

  @override
  String get sftpActionDuplicate => 'Дублировать';

  @override
  String get sftpActionGetInfo => 'Свойства';

  @override
  String get sftpActionCopyName => 'Скопировать имя';

  @override
  String get sftpUntitledFolder => 'новый каталог';

  @override
  String get sftpUntitledFile => 'без названия.txt';

  @override
  String get sftpDuplicateSuffix => 'копия';

  @override
  String get sftpNameInvalid => 'Имя не может быть пустым или содержать «/».';

  @override
  String sftpDeleteManyTitle(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Удалить $count объекта?',
      many: 'Удалить $count объектов?',
      few: 'Удалить $count объекта?',
      one: 'Удалить $count объект?',
    );
    return '$_temp0';
  }

  @override
  String get sftpDeleteManyMessage =>
      'Выбранные объекты будут удалены на сервере, каталоги — вместе со всем содержимым. Это действие нельзя отменить.';

  @override
  String sftpUploadStarted(int count, String folder) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Загружаются $count объекта в $folder',
      many: 'Загружаются $count объектов в $folder',
      few: 'Загружаются $count объекта в $folder',
      one: 'Загружается $count объект в $folder',
    );
    return '$_temp0';
  }

  @override
  String sftpDownloadStarted(int count, String folder) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Скачиваются $count объекта в $folder',
      many: 'Скачиваются $count объектов в $folder',
      few: 'Скачиваются $count объекта в $folder',
      one: 'Скачивается $count объект в $folder',
    );
    return '$_temp0';
  }

  @override
  String sftpMoved(int count, String folder) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Перемещено $count объекта в $folder',
      many: 'Перемещено $count объектов в $folder',
      few: 'Перемещено $count объекта в $folder',
      one: 'Перемещён $count объект в $folder',
    );
    return '$_temp0';
  }

  @override
  String sftpDragItems(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count объекта',
      many: '$count объектов',
      few: '$count объекта',
      one: '$count объект',
    );
    return '$_temp0';
  }

  @override
  String get sftpDisconnectEditsTitle => 'Завершить редактирование и отключиться?';

  @override
  String sftpDisconnectEditsMessage(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Сейчас редактируются $count файла с этого хоста.',
      many: 'Сейчас редактируются $count файлов с этого хоста.',
      few: 'Сейчас редактируются $count файла с этого хоста.',
      one: 'Сейчас редактируется $count файл с этого хоста.',
    );
    return '$_temp0 Несохранённые изменения сначала будут загружены на сервер; если это не удастся, рабочие копии сохранятся для восстановления.';
  }

  @override
  String sftpInfoTitle(String name) {
    return 'Свойства «$name»';
  }

  @override
  String get sftpInfoWhere => 'Расположение';

  @override
  String sftpInfoSizeBytes(String size, String bytes) {
    return '$size ($bytes Б)';
  }

  @override
  String get sftpInfoLinkTarget => 'Указывает на';

  @override
  String get sftpInfoSymlinkNote => 'Права символической ссылки применяются к объекту, на который она указывает.';

  @override
  String get sftpInfoOthers => 'Остальные';

  @override
  String get sftpInfoRead => 'Чтение';

  @override
  String get sftpInfoWrite => 'Запись';

  @override
  String get sftpInfoExecute => 'Выполнение';

  @override
  String get sftpInfoOctal => 'Восьмеричный код';

  @override
  String get sftpInfoOctalInvalid => 'Введите 3 или 4 цифры от 0 до 7.';

  @override
  String get sftpInfoApply => 'Применить';

  @override
  String get sftpInfoPermissionsSaved => 'Права изменены.';

  @override
  String get sftpQuickLookFolder => 'У каталогов нет предпросмотра.';

  @override
  String get sftpQuickLookNoPreview => 'Для файлов этого типа нет предпросмотра.';

  @override
  String sftpQuickLookTooLarge(String size) {
    return 'Изображение слишком большое для предпросмотра ($size).';
  }

  @override
  String get sftpQuickLookImageError => 'Не удалось показать изображение.';

  @override
  String sftpQuickLookTruncated(String shown, String total) {
    return 'Показаны первые $shown из $total.';
  }

  @override
  String get sftpQuickLookMemoryNote => 'Загружено только в память — на устройстве ничего не сохраняется.';

  @override
  String get sftpQuickLookOpenInEditor => 'Открыть в редакторе';

  @override
  String get sftpActivityHide => 'Скрыть панель';

  @override
  String get sftpTransfersClear => 'Очистить завершённые';

  @override
  String get sftpTransfersEmpty => 'Передач пока нет. Перетащите файлы в список или выберите «Загрузить сюда…».';

  @override
  String get sftpTransferDirectionUpload => 'Выгрузка';

  @override
  String get sftpTransferDirectionDownload => 'Загрузка';

  @override
  String get sftpTransferQueued => 'В очереди…';

  @override
  String sftpTransferTo(String path) {
    return 'в $path';
  }

  @override
  String get sftpEditingEmpty =>
      'Нет редактируемых файлов. Дважды щёлкните файл, чтобы открыть его в программе по умолчанию, — каждое сохранение загружается на сервер автоматически.';

  @override
  String get sftpEditStatusOpening => 'Открывается…';

  @override
  String get sftpEditStatusSynced => 'Синхронизирован';

  @override
  String get sftpEditStatusModified => 'Изменён';

  @override
  String sftpEditStatusUploading(int percent) {
    return 'Выгрузка $percent %';
  }

  @override
  String get sftpEditStatusUploadingUnknown => 'Выгрузка…';

  @override
  String get sftpEditStatusConflict => 'Конфликт';

  @override
  String get sftpEditStatusError => 'Ошибка выгрузки';

  @override
  String get sftpEditStatusClosed => 'Закрыт';

  @override
  String sftpEditLastSynced(String time) {
    return 'выгружен $time';
  }

  @override
  String sftpEditOpenedWith(String app) {
    return 'в $app';
  }

  @override
  String get sftpEditRevealFinder => 'Показать в Finder';

  @override
  String get sftpEditRevealExplorer => 'Показать в Проводнике';

  @override
  String get sftpEditRevealOther => 'Показать в папке';

  @override
  String get sftpEditReopen => 'Открыть снова';

  @override
  String get sftpEditSyncNow => 'Синхронизировать сейчас';

  @override
  String get sftpEditStop => 'Завершить редактирование';

  @override
  String get sftpEditResolve => 'Разрешить…';

  @override
  String sftpEditOpened(String name) {
    return 'Файл «$name» открыт. Каждое сохранение загружается на сервер автоматически.';
  }

  @override
  String sftpEditStopped(String name) {
    return 'Редактирование «$name» завершено.';
  }

  @override
  String sftpEditStopFailed(String name, String detail) {
    return 'Файл «$name» не загружен на сервер ($detail). Редактирование продолжается.';
  }

  @override
  String sftpEditTooLarge(String name, String size, String limit) {
    return 'Файл «$name» слишком большой для редактирования ($size, предел $limit).';
  }

  @override
  String get sftpEditNotAFile => 'Для редактирования можно открыть только обычный файл.';

  @override
  String get sftpEditHintTitle => 'Редактирование в другой программе';

  @override
  String sftpEditHintBody(String name) {
    return '«$name» откроется в программе по умолчанию, вне ConsoleCrypt. Пока вы редактируете, на этом устройстве хранится личная рабочая копия (доступна только вашему пользователю), а каждое сохранение автоматически загружается на сервер.';
  }

  @override
  String get sftpEditHintControl =>
      'ConsoleCrypt не контролирует, что эта программа делает с файлом, — например, её автосохранение, резервные копии или облачную синхронизацию. Рабочая копия удаляется, когда вы завершаете редактирование.';

  @override
  String get sftpEditHintDontShow => 'Больше не показывать';

  @override
  String sftpConflictTitle(String name) {
    return 'Файл «$name» изменён на сервере';
  }

  @override
  String get sftpConflictMessage =>
      'Кто-то изменил этот файл на сервере после того, как вы его открыли. Ваше последнее сохранение не загружено на сервер, поэтому ничего не перезаписано. Выберите действие:';

  @override
  String get sftpConflictDeletedMessage =>
      'Файл удалён или перемещён на сервере после того, как вы его открыли. Ваше последнее сохранение не загружено на сервер.';

  @override
  String sftpConflictRemoteMeta(String size, String time) {
    return 'Версия на сервере: $size, изменена $time';
  }

  @override
  String get sftpConflictOverwriteTitle => 'Перезаписать файл на сервере';

  @override
  String get sftpConflictOverwriteBody =>
      'Загрузить вашу версию на сервер. Другие изменения на сервере будут потеряны.';

  @override
  String get sftpConflictRecreateTitle => 'Загрузить мою версию заново';

  @override
  String get sftpConflictRecreateBody => 'Снова создать файл на сервере из вашей локальной копии.';

  @override
  String get sftpConflictKeepTitle => 'Сохранить обе версии';

  @override
  String sftpConflictKeepBody(String name) {
    return 'Сохранить версию с сервера рядом с вашей копией как «$name.remote-…» и открыть её для сравнения. Следующее сохранение загрузит на сервер вашу версию.';
  }

  @override
  String get sftpConflictDiscardTitle => 'Отменить мои изменения';

  @override
  String get sftpConflictDiscardBody => 'Заменить вашу локальную копию версией с сервера.';

  @override
  String get sftpConflictLater => 'Решить позже';

  @override
  String get sftpLeftoversTitle => 'Восстановить несохранённые правки?';

  @override
  String get sftpLeftoversMessage =>
      'ConsoleCrypt был закрыт, пока эти удалённые файлы редактировались. Их рабочие копии всё ещё хранятся на этом устройстве.';

  @override
  String get sftpLeftoverModified => 'Изменён локально, ещё не на сервере';

  @override
  String get sftpLeftoverUnchanged => 'Локальных изменений нет';

  @override
  String get sftpLeftoverUnknownHost => 'Неизвестный хост — можно только удалить';

  @override
  String get sftpLeftoverUnreadable => 'Данные сеанса повреждены — можно только удалить';

  @override
  String get sftpLeftoversRecover => 'Восстановить выбранные';

  @override
  String get sftpLeftoversRecoverHint =>
      'Выбранные файлы будут загружены на сервер после проверки конфликтов и останутся открытыми для редактирования. Остальные рабочие копии будут удалены.';

  @override
  String get sftpLeftoversDiscardAll => 'Удалить все';

  @override
  String get sftpLeftoversLater => 'Позже';

  @override
  String sftpLeftoversRecovered(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Восстановлено $count файла',
      many: 'Восстановлено $count файлов',
      few: 'Восстановлено $count файла',
      one: 'Восстановлен $count файл',
    );
    return '$_temp0';
  }

  @override
  String sftpLeftoversFailed(String name, String error) {
    return 'Не удалось восстановить «$name»: $error';
  }

  @override
  String get sftpDebugMenu => 'Имитация (демо)';

  @override
  String get sftpDebugSave => 'Имитировать сохранение в редакторе';

  @override
  String get sftpDebugRemoteChange => 'Имитировать изменение на сервере';

  @override
  String get sftpDebugFailUpload => 'Сделать следующую выгрузку неудачной';

  @override
  String get sftpDebugFailTransfer => 'Сделать следующую передачу неудачной (демо)';

  @override
  String get tunnelsTitle => 'Туннели';

  @override
  String get tunnelsSubtitle =>
      'Проброс портов через SSH. Туннели работают на этом устройстве, а их настройки синхронизируются вместе с хранилищем.';

  @override
  String get tunnelsNew => 'Новый туннель';

  @override
  String get tunnelsEmptyTitle => 'Туннелей пока нет';

  @override
  String get tunnelsEmptyMessage => 'Например, 127.0.0.1:15432 → db.internal:5432 через jump-хост.';

  @override
  String get tunnelsDeletedHost => '(хост удалён)';

  @override
  String tunnelsStatusRunning(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Работает · $count подключения',
      many: 'Работает · $count подключений',
      few: 'Работает · $count подключения',
      one: 'Работает · $count подключение',
    );
    return '$_temp0';
  }

  @override
  String tunnelsStatusRunningSince(int count, String ago) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Запущен $ago · $count подключения',
      many: 'Запущен $ago · $count подключений',
      few: 'Запущен $ago · $count подключения',
      one: 'Запущен $ago · $count подключение',
    );
    return '$_temp0';
  }

  @override
  String get tunnelsStatusStarting => 'Запускается…';

  @override
  String get tunnelsStatusFailed => 'Ошибка';

  @override
  String tunnelsStatusFailedDetail(String detail) {
    return 'Ошибка: $detail';
  }

  @override
  String get tunnelsStatusStopped => 'Остановлен';

  @override
  String tunnelsPublicBindTooltip(String address) {
    return 'Привязан к $address: доступен из сети';
  }

  @override
  String tunnelsSummaryRemote(String bind, String target) {
    return 'на сервере $bind → $target';
  }

  @override
  String tunnelsTileSubtitle(String summary, String host) {
    return '$summary  ·  через $host';
  }

  @override
  String tunnelsDeleteTitle(String name) {
    return 'Удалить туннель «$name»?';
  }

  @override
  String get tunnelsDeleteMessage => 'Туннель будет остановлен и удалён из хранилища.';

  @override
  String get tunnelsErrorChooseHost => 'Выберите хост, через который пойдёт туннель';

  @override
  String get tunnelsEdit => 'Изменить туннель';

  @override
  String get tunnelsHostLabel => 'Через хост (SSH-подключение, включая цепочку jump-хостов)';

  @override
  String get tunnelsBindAddressRemote => 'Адрес привязки на сервере';

  @override
  String get tunnelsBindAddressLocal => 'Адрес привязки (это устройство)';

  @override
  String get tunnelsPortLabel => 'Порт';

  @override
  String get tunnelsTargetRemote => 'Целевой адрес на этом устройстве';

  @override
  String get tunnelsTargetLocal => 'Целевой хост (с точки зрения сервера)';

  @override
  String get tunnelsPublicBindTitle => 'Привязка не к loopback-интерфейсу';

  @override
  String tunnelsPublicBindWarningRemote(String address) {
    return 'Адрес «$address» открывает доступ к пробрасываемому порту из сети сервера (требуется GatewayPorts). Любой, кто может к нему подключиться, сможет воспользоваться туннелем.';
  }

  @override
  String tunnelsPublicBindWarningLocal(String address) {
    return 'Адрес «$address» открывает доступ к туннелю из вашей локальной сети. Любой, кто может подключиться к этому устройству, сможет им воспользоваться. Используйте 127.0.0.1, если только это не сделано намеренно.';
  }

  @override
  String get tunnelsPublicBindAck => 'Я понимаю, что этот туннель доступен из сети';

  @override
  String get tunnelsAutoStart => 'Запускать автоматически после разблокировки';

  @override
  String get approvalWaitTitle => 'Подтверждение этого устройства';

  @override
  String approvalWaitSubtitle(String device) {
    return 'На устройстве, где ConsoleCrypt разблокирован, откройте раздел «Устройства» и проверьте запрос от «$device». Код, показанный там, должен в точности совпадать с этим: сравните все шесть групп цифр на обоих устройствах.';
  }

  @override
  String get approvalWaitSubtitleUnnamed =>
      'На устройстве, где ConsoleCrypt разблокирован, откройте раздел «Устройства» и проверьте запрос от этого устройства. Код, показанный там, должен в точности совпадать с этим: сравните все шесть групп цифр на обоих устройствах.';

  @override
  String get approvalWaitCodeLabel => 'Код проверки этого устройства';

  @override
  String get approvalWaitWaiting => 'Ожидание подтверждения… Запрос действителен 24 часа.';

  @override
  String get approvalWaitMismatchWarning =>
      'Если коды не совпадают, отклоните запрос на другом устройстве. Несовпадение может означать, что кто-то (даже скомпрометированный сервер) пытается добавить собственное устройство.';

  @override
  String get approvalWaitSimulateApproval => 'Имитировать подтверждение (mock)';

  @override
  String get createVaultDefaultName => 'Личное';

  @override
  String get createVaultTitle => 'Создайте парольную фразу хранилища';

  @override
  String get createVaultSubtitleLocal => 'Она шифрует все данные этого профиля на этом устройстве.';

  @override
  String get createVaultSubtitleSynced =>
      'Она шифрует ваше хранилище до того, как что-либо будет синхронизировано. Сервер никогда её не видит, и она отличается от пароля вашего аккаунта.';

  @override
  String get createVaultNameLabel => 'Название хранилища';

  @override
  String get createVaultPassphraseLabel => 'Парольная фраза хранилища';

  @override
  String get createVaultRepeatPassphraseLabel => 'Повторите парольную фразу';

  @override
  String get createVaultPassphraseMismatch => 'Парольные фразы не совпадают';

  @override
  String get createVaultNoResetNotice =>
      'Сбросить эту парольную фразу за вас не сможет никто — даже администратор вашего сервера. Далее вы получите комплект восстановления — единственный другой способ открыть хранилище.';

  @override
  String get createVaultSubmit => 'Создать хранилище';

  @override
  String get recoveryKitTitle => 'Сохраните комплект восстановления (Recovery Kit)';

  @override
  String get recoveryKitSubtitle =>
      'Если вы забудете парольную фразу и потеряете доступ к доверенным устройствам, этот комплект — единственный способ снова открыть хранилище. Храните его офлайн: распечатайте или перепишите от руки.';

  @override
  String get recoveryKitLostFromMemory =>
      'Комплекта восстановления больше нет в памяти (приложение было перезапущено). Создайте новый — предыдущий комплект перестанет действовать.';

  @override
  String get recoveryKitGenerateNew => 'Создать новый комплект восстановления';

  @override
  String get recoveryKitPrintUnavailable => 'Печать появится в M5. Пока перепишите слова от руки.';

  @override
  String get recoveryKitPrint => 'Печать / Сохранить как PDF';

  @override
  String get recoveryKitCopyWords => 'Скопировать слова';

  @override
  String get recoveryKitSavedConfirmation => 'Комплект восстановления сохранён в надёжном месте';

  @override
  String get recoveryKitRevealFirst => 'Сначала покажите комплект';

  @override
  String get recoveryKitKeyNotStored =>
      'Ни ConsoleCrypt, ни ваш сервер никогда не хранят ключ восстановления — только конверт, который им открывается.';

  @override
  String get recoveryKitVaultId => 'ID хранилища';

  @override
  String get recoveryKitServer => 'Сервер';

  @override
  String get recoveryKitNoServerCopy => 'Локальный профиль — копии на сервере нет';

  @override
  String get recoveryKitCreated => 'Создан';

  @override
  String get recoveryKitPrivacyHint => 'Убедитесь, что никто не видит ваш экран.';

  @override
  String get recoveryKitShow => 'Показать комплект восстановления';

  @override
  String get verifyKitTitle => 'Подтвердите комплект восстановления';

  @override
  String get verifyKitNoKit => 'Комплекта восстановления нет в памяти. Вернитесь назад, чтобы создать его.';

  @override
  String get verifyKitBackToRecoveryKit => 'Назад к комплекту восстановления';

  @override
  String get verifyKitSubtitle =>
      'Введите запрошенные слова из комплекта, чтобы подтвердить, что он сохранён. Этот шаг обязателен.';

  @override
  String verifyKitWordLabel(int index) {
    return 'Слово № $index';
  }

  @override
  String get verifyKitMismatch =>
      'Эти слова не совпадают с вашим комплектом восстановления. Проверьте комплект и попробуйте снова.';

  @override
  String get verifyKitBackToKit => 'Назад к комплекту';

  @override
  String get verifyKitSubmit => 'Проверить';

  @override
  String get onboardingLocalNoticeTitle => 'Нет облачной копии';

  @override
  String get onboardingLocalNoticeSubtitle =>
      'Этот профиль хранится только на этом устройстве — ни на одном сервере его копии нет. Храните комплект восстановления и делайте резервные копии.';

  @override
  String get onboardingLocalNoticeKitTitle => 'Храните комплект восстановления в надёжном месте';

  @override
  String get onboardingLocalNoticeKitBody => 'Он откроет хранилище, если вы забудете парольную фразу.';

  @override
  String get onboardingLocalNoticeBackupsTitle => 'Делайте зашифрованные резервные копии';

  @override
  String get onboardingLocalNoticeBackupsBody =>
      'Экспортируйте файл .ccbackup или включите автоматическое резервное копирование ConsoleCrypt в папку.';

  @override
  String get onboardingLocalNoticeLossTitle => 'Потеря устройства без резервной копии = потеря данных';

  @override
  String get onboardingLocalNoticeLossBody =>
      'Копии на сервере, из которой можно было бы восстановить данные, нет. Синхронизацию можно включить позже в настройках.';

  @override
  String get onboardingLocalNoticeSetUpBackups => 'Настроить автоматическое резервное копирование';

  @override
  String get onboardingLocalNoticeContinue => 'Понятно — продолжить';

  @override
  String unlockTitle(String name) {
    return 'Разблокировать хранилище «$name»';
  }

  @override
  String get unlockTitleGeneric => 'Разблокировать хранилище';

  @override
  String get unlockPassphraseLabel => 'Парольная фраза хранилища';

  @override
  String get unlockSubmit => 'Разблокировать';

  @override
  String get unlockOsAuthReason => 'разблокировать хранилище ConsoleCrypt';

  @override
  String unlockWithOsAuth(String method) {
    return 'Разблокировать ($method)';
  }

  @override
  String get unlockUntrustedTitle => 'Это устройство пока не доверенное';

  @override
  String get unlockUntrustedMessage =>
      'Разблокируйте хранилище парольной фразой или одобрите это устройство с того, которым уже пользуетесь. Для подтверждения нужно сравнить код проверки на экранах обоих устройств.';

  @override
  String get unlockApproveFromOtherDevice => 'Одобрить с другого устройства';

  @override
  String get unlockForgotPassphrase => 'Забыли парольную фразу?';

  @override
  String get unlockSignOut => 'Выйти';

  @override
  String get recoveryTitle => 'Восстановление доступа к хранилищу';

  @override
  String get recoverySubtitle =>
      'Забыли парольную фразу хранилища? Откройте хранилище другим способом и задайте новую парольную фразу.';

  @override
  String get recoveryMethodRecoveryKey => 'Ключ восстановления';

  @override
  String get recoveryMethodTrustedDevice => 'Это доверенное устройство';

  @override
  String get recoveryWordsInstructions =>
      'Введите 24 слова из комплекта восстановления или вставьте текст его QR-кода.';

  @override
  String get recoveryInputHint => 'word1 word2 … word24  или  consolecrypt-recovery:v1:…';

  @override
  String get recoveryQrRecognised => 'Текст QR-кода распознан';

  @override
  String get recoveryQrIncomplete => 'Текст QR-кода неполный';

  @override
  String recoveryWordCount(int count, int total) {
    return 'Слов: $count из $total';
  }

  @override
  String recoveryTrustedDeviceInfo(String method) {
    return '$method выдаёт ключ разблокировки этого устройства, чтобы открыть хранилище. Затем задайте новую парольную фразу; другие устройства продолжат работать.';
  }

  @override
  String get recoveryDeviceNotTrusted =>
      'Это устройство не является доверенным для хранилища. Используйте ключ восстановления.';

  @override
  String get recoveryNewPassphraseLabel => 'Новая парольная фраза хранилища';

  @override
  String get recoveryRepeatNewPassphraseLabel => 'Повторите новую парольную фразу';

  @override
  String get recoverySubmitTrustedDevice => 'Пройти аутентификацию и задать парольную фразу';

  @override
  String get recoverySubmitRecoveryKey => 'Восстановить и задать парольную фразу';

  @override
  String get recoveryLostEverythingTitle =>
      'Потеряли парольную фразу, ключ восстановления и все доверенные устройства?';

  @override
  String get recoveryLostEverythingLocal =>
      'Этот локальный профиль можно вернуть только из файла .ccbackup, открыв его парольной фразой или ключом восстановления (экран приветствия → «Восстановить из резервной копии»). Без такого файла данные восстановить невозможно: так задумано — мастер-ключа нет ни у кого.';

  @override
  String get recoveryLostEverythingSynced =>
      'Старое хранилище не может расшифровать никто, включая ваш сервер, — так задумано. Вы можете восстановить аккаунт по e-mail и начать с нового, пустого хранилища.';

  @override
  String get shellToggleSidebar => 'Показать или скрыть боковую панель';

  @override
  String get shellNewConnection => 'Новое подключение';

  @override
  String shellNewConnectionTooltip(String shortcut) {
    return 'Новое подключение ($shortcut)';
  }

  @override
  String get shellSyncNever => 'Никогда';

  @override
  String get shellSyncDetails => 'Подробнее';

  @override
  String aboutTitle(String appName) {
    return 'О программе $appName';
  }

  @override
  String get aboutTagline => 'SSH-клиент со сквозным шифрованием';

  @override
  String aboutVersion(String version) {
    return 'Версия $version';
  }

  @override
  String get aboutAuthor => 'Автор';

  @override
  String get aboutLicenses => 'Лицензии';

  @override
  String get aboutLicenseClient => 'Клиент для компьютера';

  @override
  String get aboutLicenseServer => 'Сервер синхронизации';

  @override
  String get aboutOpenSource => 'ConsoleCrypt — программа с открытым исходным кодом. Ваши подключения в безопасности.';

  @override
  String get settingsAboutTitle => 'О программе';

  @override
  String get settingsAppearanceTitle => 'Оформление';

  @override
  String get settingsSecurityTitle => 'Безопасность';

  @override
  String get settingsGlassHelp =>
      'Насколько прозрачны боковая панель, панель инструментов, меню и диалоги. Окна безопасности всегда непрозрачны.';

  @override
  String get settingsGlassSolidReduceTransparency =>
      'Отображается непрозрачным: в настройках системы включено уменьшение прозрачности.';

  @override
  String get settingsGlassSolidRemote => 'Отображается непрозрачным в сеансе удалённого рабочего стола.';

  @override
  String get settingsGlassSolidBattery => 'Отображается непрозрачным для экономии заряда (режим энергосбережения).';

  @override
  String get settingsGlassSolidPerformance => 'Эффекты стекла упрощены ради производительности.';

  @override
  String get settingsSidebarLabel => 'Боковая панель';

  @override
  String get settingsSidebarFloating => 'Плавающая';

  @override
  String get settingsSidebarEdgeToEdge => 'От края до края';

  @override
  String get secretFieldCyrillicHint => 'В тексте есть кириллица — проверьте раскладку клавиатуры.';

  @override
  String get secretFieldNonLatinHint => 'В тексте есть буквы не из латиницы A–Z — проверьте раскладку клавиатуры.';

  @override
  String get passphraseLayoutNotice =>
      'Парольная фраза содержит буквы не из латиницы A–Z. Чтобы разблокировать хранилище, её нужно будет вводить в той же раскладке клавиатуры.';

  @override
  String get unlockDeleteProfile => 'Удалить этот профиль…';

  @override
  String get deleteProfileLocalWarning =>
      'Это хранилище есть только на этом устройстве. После удаления его можно вернуть только из файла резервной копии (.ccbackup), открыв его парольной фразой или ключом восстановления. Без такого файла все хосты, ключи и пароли из него будут потеряны навсегда.';

  @override
  String get deleteProfileAcknowledge => 'Я понимаю, что это действие необратимо';

  @override
  String runFlowConfirmTitleDestructive(String host) {
    return 'Выполнить разрушительную команду на «$host»?';
  }

  @override
  String get paletteGroupActions => 'Действия';

  @override
  String get paletteGroupAi => 'ИИ';

  @override
  String get settingsUiFontScale => 'Размер текста интерфейса';

  @override
  String get settingsUiScaleHelp =>
      '100% — новый базовый размер, равный прежним 90%. Можно выбрать от 80% до 140%. Размер текста терминала настраивается отдельно.';

  @override
  String get settingsAccentColor => 'Акцентный цвет';

  @override
  String get settingsBackgroundTint => 'Оттенок фона';

  @override
  String get settingsColorsHelp =>
      'Выберите цвет из палитры или введите любой HEX-код. Фон адаптируется к светлой или тёмной теме, текст остаётся читаемым.';

  @override
  String get settingsColorApply => 'Применить';

  @override
  String get settingsColorHex => 'HEX-код цвета';

  @override
  String get settingsColorInvalid => 'Введите шесть шестнадцатеричных цифр, например #4C8DFF.';

  @override
  String get settingsResetUi => 'Сбросить размер текста и цвета';

  @override
  String get settingsTerminalColorScheme => 'Цветовая тема терминала';

  @override
  String get settingsTerminalThemeHelp =>
      'Сразу применяется ко всем вкладкам терминала, независимо от цветов интерфейса.';

  @override
  String get settingsTerminalThemeSystem => 'Как в интерфейсе';

  @override
  String get settingsTerminalThemeDark => 'Графит';

  @override
  String get settingsTerminalThemeLight => 'Бумага';

  @override
  String get settingsTerminalThemeMidnight => 'Полночь';

  @override
  String get settingsTerminalThemeOcean => 'Океан';

  @override
  String get settingsTerminalThemeForest => 'Лес';

  @override
  String get settingsTerminalThemeAmber => 'Янтарь';

  @override
  String get settingsTerminalPreview => 'Предпросмотр терминала';

  @override
  String get settingsAppearanceLocalHint => 'Сохраняется на этом устройстве. Перезапуск не требуется.';

  @override
  String get errorSecureStore =>
      'Системное хранилище ключей недоступно. На macOS связка «Вход» запрашивает пароль входа в Mac, а не пароль хранилища. Если вы меняли пароль Mac, связка может использовать прежний. Откройте «Связка ключей», разблокируйте «Вход» и повторите открытие профиля. Не удаляйте и не сбрасывайте связку: в ней находятся ключи шифрования этого устройства.';

  @override
  String get welcomeResumeTitle => 'Открыть профиль';

  @override
  String get welcomeResumeHelp => 'Ваши профили сохранены на этом устройстве. Выберите профиль, чтобы продолжить.';

  @override
  String get welcomeKeychainHelp =>
      'При открытии профиля система может запросить доступ к связке ключей. На macOS нужен пароль связки «Вход» — обычно это пароль входа в Mac. Если он не подходит, разблокируйте «Вход» в приложении «Связка ключей»: после смены пароля Mac там мог остаться прежний пароль.';

  @override
  String get reopenLastProfile => 'Открывать последний профиль при запуске';

  @override
  String get reopenLastProfileHelp => 'Может вызывать системный запрос доступа к связке ключей при запуске.';

  @override
  String get settingsTerminalThemeCustom => 'Своя схема';

  @override
  String get colorSpectrum => 'Спектр цветов';

  @override
  String get colorHue => 'Оттенок';

  @override
  String get colorSaturation => 'Насыщенность';

  @override
  String get colorBrightness => 'Яркость';

  @override
  String get commonThemeColor => 'Цвет';

  @override
  String get terminalThemeEdit => 'Настроить / импортировать…';

  @override
  String get terminalThemeEditorHelp =>
      'Нажмите на цвет, чтобы изменить его. Кнопка «Применить» обновит все вкладки терминала.';

  @override
  String get terminalThemeImport => 'Импорт темы';

  @override
  String get terminalThemeExport => 'Экспорт JSON';

  @override
  String get terminalThemeFormats =>
      'Импорт: iTerm .itermcolors (XML) или ConsoleCrypt .json, до 256 КБ. Экспорт: ConsoleCrypt .json.';

  @override
  String get terminalThemeImportError =>
      'Не удалось импортировать тему. Выберите корректный XML-файл iTerm или JSON ConsoleCrypt до 256 КБ. Сохранённая тема не изменилась.';

  @override
  String get terminalThemeExportError => 'Не удалось сохранить тему. Выберите другую папку и повторите попытку.';

  @override
  String get terminalColorBackground => 'Фон';

  @override
  String get terminalColorForeground => 'Текст';

  @override
  String get terminalColorCursor => 'Курсор';

  @override
  String get terminalColorSelection => 'Выделение';

  @override
  String terminalColorAnsi(int index) {
    return 'ANSI $index';
  }

  @override
  String terminalColorAnsiBright(int index) {
    return 'Яркий $index';
  }

  @override
  String get terminalThemeFileType => 'Цветовые схемы терминала';

  @override
  String get terminalThemeJsonType => 'Тема ConsoleCrypt';

  @override
  String get terminalSampleSelection => ' выделенный текст ';

  @override
  String get settingsGlassRetryEffects => 'Включить эффекты снова';

  @override
  String get deviceUnlockTitle => 'Разблокировка отпечатком пальца';

  @override
  String get deviceUnlockHelp =>
      'Система выдаёт ключ хранилища только после отпечатка пальца, лица или пароля устройства.';

  @override
  String get deviceUnlockLocalOnly =>
      'Только для этого хранилища на этом устройстве. Парольная фраза и ключ восстановления остаются доступны.';

  @override
  String get deviceUnlockEnableReason => 'разрешить разблокировку хранилища ConsoleCrypt на этом устройстве';

  @override
  String get deviceUnlockNeedsTrust => 'Сначала разблокируйте хранилище парольной фразой на этом устройстве.';

  @override
  String get deviceUnlockNotEnrolled =>
      'Биометрия не настроена. Добавьте отпечаток пальца или распознавание лица в системных настройках устройства, затем проверьте доступность снова.';

  @override
  String get deviceUnlockUnsupported =>
      'Биометрическая разблокировка сейчас недоступна на этом устройстве. Проверьте поддержку и настройку биометрии в системных настройках.';

  @override
  String get deviceUnlockMacPassword =>
      'Сейчас macOS предлагает вход своим паролем. Для Touch ID нужен настроенный отпечаток: Системные настройки → Touch ID и пароль.';

  @override
  String get deviceUnlockRecoveryUnavailable =>
      'Разблокировка через это устройство выключена или сейчас недоступна. Используйте ключ восстановления. Включить её можно после входа: Настройки → Хранилище.';

  @override
  String get deviceUnlockRefresh => 'Проверить доступность снова';

  @override
  String deviceUnlockWithMethod(String method) {
    return 'Разблокировка через $method';
  }

  @override
  String get deviceUnlockMacPasswordName => 'пароль macOS';

  @override
  String get errorOsAuthInvalidated =>
      'Отпечатки или вход в систему изменились. Разблокируйте хранилище парольной фразой и снова включите разблокировку устройством: Настройки → Хранилище.';

  @override
  String deviceUnlockProtectedBySystem(String method) {
    return 'Защищено системой: ключ выдаётся только после проверки ($method).';
  }

  @override
  String get deviceUnlockConvenienceWarning =>
      'Разблокировка без пароля доступна любой программе вашего пользователя. Это удобство, а не защита.';

  @override
  String get deviceUnlockConvenienceConfirmTitle => 'Разблокировать без парольной фразы?';

  @override
  String get deviceUnlockConvenienceConfirmAction => 'Всё равно включить';

  @override
  String get deviceUnlockKeyringName => 'системная связка ключей';

  @override
  String deviceUnlockReofferMigratedTitle(String method) {
    return 'Снова включить $method?';
  }

  @override
  String get deviceUnlockReofferMigratedMessage =>
      'Разблокировка этим устройством теперь защищена системой. При обновлении она была выключена — включите её снова одним подтверждением.';

  @override
  String get deviceUnlockReofferInvalidatedTitle => 'Отпечатки изменились';

  @override
  String deviceUnlockReofferInvalidatedMessage(String method) {
    return 'Ключ $method больше не действует: изменился вход в систему. Снова включить разблокировку этим устройством?';
  }

  @override
  String get deviceUnlockReofferEnable => 'Включить';

  @override
  String get deviceUnlockReofferLater => 'Не сейчас';

  @override
  String get recoveryDeviceResetReason => 'сбросить парольную фразу хранилища ConsoleCrypt';

  @override
  String get workspaceToolsClose => 'Закрыть правую панель';

  @override
  String get workspaceToolsResize => 'Ширина правой панели';

  @override
  String get snippetPackage => 'Набор';

  @override
  String get snippetPackageHint => 'Выберите существующее название или введите новое. Пустое поле — без набора.';

  @override
  String get snippetAllPackages => 'Все сниппеты';

  @override
  String get snippetNoPackage => 'Без набора';

  @override
  String get snippetStarterCatalog => 'Базовые наборы';

  @override
  String get snippetStarterIntro =>
      'Добавьте готовые команды в хранилище. Затем их можно изменять и удалять; изменения синхронизируются вместе с хранилищем.';

  @override
  String snippetStarterOrigin(String package) {
    return 'Из базового набора «$package».';
  }

  @override
  String snippetAddCount(int count) {
    return 'Добавить команд: $count';
  }

  @override
  String get snippetPackageAdded => 'Добавлен';

  @override
  String get snippetPackageRename => 'Переименовать набор';

  @override
  String get snippetPackageDelete => 'Удалить набор';

  @override
  String snippetPackageDeleteMessage(int count) {
    return 'Удалить все сниппеты в этом наборе ($count)? Удаление также синхронизируется с другими устройствами.';
  }

  @override
  String get snippetMovePackage => 'Переместить в набор…';

  @override
  String get snippetPaste => 'Вставить';

  @override
  String get snippetRunMany => 'Запустить в нескольких терминалах…';

  @override
  String get snippetChooseTerminals => 'Выберите терминалы';

  @override
  String get snippetConnectedOnly =>
      'Показаны подключённые терминалы. Перед запуском вы увидите команду и все выбранные хосты.';

  @override
  String get snippetNoConnected => 'Сначала подключите терминал.';

  @override
  String get snippetTargetsChanged => 'Профиль или выбранные терминалы изменились. Выберите цели ещё раз.';

  @override
  String get snippetUnsafePaste =>
      'Терминал не поддерживает безопасную многострочную вставку. Нажмите «Запустить», чтобы проверить скрипт перед выполнением.';

  @override
  String get snippetSyncHint => 'Сохраняются в хранилище · синхронизируются при включённой синхронизации';

  @override
  String get snippetPackLinux => 'Диагностика Linux';

  @override
  String get snippetPackLinuxDescription => 'Нагрузка, диски, память, порты и журналы служб. Для хостов Linux.';

  @override
  String get snippetPackDocker => 'Docker';

  @override
  String get snippetPackDockerDescription =>
      'Контейнеры, Compose, ресурсы и журналы. На хосте должен быть установлен Docker.';

  @override
  String get snippetPackKubernetes => 'Kubernetes';

  @override
  String get snippetPackKubernetesDescription =>
      'Узлы, поды, сервисы, журналы и события. Нужен настроенный контекст kubectl.';

  @override
  String get snippetCatalogUptime => 'Нагрузка и время работы';

  @override
  String get snippetCatalogDisk => 'Свободное место на дисках';

  @override
  String get snippetCatalogMemory => 'Использование памяти';

  @override
  String get snippetCatalogPorts => 'Прослушиваемые порты';

  @override
  String get snippetCatalogJournal => 'Журнал службы';

  @override
  String get snippetCatalogContainers => 'Все контейнеры';

  @override
  String get snippetCatalogCompose => 'Состояние проекта Compose';

  @override
  String get snippetCatalogStats => 'Ресурсы контейнеров';

  @override
  String get snippetCatalogDockerLogs => 'Журнал контейнера';

  @override
  String get snippetCatalogDockerDisk => 'Занятое Docker место';

  @override
  String get snippetCatalogNodes => 'Узлы кластера';

  @override
  String get snippetCatalogPods => 'Поды в пространстве имён';

  @override
  String get snippetCatalogServices => 'Сервисы в пространстве имён';

  @override
  String get snippetCatalogPodLogs => 'Журнал пода';

  @override
  String get snippetCatalogEvents => 'Последние события кластера';

  @override
  String get glassScrollTabsLeft => 'Прокрутить вкладки влево';

  @override
  String get glassScrollTabsRight => 'Прокрутить вкладки вправо';

  @override
  String inventoryOverview(int hosts, int groups) {
    return 'Хостов: $hosts · Групп: $groups';
  }

  @override
  String get inventoryAllHosts => 'Все хосты';

  @override
  String get inventoryUngrouped => 'Без группы';

  @override
  String get inventoryFolders => 'ГРУППЫ';

  @override
  String get inventoryLocation => 'Расположение';

  @override
  String get inventoryExpand => 'Развернуть группу';

  @override
  String get inventoryCollapse => 'Свернуть группу';

  @override
  String get inventorySearch => 'Поиск хостов, групп, адресов и тегов';

  @override
  String get inventoryCards => 'Карточки';

  @override
  String get inventoryList => 'Список';

  @override
  String get inventoryGroupSettings => 'Настройки группы';

  @override
  String get inventoryEffectiveDefaults => 'Сохранённые настройки и наследование';

  @override
  String get inventoryIncludesSubgroups => 'Хосты этой группы и всех её подгрупп';

  @override
  String get inventoryEmptyHelp => 'Добавьте хосты или группы, чтобы организовать серверы.';

  @override
  String get inventorySearchHelp => 'Измените запрос или сбросьте фильтры.';

  @override
  String get inventoryClearFilters => 'Сбросить фильтры';

  @override
  String get inventoryConnected => 'Есть активное SSH-подключение';

  @override
  String get settingsWorkspacePanel => 'Правая панель';

  @override
  String get settingsWorkspaceFloating => 'Пузыри';

  @override
  String get settingsWorkspaceExpanded => 'Единая панель';

  @override
  String get settingsWorkspaceFloatingHelp =>
      'Отдельные плавающие панели рядом со значками инструментов. Выбор сохраняется на этом устройстве.';

  @override
  String get settingsWorkspaceExpandedHelp =>
      'Правая панель раскрывается целиком: сниппеты и ИИ-чат переключаются внутри. В широком окне она сдвигает рабочую область, в узком — открывается поверх неё. Выбор сохраняется на этом устройстве.';

  @override
  String get workspaceToolsCollapse => 'Свернуть правую панель';

  @override
  String get mobileMore => 'Ещё';

  @override
  String get mobileKeyboard => 'Клавиатура';

  @override
  String get mobileTools => 'Инструменты';

  @override
  String get mobileMessageHint => 'Сообщение…';

  @override
  String get mobileDeviceAuth => 'Отпечаток или PIN';

  @override
  String get mobileDeviceAuthHelp => 'Подтвердите личность отпечатком пальца или кодом блокировки телефона.';

  @override
  String get mobileBackupsHelp =>
      'Зашифрованную резервную копию можно сохранить через «Файлы». Автоматическое резервное копирование по расписанию пока недоступно на Android.';

  @override
  String get sftpDefaultEditor => 'Редактор по умолчанию';

  @override
  String get sftpSystemEditor => 'Системный редактор';

  @override
  String get sftpDefaultEditorHelp =>
      '«Открыть» и «Открыть в редакторе» используют эту программу. Через «Открыть в программе…» можно выбрать другую для отдельного файла. Настройка сохраняется только на этом устройстве.';

  @override
  String get terminalContextMenu => 'Действия терминала';

  @override
  String get terminalPaste => 'Вставить';

  @override
  String get terminalSaveSnippet => 'Сохранить как сниппет…';

  @override
  String get terminalSelectAll => 'Выделить весь буфер';

  @override
  String get terminalClearSelection => 'Снять выделение';

  @override
  String get terminalPasteConfirmTitle => 'Вставить строки или управляющие символы?';

  @override
  String get terminalPasteConfirmMessage =>
      'Этот текст может выполнить команды или прервать работающий процесс. Проверьте его перед вставкой.';

  @override
  String aiTerminalAttachment(String host) {
    return 'Выделенный текст · $host';
  }

  @override
  String get aiRemoveAttachment => 'Убрать выделенный текст';

  @override
  String get screenCaptureTitle => 'Скриншоты и запись экрана';

  @override
  String get screenCaptureAllow => 'Разрешить скриншоты';

  @override
  String get screenCaptureHelp =>
      'Также разрешает запись экрана и превью в списке недавних приложений. Выключите, чтобы скрыть содержимое приложения при захвате экрана.';

  @override
  String get screenCaptureLocalOnly =>
      'Применяется сразу ко всему приложению на этом устройстве, включая экран блокировки. Сохраняется после перезапуска и не синхронизируется.';

  @override
  String get screenCaptureMacosHelp =>
      'Скриншоты разрешены. Современные версии macOS не предоставляют приложению надёжного способа запретить скриншоты и запись экрана.';

  @override
  String get screenCaptureError =>
      'Не удалось прочитать или применить настройку скриншотов. Обновите её состояние и попробуйте ещё раз.';

  @override
  String get updatesTitle => 'Обновления';

  @override
  String get updateAutomatic => 'Проверять обновления при запуске';

  @override
  String get updatePrivacyHelp =>
      'Используются только версия приложения и платформа. Данные хранилища, пароли и токены аккаунта не отправляются.';

  @override
  String get updateCheck => 'Проверить обновления';

  @override
  String updateCurrentVersion(String version) {
    return 'Установлена версия: $version';
  }

  @override
  String get updateChecking => 'Проверяем обновления…';

  @override
  String get updateCurrent => 'У вас последняя версия.';

  @override
  String updateAvailable(String version) {
    return 'Доступна версия $version';
  }

  @override
  String get updateDownloadInstall => 'Скачать и установить';

  @override
  String updateDownloading(int percent) {
    return 'Скачивание: $percent%';
  }

  @override
  String get updateInstalling => 'Проверяем файл и запускаем установщик…';

  @override
  String get updateInstallHelp =>
      'Установщик обновит ConsoleCrypt и может закрыть приложение вместе с SSH-сеансами. Профили и хранилища сохранятся. На Android нужно подтвердить установку в системном окне.';

  @override
  String get updateMacInstallHelp =>
      'После загрузки выберите, куда сохранить проверенный DMG, в системном диалоге macOS. Откроется установщик. Завершите ConsoleCrypt, перетащите приложение в Applications и подтвердите замену. Профили и хранилища сохранятся.';

  @override
  String get updateAndroidPermission =>
      'Разрешите ConsoleCrypt установку обновлений в открывшихся настройках Android, затем снова нажмите «Скачать и установить».';

  @override
  String get updateInstallerOpened => 'Системный установщик открыт. Завершите установку в нём.';

  @override
  String get updateFailed => 'Не удалось выполнить обновление.';

  @override
  String get updateFailedHelp =>
      'Проверьте соединение и попробуйте снова. Файлы с неверной подписью или контрольной суммой не устанавливаются.';

  @override
  String get updateAppleHelp =>
      'На iOS обновления устанавливаются через App Store, TestFlight или Xcode, в зависимости от способа установки приложения.';

  @override
  String get updateLinuxHelp =>
      'На Linux скачайте с официального сайта последнюю версию пакета .deb или .rpm для своего дистрибутива и установите через менеджер пакетов. Для обновления приложения используйте тот же способ.';

  @override
  String get updateLinuxDownloadPage => 'Официальная страница загрузки';

  @override
  String get sharingTitle => 'Совместный доступ';

  @override
  String get sharingSubtitle => 'Выбранные данные для проверенных устройств коллег на этом сервере.';

  @override
  String get sharingUnavailable => 'Совместный доступ недоступен на этом сервере.';

  @override
  String get sharingLocal => 'Для совместного доступа подключите профиль к серверу.';

  @override
  String get sharingReceived => 'Доступно мне';

  @override
  String get sharingOwned => 'Я поделился';

  @override
  String get sharingPublish => 'Поделиться…';

  @override
  String get sharingPublishConfirm => 'Поделиться';

  @override
  String get sharingEmpty => 'Здесь пока нет общих объектов.';

  @override
  String get sharingReceivedHelp =>
      'Объекты, которыми с вами поделились коллеги. Проверьте владельца, прежде чем принять доступ.';

  @override
  String get sharingOwnedHelp => 'Управляйте переданными объектами, правами получателей и отзывом доступа.';

  @override
  String get sharingReceivedEmpty => 'Вам пока ничего не передали';

  @override
  String get sharingReceivedEmptyHelp =>
      'Попросите коллегу на этом сервере поделиться объектом, затем обновите страницу. Перед принятием сравните код устройства владельца.';

  @override
  String get sharingOwnedEmpty => 'Вы пока ничем не поделились';

  @override
  String get sharingOwnedEmptyHelp =>
      'Выберите «Поделиться» в меню нужного объекта. Укажите получателей и проверьте коды их устройств. Передаются только выбранные вами объекты.';

  @override
  String get sharingDevicesTitle => 'Доступ на другом устройстве';

  @override
  String get sharingDevicesHelp =>
      'Добавьте устройство к уже полученному общему доступу. Проверка устройства и разрешения владельца сохраняются.';

  @override
  String get sharingDevicePrepare => 'Добавить это устройство';

  @override
  String get sharingDeviceEndorse => 'Подтвердить устройство';

  @override
  String get sharingDeviceContinue => 'Продолжить добавление';

  @override
  String get sharingDeviceMore => 'Другие действия';

  @override
  String get sharingDeviceMoreHelp =>
      'Продолжите с уже подтверждённым пакетом устройства или проверьте подписанную историю для восстановления локального запроса. Сами по себе эти действия не предоставляют доступ.';

  @override
  String get sharingInvitation => 'Приглашение — проверьте владельца';

  @override
  String get sharingAccept => 'Проверить и принять';

  @override
  String get sharingRefresh => 'Обновить общие данные';

  @override
  String get sharingIdentity => 'Код моего устройства';

  @override
  String get sharingVerifyHelp =>
      'Сравните этот код с кодом коллеги по независимому каналу. Вход в аккаунт и электронная почта не подтверждают ключи устройства.';

  @override
  String get sharingCode => 'Код проверки';

  @override
  String get sharingConfirmed => 'Код совпадает с кодом на устройстве коллеги';

  @override
  String get sharingEmail => 'Электронная почта коллеги';

  @override
  String get sharingFind => 'Найти устройства';

  @override
  String get sharingSelectDevice => 'Выберите и проверьте устройства получателя';

  @override
  String get sharingReader => 'Чтение';

  @override
  String get sharingEditor => 'Редактирование';

  @override
  String get sharingReaderHelp => 'Может читать и сохранять копии; не может менять общий объект.';

  @override
  String get sharingEditorHelp => 'Может читать и менять содержимое; доступом управляет владелец.';

  @override
  String get sharingPreview => 'Что будет передано';

  @override
  String get sharingExclude =>
      'Пароли, SSH-ключи, личные ссылки, история терминала и ключи ИИ не включаются. Проверьте команды и свободный текст перед отправкой.';

  @override
  String get sharingIncludeNotes => 'Включить заметки хоста';

  @override
  String get sharingSecretWarning =>
      'Получатель сможет прочитать и сохранить секрет. Отзыв доступа не удалит уже прочитанные копии. Для SSH-доступа также нужна смена учётных данных на сервере.';

  @override
  String get sharingSecretConfirmed => 'Я понимаю и разрешаю передать этот секрет выбранным устройствам';

  @override
  String get sharingHost => 'Хост';

  @override
  String get sharingSnippet => 'Сниппет';

  @override
  String get sharingGroup => 'Общая коллекция';

  @override
  String get sharingSecret => 'Секрет';

  @override
  String get sharingDevice => 'Устройство';

  @override
  String get sharingOwner => 'Владелец';

  @override
  String get sharingMembers => 'Участники';

  @override
  String get sharingVerified => 'Проверено';

  @override
  String get sharingBlocked => 'Требуется проверка';

  @override
  String get sharingDeleted => 'Удалено';

  @override
  String get sharingRevision => 'Версия';

  @override
  String get sharingQueue => 'Очередь отправки';

  @override
  String get sharingFlush => 'Отправить ожидающие изменения';

  @override
  String get sharingPending => 'Ожидает отправки';

  @override
  String get sharingConflictHelp =>
      'Права или содержимое изменились. Обновите объект и проверьте изменения перед повторной отправкой.';

  @override
  String get sharingRevoke => 'Отозвать доступ';

  @override
  String get sharingRevokeHelp =>
      'Будущие изменения будут зашифрованы новым ключом. Прочитанные копии останутся у получателя.';

  @override
  String get sharingStop => 'Удалить общий объект';

  @override
  String get sharingStopHelp =>
      'Общий объект будет удалён для участников. Личные копии и уже прочитанные данные сохранятся.';

  @override
  String get sharingCopyPersonal => 'Сохранить личную копию';

  @override
  String get sharingCopyHelp =>
      'Копия сохранится в вашем личном хранилище. Учётные данные задаются отдельно и не передаются коллегам.';

  @override
  String get sharingSaved => 'Готово';

  @override
  String get sharingEdit => 'Изменить общий объект';

  @override
  String get sharingChangeSaved => 'Изменение сохранено или поставлено в очередь отправки.';

  @override
  String get sharingName => 'Название';

  @override
  String get sharingAddress => 'Адрес';

  @override
  String get sharingPort => 'Порт';

  @override
  String get sharingUsername => 'Имя пользователя';

  @override
  String get sharingNotes => 'Заметки';

  @override
  String get sharingDescription => 'Описание';

  @override
  String get sharingCommand => 'Команда';

  @override
  String get sharingNoDevices => 'Доступных устройств не найдено. Проверьте адрес и подтверждение почты коллеги.';

  @override
  String get sharingNoAutoRun => 'Полученные команды никогда не выполняются автоматически.';

  @override
  String get sharingOwnerDeviceOnly =>
      'Управление доступом доступно на устройстве владельца, которое создало этот объект.';

  @override
  String get sharingWorking => 'Подготовка защищённого обмена…';

  @override
  String get sharingCopyCode => 'Копировать код';

  @override
  String get sharingAddRecipient => 'Добавить получателя';

  @override
  String get sharingRefreshCopy => 'Обновить личную копию';

  @override
  String get sharingManageAccess => 'Управление доступом';

  @override
  String get sharingReviewQueue => 'Посмотреть очередь';

  @override
  String get sharingQueueHelp =>
      'После изменения прав или содержимого отправка приостанавливается. Проверьте актуальный элемент; ненужный запрос можно удалить и создать новое изменение.';

  @override
  String get sharingDiscard => 'Убрать из очереди';

  @override
  String get sharingDiscardHelp =>
      'Удалить этот запрос с устройства? Уже принятые сервером изменения остаются. Удаление запроса не отменяет опубликованный доступ.';

  @override
  String get sharingReconcile => 'Проверить историю';

  @override
  String get sharingReconcileHelp =>
      'Локальная копия отстаёт от сохранённой защищённой отметки. Сверьте код владельца. Приложение проверит полную подписанную историю; пропуски или другая ветка будут отклонены.';

  @override
  String get sharingEndpointChanged =>
      'Адрес или порт общего хоста изменился. Проверьте новые значения и подтвердите обновление перед подключением со своими учётными данными.';

  @override
  String get sharingTunnelVerify => 'Для общего хоста сначала проверьте доступ и запустите туннель вручную.';

  @override
  String get sharingPromptPassword => 'Спрашивать пароль при подключении';

  @override
  String get sharingRefreshHost => 'Обновить общий хост';

  @override
  String get sharingRefreshHostHelp =>
      'Проверьте адрес и порт в разделе «Общий доступ». Подтвердить их обновление в личной копии? Остальные настройки подключения останутся вашими.';

  @override
  String get sharingDetachHost => 'Сделать независимой копией';

  @override
  String get sharingDetachHostHelp =>
      'Эта копия больше не будет проверять изменения и доступ к общему хосту. У коллег останутся их права и копии.';

  @override
  String get sharingCollectionHelp =>
      'Выберите уже опубликованные элементы. Группа хранит только ссылки и не даёт доступ к их содержимому.';

  @override
  String get sharingCollectionEmpty => 'Сначала поделитесь нужными элементами отдельно.';

  @override
  String get sharingCollectionUnavailable => 'Элемент недоступен: нужен отдельный доступ.';

  @override
  String get sharingCollectionChildren => 'Элементы группы';

  @override
  String get sharingSecretSelection => 'Передаваемый секрет';

  @override
  String get sharingSecretPrimary => 'Пароль или приватный ключ';

  @override
  String get sharingSecretPassphrase => 'Парольная фраза ключа (отдельно)';

  @override
  String get enrollmentTitle => 'Новые собственные устройства';

  @override
  String get enrollmentHelp =>
      'Разрешение относится к одному общему элементу. Уже проверенное устройство коллеги подтверждает его новое устройство; владелец остаётся онлайн для выдачи доступа. Данные личного хранилища не передаются.';

  @override
  String get enrollmentCreate => 'Разрешить новые устройства';

  @override
  String get enrollmentAnchor => 'Устройство для подтверждения';

  @override
  String get enrollmentExpiry => 'Срок разрешения';

  @override
  String get enrollmentQuota => 'Количество новых устройств';

  @override
  String get enrollmentOneDay => '1 день';

  @override
  String get enrollmentSevenDays => '7 дней';

  @override
  String get enrollmentThirtyDays => '30 дней';

  @override
  String get enrollmentAutomatic => 'Принимать подтверждённые устройства автоматически';

  @override
  String get enrollmentAutomaticHelp =>
      'Только после сверки кода на уже проверенном устройстве, пока владелец онлайн. Новое устройство не сможет превысить выбранные права.';

  @override
  String get enrollmentDisable => 'Отключить разрешение';

  @override
  String get enrollmentDisableHelp =>
      'Новые устройства больше не получат доступ по этому разрешению. Уже добавленные устройства удаляются отдельно в управлении доступом. Отключение сохранится на этом устройстве даже без связи с сервером.';

  @override
  String get enrollmentExport => 'Пакет для нового устройства';

  @override
  String get enrollmentRequests => 'Запросы устройств';

  @override
  String get enrollmentPrepare => 'Я на новом устройстве';

  @override
  String get enrollmentEndorse => 'Подтвердить другое устройство';

  @override
  String get enrollmentSubmit => 'Продолжить добавление устройства';

  @override
  String get enrollmentPackage => 'Пакет устройства';

  @override
  String get enrollmentPackageHelp =>
      'Передайте этот публичный пакет между своими устройствами. Он содержит ключи и подписанные подтверждения, без паролей и содержимого общих элементов. Сверка кода остаётся обязательной.';

  @override
  String get enrollmentPreview => 'Проверить пакет';

  @override
  String get enrollmentCodeHelp =>
      'Сравните весь код на новом и уже проверенном устройстве лично или по независимому каналу. Подтвердите только полное совпадение. Код относится именно к этому запросу.';

  @override
  String get enrollmentCopyPackage => 'Скопировать пакет';

  @override
  String get enrollmentSavePackage => 'Сохранить пакет в файл';

  @override
  String get enrollmentOpenPackage => 'Открыть пакет из файла';

  @override
  String get enrollmentPending => 'Мои запросы устройств';

  @override
  String get enrollmentWait =>
      'Запрос отправлен. Держите это устройство и устройство владельца онлайн. Продолжите проверку, когда владелец отправит запрос подтверждения.';

  @override
  String get enrollmentChallenge => 'Проверить ключи устройства';

  @override
  String get enrollmentRespond => 'Продолжить проверку';

  @override
  String get enrollmentAccept => 'Предоставить доступ';

  @override
  String get enrollmentManualConfirmed => 'Подтверждаю добавление этого устройства с указанными правами';

  @override
  String get enrollmentActive => 'Действует';

  @override
  String get enrollmentDisabled => 'Отключено';

  @override
  String get enrollmentRequested => 'Ожидает проверки';

  @override
  String get enrollmentChallenged => 'Ожидает ответа устройства';

  @override
  String get enrollmentResponded => 'Ключи подтверждены';

  @override
  String get enrollmentAccepted => 'Доступ предоставлен';

  @override
  String get enrollmentExpired => 'Срок истёк';

  @override
  String get enrollmentBlocked => 'Нужна проверка актуальных прав и истории';

  @override
  String get enrollmentCreateConfirmed =>
      'Разрешаю добавлять собственные устройства этого пользователя в выбранных пределах';

  @override
  String glassOpenTerminalTabs(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Открыто $count терминала',
      many: 'Открыто $count терминалов',
      few: 'Открыто $count терминала',
      one: 'Открыт $count терминал',
    );
    return '$_temp0';
  }

  @override
  String get updateMacInstallerOpened =>
      'DMG сохранён и открыт. Завершите ConsoleCrypt, перенесите приложение в Applications с заменой и запустите его снова.';

  @override
  String get updateMacSaveTitle => 'Сохранить обновление ConsoleCrypt';

  @override
  String get updateMacSavePrompt => 'Сохранить и открыть';

  @override
  String get updateMacDestinationExists =>
      'Этот файл уже существует. Повторите установку и выберите другое имя или папку для нового DMG.';

  @override
  String get rdpTitle => 'Удалённый рабочий стол';

  @override
  String get rdpNewConnection => 'Новое RDP-подключение';

  @override
  String get rdpEmptyTitle => 'Рабочий стол во вкладке';

  @override
  String get rdpEmptyHelp => 'Укажите сервер Windows и учётную запись. Сначала проверьте сертификат сервера.';

  @override
  String get rdpLocked => 'Разблокируйте хранилище, чтобы открыть RDP.';

  @override
  String get rdpConnecting => 'Подключение…';

  @override
  String get rdpConnected => 'Подключено';

  @override
  String get rdpDisconnected => 'Соединение закрыто. Для нового входа создайте подключение.';

  @override
  String get rdpConnectionFailed => 'Не удалось подключиться. Проверьте адрес, учётную запись и сертификат.';

  @override
  String get rdpSecureAttention => 'Ctrl + Alt + Delete';

  @override
  String get rdpResize => 'Экран 1920 × 1080';

  @override
  String get rdpExpand => 'Во весь экран';

  @override
  String get rdpCollapse => 'Выйти из полного экрана';

  @override
  String get rdpAddress => 'Адрес сервера';

  @override
  String get rdpPort => 'Порт';

  @override
  String get rdpUsername => 'Пользователь';

  @override
  String get rdpDomain => 'Домен (необязательно)';

  @override
  String get rdpPassword => 'Пароль Windows';

  @override
  String get rdpCredentialsHelp =>
      'Пароль используется только для этого входа и не сохраняется. До подтверждения сертификата он не отправляется серверу.';

  @override
  String get rdpInvalidForm => 'Введите адрес, порт от 1 до 65535, пользователя и пароль.';

  @override
  String get rdpCertificateTitle => 'Проверьте сертификат RDP';

  @override
  String get rdpCertificateHelp =>
      'Сравните весь отпечаток с данными администратора по доверенному каналу. Подтверждение действует только для этого подключения. Если отпечаток не совпадает, отмените вход.';

  @override
  String get rdpFingerprint => 'SHA-256 сертификата';

  @override
  String get rdpCertificateConfirm => 'Я сверил весь отпечаток сертификата';

  @override
  String get rdpCertificateChanged =>
      'Сертификат сервера изменился. Вход заблокирован; создайте новое подключение и сверьте отпечаток.';

  @override
  String get rdpAuthenticationFailed => 'Windows отклонила вход. Проверьте пользователя, домен и пароль.';

  @override
  String get rdpResizeUnavailable => 'Сервер не поддерживает этот размер экрана. Продолжайте с исходным размером.';

  @override
  String get rdpWorkspaceHelp =>
      'Рабочий стол во вкладке. Буфер обмена и выбранная папка доступны только с вашего разрешения.';

  @override
  String get rdpPermissions => 'Разрешения сеанса';

  @override
  String get rdpAllowClipboard => 'Разрешить текстовый буфер обмена';

  @override
  String get rdpClipboardHelp =>
      'Ctrl+V (⌘V на Mac) передаёт и вставляет текст. Кнопки отправки и получения — отдельный обмен. По умолчанию доступ выключен.';

  @override
  String get rdpFolderHelp =>
      'Предоставьте удалённому компьютеру доступ только к выбранной папке. По умолчанию — только чтение.';

  @override
  String get rdpSelectFolder => 'Выбрать папку…';

  @override
  String get rdpStopFolder => 'Отключить папку';

  @override
  String get rdpNoFolder => 'Папка не предоставлена';

  @override
  String get rdpAllowFolderWrite => 'Разрешить запись в папку';

  @override
  String get rdpFolderWriteHelp => 'Удалённый компьютер сможет создавать, изменять и удалять файлы в этой папке.';

  @override
  String get rdpFolderUnsupported => 'Передача папки недоступна на этом устройстве.';

  @override
  String get rdpPermissionsFailed => 'Не удалось применить разрешения. Попробуйте ещё раз.';

  @override
  String get rdpClipboardReceived => 'Текст получен. Буфер обмена очистится по настройкам безопасности.';

  @override
  String get rdpClipboardSent => 'Текст отправлен на удалённый компьютер.';

  @override
  String get rdpClipboardFailed => 'Не удалось передать текст буфера обмена.';

  @override
  String get rdpSendClipboard => 'Отправить текст буфера обмена';

  @override
  String get rdpReceiveClipboard => 'Получить текст буфера обмена';

  @override
  String get rdpSavedPasswordHelp => 'Пароль будет взят из зашифрованной учётной записи этого хоста.';

  @override
  String get hostProtocolLabel => 'Тип подключения';

  @override
  String get hostRdpConnectionHelp =>
      'RDP подключается напрямую. Порт и учётная запись задаются для этого хоста; настройки SSH-группы не применяются.';

  @override
  String get hostRdpPasswordOnly => 'Для RDP выберите парольную учётную запись.';

  @override
  String get hostRdpUsernameRequired => 'Введите имя пользователя Windows';

  @override
  String get rdpFolderPending => 'Windows подключает папку…';

  @override
  String get rdpFolderReady => 'Папка подключена. Откройте её в Проводнике Windows.';

  @override
  String get rdpFolderDenied =>
      'Windows отклонила подключение папки. Попросите администратора разрешить перенаправление дисков; удалённый рабочий стол продолжает работать.';

  @override
  String get rdpFolderUnavailable =>
      'Windows не подключила папку. Проверьте, разрешено ли перенаправление дисков на сервере, затем подключитесь заново.';

  @override
  String get rdpFolderChooseFailed =>
      'Не удалось открыть выбранную папку. Проверьте доступ к ней и выберите папку снова.';

  @override
  String get rdpFolderChooseLimit =>
      'Не удалось выбрать ещё одну папку. Отмените лишний выбор или закройте ненужные сеансы и попробуйте снова.';

  @override
  String get rdpFolderOpenHelp =>
      'После подключения откройте «Этот компьютер» в Проводнике Windows или вставьте этот путь в адресную строку:';

  @override
  String get rdpFolderWindowsPath => '\\\\tsclient\\ConsoleCrypt';

  @override
  String get rdpFolderLimits =>
      'До 256 МиБ на файл и до 512 МиБ записи за сеанс. Ссылки и специальные файлы недоступны. Изменение папки прерывает открытые в ней файлы.';

  @override
  String get rdpClipboardUnavailable =>
      'Текстовый буфер обмена ещё не готов или запрещён сервером. Подождите и попробуйте снова; если ошибка повторится, проверьте настройки перенаправления с администратором.';

  @override
  String get rdpClipboardLimit => 'Можно передать только текст размером до 64 КиБ.';

  @override
  String get rdpClipboardBusy => 'Предыдущий запрос буфера обмена ещё обрабатывается. Подождите и попробуйте снова.';

  @override
  String get rdpClipboardEmpty => 'В буфере обмена нет доступного текста.';

  @override
  String get rdpPinBar => 'Закрепить панель подключения';

  @override
  String get rdpUnpinBar => 'Скрывать панель автоматически';

  @override
  String get rdpConnections => 'Переключить подключение';

  @override
  String get rdpMinimize => 'Свернуть окно';

  @override
  String get rdpDisconnect => 'Закрыть этот сеанс';

  @override
  String get rdpShowBar => 'Показать панель подключения (Ctrl+Alt+Home)';

  @override
  String get rdpFullscreenFailed => 'Не удалось изменить полноэкранный режим. Используйте кнопки управления окном.';

  @override
  String get rdpMobileTouchpad => 'Тачпад';

  @override
  String get rdpMobileDirectTouch => 'Касание';

  @override
  String get rdpMobileShowKeyboard => 'Открыть клавиатуру';

  @override
  String get rdpMobileHideKeyboard => 'Скрыть клавиатуру';

  @override
  String get rdpMobileLeftClick => 'Левый клик в позиции курсора';

  @override
  String get rdpMobileRightClick => 'Правый клик в позиции курсора';

  @override
  String get rdpMobileHelpTitle => 'Управление с телефона';

  @override
  String get rdpMobileHelp =>
      'Рабочий стол — это тачпад. Водите одним пальцем для движения курсора; палец можно поднимать и переставлять.\n\nКасание — левый клик, два быстрых касания — двойной. Удерживайте второе касание и двигайте палец для перетаскивания. Можно также удержать один палец около половины секунды, затем двигать.\n\nДва пальца: касание — правый клик, параллельное движение — прокрутка, щипок — масштаб от 1× до 4×. Двойное касание двумя пальцами вписывает картинку в экран. При приближении картинка следует за курсором.\n\nКасание тремя пальцами показывает или скрывает клавиатуру. Она перекрывает картинку, а не сжимает её. В панели «Клавиши» одно касание Ctrl, Alt, Win или Shift действует на следующую клавишу, двойное фиксирует, ещё одно отпускает.\n\nКруглая кнопка открывает меню. Перетащите её к краю экрана. Здесь можно переключить тачпад на прямое касание, настроить размер, сменить сеанс или свернуть его. Подключённая мышь работает напрямую.';

  @override
  String get localAiTitle => 'Встроенный локальный ИИ';

  @override
  String get localAiDescription =>
      'Выберите модель для этого компьютера. После установки чат и подсказки команд работают без интернета.';

  @override
  String get localAiModel => 'Модель';

  @override
  String localAiDownloadSize(String size) {
    return 'Загрузка модели: $size';
  }

  @override
  String get localAiNotInstalled => 'Не установлено';

  @override
  String get localAiInstalling => 'Загрузка и проверка файлов…';

  @override
  String get localAiStopped => 'Установлено · запустится при запросе';

  @override
  String get localAiStarting => 'Запуск модели…';

  @override
  String get localAiReady => 'Готово · работает на этом компьютере';

  @override
  String get localAiStopping => 'Остановка…';

  @override
  String get localAiError => 'Ошибка локального ИИ';

  @override
  String get localAiUnsupported => 'Доступно на поддерживаемых компьютерах с Windows и macOS.';

  @override
  String get localAiDefault => 'Использовать по умолчанию после установки';

  @override
  String get localAiDeviceOnly =>
      'Только на этом компьютере. Настройки других устройств и сохранённых провайдеров независимы.';

  @override
  String get localAiInstall => 'Установить локальный ИИ';

  @override
  String get localAiStart => 'Запустить';

  @override
  String get localAiStop => 'Остановить';

  @override
  String get localAiRemove => 'Удалить выбранную модель';

  @override
  String get localAiResourceHelp =>
      'Во время работы использует память и процессор; остановится через 15 минут без запросов. Небольшие модели могут ошибаться. Проверяйте команды перед запуском.';

  @override
  String get localAiOperationFailed =>
      'Не удалось выполнить действие с локальным ИИ. Проверьте состояние и повторите попытку.';

  @override
  String get localAiInstallFailed =>
      'Не удалось установить модель. Проверьте интернет и свободное место на диске, затем повторите попытку.';

  @override
  String get localAiFilesChanged => 'Не удалось проверить установленные файлы. Удалите модель и установите её заново.';

  @override
  String get localAiStartFailed =>
      'Модель не запустилась или неожиданно остановилась. Освободите оперативную память и повторите попытку.';

  @override
  String get localAiStorageUnavailable =>
      'Нет доступа к файлам локального ИИ. Проверьте права на папку данных приложения и свободное место на диске.';

  @override
  String localAiRamRecommendation(int ram) {
    return 'Рекомендуется от $ram ГБ оперативной памяти. Скорость зависит от компьютера.';
  }

  @override
  String get localAiModelsPreserved =>
      'При смене модели остальные загрузки сохраняются. Обновление приложения сохраняет модели и ваш выбор.';

  @override
  String get localAiModelInstalled => 'Установлена';

  @override
  String get localAiRuntimeMissing => 'Движок ИИ отсутствует. Восстановите его; скачанные модели сохранятся.';

  @override
  String get localAiBundledRuntimeMissing =>
      'Движок ИИ в приложении недоступен. Переустановите приложение; скачанные модели сохранятся.';

  @override
  String get localAiRepairRuntime => 'Восстановить движок';

  @override
  String get localAiInstallSelectedHelp =>
      'Установите выбранную модель для локального ИИ. Выбор локального ИИ по умолчанию сохранён; запросы не будут переключаться на облако.';

  @override
  String get localAiTierLight => 'Лёгкая';

  @override
  String get localAiTierMedium => 'Средняя';

  @override
  String get localAiTierStrong => 'Тяжёлая';

  @override
  String localAiModelRequirements(String size, int ram) {
    return '$size для загрузки · рекомендуется $ram ГБ ОЗУ';
  }

  @override
  String get localAiRecommended => 'Рекомендуется по объёму памяти этого компьютера';

  @override
  String localAiDetectedMemory(String size) {
    return 'На компьютере: $size оперативной памяти. Звезда — рекомендация; ваш выбор не меняется.';
  }

  @override
  String get localAiMetalHelp =>
      'На Apple Silicon используется ускорение Metal. Большим моделям нужно больше памяти; ответы могут быть медленнее.';

  @override
  String get localAiCpuHelp =>
      'На Windows и Intel Mac используется процессор. Объём памяти не гарантирует быстрые ответы; большие модели могут работать медленно.';

  @override
  String localAiModelAttribution(String publisher, String license) {
    return 'Модель / GGUF: $publisher · Лицензия: $license';
  }

  @override
  String get localAiModelNotices => 'Источники и лицензии моделей';

  @override
  String get terminalLocalSearch => 'Поиск оболочек или SSH-хостов';

  @override
  String get terminalLocalSection => 'Локальные оболочки';

  @override
  String get terminalLocalSshSection => 'SSH-хосты';

  @override
  String get terminalLocalComputer => 'Этот компьютер';

  @override
  String get terminalLocalUnavailable => 'Локальные оболочки недоступны.';

  @override
  String get terminalLocalSandboxUnavailable => 'В этой сборке с песочницей локальные оболочки недоступны.';

  @override
  String get terminalLocalRefresh => 'Обновить список оболочек';

  @override
  String get terminalLocalEmptyMessage => 'Откройте локальную оболочку или подключитесь к SSH-хосту в новой вкладке.';

  @override
  String get terminalLocalOpen => 'Открыть терминал';

  @override
  String get terminalLocalRestart => 'Перезапустить оболочку';

  @override
  String get terminalLocalInsertOnly => 'Вставьте команду, проверьте её и нажмите Enter в локальном терминале.';

  @override
  String get rdpMobileMenu => 'Управление сеансом';

  @override
  String get rdpMobileKeys => 'Клавиши';

  @override
  String get rdpMobileFit => 'Вписать';

  @override
  String get rdpMobileFitResolution => 'Подогнать под экран';

  @override
  String get rdpMobileOrientation => 'Поворот';

  @override
  String get rdpMobileAutomatic => 'Авто';

  @override
  String get rdpMobileLandscape => 'Альбомная';

  @override
  String get rdpMobilePortrait => 'Портретная';

  @override
  String get rdpMobileMinimize => 'Свернуть сеанс';

  @override
  String get rdpMobileBackAgain => 'Нажмите «Назад» ещё раз, чтобы свернуть сеанс';

  @override
  String get rdpMobileDisconnectConfirm => 'Отключиться от удалённого рабочего стола?';

  @override
  String get rdpMobileClipboard => 'Буфер обмена';

  @override
  String get rdpMobileIntroMove =>
      'Водите пальцем по экрану — это тачпад. Курсор остаётся на месте, когда вы переставляете палец.';

  @override
  String get rdpMobileIntroZoom => 'Два пальца: касание — правый клик, движение — прокрутка, щипок — масштаб.';

  @override
  String get rdpMobileIntroMenu =>
      'Три пальца — клавиатура. Круглая плавающая кнопка открывает меню; её можно перетащить к краю.';

  @override
  String get rdpMobileDontShow => 'Не показывать';

  @override
  String get rdpMobileSettingsTitle => 'RDP · Управление на телефоне';

  @override
  String get rdpMobileSettingsLocal =>
      'Настройки сохраняются только на этом устройстве. Последний режим управления запоминается отдельно для каждого хоста.';

  @override
  String get rdpMobileDefaultMode => 'Режим по умолчанию';

  @override
  String get rdpMobileAutoDefault => 'Тачпад на телефоне, касание на планшете';

  @override
  String get rdpMobileSpeed => 'Скорость указателя';

  @override
  String get rdpMobileAcceleration => 'Ускорение указателя';

  @override
  String get rdpMobileCursorSize => 'Размер курсора';

  @override
  String get rdpMobileAutoResize => 'Подгонять разрешение под экран';

  @override
  String get rdpMobileClarity => 'Чёткость';

  @override
  String get rdpMobileWindowsScale => 'Масштаб Windows';

  @override
  String get rdpMobileAutoHide => 'Автоскрытие кнопки';

  @override
  String get paletteNewLocalTerminal => 'Новый локальный терминал';

  @override
  String get hostImportTitle => 'Импорт хостов из системы';

  @override
  String get hostImportHelp =>
      'Выберите адреса из known_hosts. Логины и пароли в этом файле не хранятся — их можно задать после импорта. Ключи доверия не изменяются; при первом подключении подтвердите ключ сервера.';

  @override
  String get hostImportReadFailed =>
      'Не удалось прочитать файл known_hosts. Проверьте доступ и размер файла (до 4 МиБ).';

  @override
  String get hostImportSaveFailed =>
      'Не удалось сохранить все хосты. Уже добавленные хосты отмечены в списке; можно повторить для оставшихся.';

  @override
  String hostImportSkipped(int count) {
    return 'Пропущено записей: $count. Хешированные адреса, шаблоны, записи CA, отозванные и повреждённые ключи не создают хостов.';
  }

  @override
  String hostImportSaved(int count) {
    return 'Добавлено хостов: $count.';
  }

  @override
  String get hostImportSelectAll => 'Выбрать все новые хосты';

  @override
  String get hostImportEmpty =>
      'Адресов для импорта не найдено. В файле могут быть только хешированные имена; их невозможно восстановить.';

  @override
  String get hostImportExists => 'Уже есть в этом профиле';

  @override
  String get commonImport => 'Импорт';

  @override
  String get kubeTitle => 'Kubernetes';

  @override
  String get kubeSubtitle => 'Подключения к кластерам в зашифрованном хранилище.';

  @override
  String get kubeImport => 'Импорт kubeconfig';

  @override
  String get kubeImportHelp =>
      'Выберите kubeconfig или вставьте YAML, затем отметьте контексты. Импорт не подключается к кластеру и не запускает команды аутентификации.';

  @override
  String get kubeChooseFile => 'Выбрать файл';

  @override
  String get kubeYaml => 'YAML kubeconfig';

  @override
  String get kubePreview => 'Проверить контексты';

  @override
  String get kubeSaveContexts => 'Сохранить выбранные контексты';

  @override
  String get kubeChooseAgain => 'Выбрать другую конфигурацию';

  @override
  String get kubeEmpty => 'Подключений Kubernetes пока нет. Импортируйте kubeconfig, чтобы добавить контексты.';

  @override
  String get kubeNamespace => 'Namespace';

  @override
  String get kubeAuthentication => 'Аутентификация';

  @override
  String get kubeProduction => 'Production: защита от изменений';

  @override
  String get kubeApproveFiles => 'Прочитать указанные файлы для выбранных контекстов';

  @override
  String get kubeExecHelp =>
      'Этот контекст использует внешнюю команду аутентификации. Импорт не даёт разрешения на её запуск.';

  @override
  String get kubeImportInvalid =>
      'Этот контекст нельзя импортировать. Проверьте настройки кластера, пользователя и аутентификации в исходном файле.';

  @override
  String get kubeImportFailed =>
      'Не удалось импортировать конфигурацию. Проверьте выбранные контексты и файлы, затем повторите проверку.';

  @override
  String get kubeFileFailed => 'Выберите обычный файл UTF-8 размером не более 4 МиБ.';

  @override
  String get kubeExpired => 'Время предварительного просмотра истекло. Проверьте конфигурацию снова.';

  @override
  String get kubeTlsRequired =>
      'Для Production нужна проверка TLS-сертификата. Исправьте kubeconfig или отметьте контекст как непродуктивный.';

  @override
  String get kubeRoute => 'Маршрут подключения';

  @override
  String get kubeRouteDirect => 'Прямое подключение';

  @override
  String get kubeRouteRequired =>
      'В kubeconfig указан прокси. Выберите сохранённый SSH-маршрут с доступом к API-серверу.';

  @override
  String get kubeSaved => 'Выбранные контексты сохранены.';

  @override
  String get kubeDeleteHelp =>
      'Удалить сохранённый контекст из хранилища? Это не изменит ничего в кластере Kubernetes.';

  @override
  String get kubeLoadFailed => 'Не удалось загрузить подключения Kubernetes.';

  @override
  String get kubeRefresh => 'Обновить подключения';

  @override
  String get kubeContextLimit => 'Выберите не более 64 контекстов за один импорт.';

  @override
  String get kubeAuthToken => 'Токен';

  @override
  String get kubeAuthCertificate => 'Клиентский сертификат';

  @override
  String get kubeAuthExec => 'Внешняя команда';

  @override
  String get kubeAuthNone => 'Не настроена';

  @override
  String get kubeConnect => 'Подключиться';

  @override
  String get kubeDisconnect => 'Отключиться';

  @override
  String get kubeConnecting => 'Подключение к кластеру…';

  @override
  String get kubeConnectionFailed => 'Не удалось подключиться. Проверьте сеть, авторизацию и маршрут подключения.';

  @override
  String get kubeRetry => 'Повторить';

  @override
  String get kubeBackToContexts => 'Сохранённые подключения';

  @override
  String get kubeReadOnly => 'Прод · только чтение';

  @override
  String get kubeReadOnlyHelp => 'Этот контекст защищён от изменений на уровне нативного клиента Kubernetes.';

  @override
  String get kubeResources => 'Ресурсы';

  @override
  String get kubeAllNamespaces => 'Все пространства имён';

  @override
  String get kubeResourceKind => 'Тип ресурса';

  @override
  String get kubeLabelSelector => 'Фильтр по меткам';

  @override
  String get kubeApplyFilter => 'Применить фильтр';

  @override
  String get kubeNoResources => 'Ресурсов с такими фильтрами нет.';

  @override
  String get kubeNoResourceKinds =>
      'Доступные типы ресурсов не обнаружены. Проверьте права этой учётной записи в Kubernetes.';

  @override
  String get kubeRequestFailed => 'Не удалось загрузить данные кластера. Проверьте соединение и права в Kubernetes.';

  @override
  String get kubeNamespacesFailed =>
      'Список пространств имён недоступен. Ресурсы выбранного пространства по-прежнему можно просматривать.';

  @override
  String get kubeLoadMore => 'Загрузить ещё';

  @override
  String get kubeRefreshResources => 'Обновить ресурсы';

  @override
  String get kubeLive => 'Обновления в реальном времени';

  @override
  String get kubeSnapshot => 'Снимок состояния';

  @override
  String get kubeWatchFailed => 'Обновления остановлены. Нажмите «Обновить», чтобы подключиться снова.';

  @override
  String get kubeResourceLimit =>
      'В памяти можно держать до 10 000 ресурсов. Уточните пространство имён или фильтр по меткам.';

  @override
  String get kubeOverview => 'Обзор';

  @override
  String get kubeObjectYaml => 'YAML';

  @override
  String get kubeLogs => 'Логи';

  @override
  String get kubeContainer => 'Контейнер';

  @override
  String get kubePreviousLogs => 'Предыдущий контейнер';

  @override
  String get kubeLogFilter => 'Фильтр строк журнала';

  @override
  String get kubeNoLogs => 'Нет строк журнала для отображения.';

  @override
  String get kubeLogTailHelp => 'Последние 500 строк загружаются в память. Обновите, чтобы получить свежие записи.';

  @override
  String get kubeSecretRedacted => 'Значения секрета скрыты. Здесь показаны только метаданные и имена ключей.';

  @override
  String get kubeStatus => 'Состояние';

  @override
  String get kubeCreatedAt => 'Создан';

  @override
  String get kubeLabels => 'Метки';

  @override
  String get kubeConditions => 'Условия';

  @override
  String get kubeOwners => 'Владельцы';

  @override
  String get kubeName => 'Имя';

  @override
  String get kubeNotReported => 'Нет данных';

  @override
  String get kubeSessionEnded => 'Соединение завершено. Подключитесь снова, чтобы загрузить данные кластера.';

  @override
  String kubeLoadedCount(int count) {
    return 'Загружено ресурсов: $count';
  }

  @override
  String get kubeShowDetails => 'Открыть сведения о ресурсе';

  @override
  String kubeClusterVersion(String version) {
    return 'Kubernetes $version';
  }

  @override
  String get kubeSortName => 'По имени';

  @override
  String get kubeSortNamespace => 'По пространству имён';

  @override
  String get kubeSortStatus => 'По состоянию';

  @override
  String get kubeSortAge => 'Сначала новые';

  @override
  String get kubeRefreshDetails => 'Обновить сведения';

  @override
  String get kubeAllowChanges => 'Разрешить изменения на 10 минут';

  @override
  String get kubeAllowChangesHelp =>
      'В этом прод-контексте откроется временное окно записи. Изменения будут записаны в локальный журнал аудита. Для продолжения введите пароль хранилища.';

  @override
  String get kubeCloseWriteWindow => 'Вернуть режим только для чтения';

  @override
  String kubeWriteWindowRemaining(String time) {
    return 'ПРОД · изменения разрешены · осталось $time';
  }

  @override
  String get kubeWriteWindowFailed => 'Не удалось изменить окно записи. Подключитесь заново и повторите попытку.';

  @override
  String get kubeEditYaml => 'Изменить YAML';

  @override
  String get kubeApplyYaml => 'Применить изменения';

  @override
  String get kubeApplyYamlHelp =>
      'Применить проверенные изменения к ресурсу? Для подтверждения введите пароль хранилища. Сервер отклонит обновление, если версия ресурса изменилась.';

  @override
  String get kubeApplyFailed =>
      'Изменение не принято. Обновите объект и проверьте права, корректность YAML и конфликты владения полями перед повторной попыткой.';

  @override
  String get kubeReviewChanges => 'Проверка изменений';

  @override
  String get kubeDryRun => 'Проверить на сервере';

  @override
  String get kubeDryRunRequired => 'Сначала проверьте YAML на сервере. Затем просмотрите изменения перед применением.';

  @override
  String get kubeDryRunHelp =>
      'Проверка на сервере: удалённые строки помечены −, добавленные +. Изменения ещё не сохранены в кластере.';

  @override
  String get kubeForceApply => 'Передать владение конфликтующими полями';

  @override
  String get kubeForceApplyHelp =>
      'Включайте только если намерены заменить поля, которыми управляет другой контроллер. После включения проверьте изменения заново.';

  @override
  String get kubeNoYamlChanges => 'Сервер не обнаружил изменений в YAML.';

  @override
  String get kubeDeleteResource => 'Удалить ресурс Kubernetes';

  @override
  String get kubeDeleteResourceHelp =>
      'Ресурс будет удалён из кластера. Это может остановить работающие приложения. Для подтверждения введите пароль хранилища.';

  @override
  String get kubeFollowLogs => 'Следить за новыми записями';

  @override
  String get kubePauseLogs => 'Пауза';

  @override
  String get kubeResumeLogs => 'Продолжить';

  @override
  String get kubeLogBufferHelp => 'Живой журнал: до 20 000 строк и 2 МиБ, только в памяти.';

  @override
  String get kubeLogOptions => 'Параметры журнала';

  @override
  String get kubeGitOps => 'GitOps';

  @override
  String get kubeProblems => 'Проблемы';

  @override
  String get kubeHelm => 'Helm';

  @override
  String get kubeInsightsPartial =>
      'Часть ресурсов не удалось проверить из-за ограничений прав, ошибок обнаружения API или объёма данных. Результаты неполные.';

  @override
  String get kubeGitOpsAll => 'Все';

  @override
  String get kubeGitOpsReady => 'Готовы';

  @override
  String get kubeGitOpsAttention => 'Требуют внимания';

  @override
  String get kubeGitOpsSuspended => 'Приостановлены';

  @override
  String get kubeDependencies => 'Зависимости';

  @override
  String get kubeNoVisibleProblems => 'Среди проверенных ресурсов проблемы не обнаружены.';

  @override
  String get kubeNoProblems => 'В проверенных ресурсах известные проблемы не обнаружены.';

  @override
  String get kubeNoGitOpsResources => 'Ресурсов Flux или Argo CD с таким фильтром нет.';

  @override
  String get kubeGitOpsActions => 'Действия контроллера';

  @override
  String get kubeDependencyCycle => 'Обнаружен цикл зависимостей';

  @override
  String get kubeDependencyMissing => 'Зависимость не найдена';

  @override
  String get kubeDependencyReady => 'Зависит от готового ресурса';

  @override
  String get kubeDependencyWaiting => 'Ожидает зависимость';

  @override
  String get kubeGitOpsSuspend => 'Приостановить синхронизацию';

  @override
  String get kubeGitOpsResume => 'Возобновить синхронизацию';

  @override
  String get kubeGitOpsReset => 'Сбросить счётчик ошибок';

  @override
  String get kubeGitOpsForce => 'Принудительно обновить Helm';

  @override
  String get kubeGitOpsReconcile => 'Синхронизировать сейчас';

  @override
  String get kubeArgoSync => 'Синхронизировать приложение';

  @override
  String get kubeArgoRefresh => 'Обновить состояние приложения';

  @override
  String get kubeArgoHardRefresh => 'Обновить состояние и манифесты';

  @override
  String get kubeGitOpsChangeHelp =>
      'Контроллер получит изменение, которое может повлиять на работающие приложения. Проверьте действие и введите пароль хранилища.';

  @override
  String get kubeGitOpsReason => 'Причина (сохраняется в аннотации ресурса)';

  @override
  String get kubeIssueCrashLoop => 'Контейнер постоянно перезапускается после сбоя';

  @override
  String get kubeIssueImagePull => 'Не удаётся скачать образ контейнера';

  @override
  String get kubeIssueContainerConfig => 'Ошибка конфигурации контейнера';

  @override
  String get kubeIssueOom => 'Контейнер превысил ограничение памяти';

  @override
  String get kubeIssueRestarts => 'Контейнер многократно перезапускался';

  @override
  String get kubeIssueScheduling => 'Не удаётся разместить под на узле';

  @override
  String get kubeIssuePodNotReady => 'Под не готов к работе';

  @override
  String get kubeIssueNodeNotReady => 'Узел не готов к работе';

  @override
  String get kubeIssueNodePressure => 'Узлу не хватает ресурсов';

  @override
  String get kubeIssueCondition => 'Ресурс требует внимания';

  @override
  String get kubeIssueRolloutDeadline => 'Развёртывание превысило допустимое время';

  @override
  String get kubeIssueReplicaFailure => 'Контроллер не смог создать реплики';

  @override
  String get kubeIssueReplicasUnavailable => 'Часть реплик недоступна';

  @override
  String get kubeIssueVolumePending => 'Запрос тома ожидает хранилище';

  @override
  String get kubeIssueVolumeLost => 'Запрос тома потерял связанный том';

  @override
  String get kubeIssueJobFailed => 'Задание завершилось ошибкой';

  @override
  String get kubeIssueArgoDegraded => 'Приложение Argo CD в состоянии Degraded';

  @override
  String get kubeIssueArgoOutOfSync => 'Приложение Argo CD не синхронизировано';

  @override
  String get kubeIssueFluxPending => 'Flux ещё не обработал последнюю конфигурацию';

  @override
  String get kubeIssueFluxNotReady => 'Ресурс Flux не готов';

  @override
  String get kubeIssueFluxStalled => 'Синхронизация Flux остановилась с ошибкой';

  @override
  String get kubeIssueFluxSuspended => 'Синхронизация Flux приостановлена';

  @override
  String get kubeHelmAllRevisions => 'Показать все ревизии';

  @override
  String get kubeHelmEmpty => 'Релизов Helm с таким фильтром нет.';

  @override
  String kubeHelmRevision(int revision) {
    return 'Ревизия $revision';
  }

  @override
  String get kubeHelmHistory => 'История релиза';

  @override
  String get kubeHelmSecretsMasked => 'Чувствительные значения скрыты. Значения и манифесты хранятся только в памяти.';

  @override
  String get kubeHelmValues => 'Пользовательские значения';

  @override
  String get kubeHelmChartValues => 'Значения чарта';

  @override
  String get kubeHelmManifest => 'Манифест';

  @override
  String get kubeHelmCompare => 'Сравнить с ревизией';

  @override
  String get kubeHelmChooseRevision => 'Выберите ревизию';

  @override
  String get kubeHelmDiffHelp => 'Удалённые строки помечены −, добавленные +. Сравнение не изменяет релиз.';

  @override
  String get kubeExecReviewTitle => 'Разрешить команду авторизации?';

  @override
  String get kubeExecReviewHelp =>
      'Команда запускается на этом компьютере. Проверьте программу, аргументы и окружение. Разрешение действует только в текущем разблокированном профиле; изменённая команда потребует нового подтверждения.';

  @override
  String get kubeExecArguments => 'Аргументы';

  @override
  String get kubeExecEnvironment => 'Окружение';

  @override
  String get kubeExecAllow => 'Разрешить эту команду';

  @override
  String get kubeAuthFailed => 'Не удалось завершить авторизацию. Проверьте настройки подключения и попробуйте снова.';

  @override
  String get kubeAuthExpired => 'Срок запроса истёк. Начните авторизацию заново.';

  @override
  String get kubeOidcTitle => 'Войти в кластер';

  @override
  String get kubeOidcHelp =>
      'Войдите через своего провайдера авторизации в системном браузере. Данные авторизации передаются только между этим устройством и провайдером.';

  @override
  String get kubeOidcOpen => 'Открыть браузер';

  @override
  String get kubeOidcWaiting => 'Ожидание входа в браузере…';

  @override
  String get kubeAuthAction => 'Авторизоваться';

  @override
  String get kubeExecMobile =>
      'Внешние команды авторизации недоступны на телефоне. Используйте токен, клиентский сертификат или вход через браузер (OIDC).';

  @override
  String get kubePodTerminal => 'Терминал Pod';

  @override
  String get kubeExecProductionBlocked => 'Терминал запрещён настройками этого production-контекста.';

  @override
  String get kubeAttach => 'Подключиться к процессу';

  @override
  String get kubeAttachHelp =>
      'Подключение к уже запущенному процессу контейнера. Ввод с клавиатуры поступает непосредственно в этот процесс.';

  @override
  String get kubeShell => 'Оболочка в контейнере';

  @override
  String get kubeOpenShell => 'Открыть оболочку';

  @override
  String get kubeTerminalFailed =>
      'Не удалось открыть терминал. Проверьте состояние контейнера, права доступа и выбранную оболочку.';

  @override
  String get kubeHidePanel => 'Скрыть панель терминала';

  @override
  String get kubeShowPanel => 'Открыть панель терминала';

  @override
  String get kubeTerminalEmpty => 'Откройте терминал в карточке Pod.';

  @override
  String get kubeTerminalConnected => 'Подключён';

  @override
  String get kubeTerminalDisconnected => 'Отключён';

  @override
  String get kubePortForward => 'Проброс порта Pod';

  @override
  String get kubeForwardHelp =>
      'Порт доступен только на этом устройстве по адресу 127.0.0.1. Проброс остановится при отключении кластера или блокировке профиля.';

  @override
  String get kubeForwardProductionBlocked => 'Проброс портов запрещён настройками этого production-контекста.';

  @override
  String get kubeRemotePort => 'TCP-порт контейнера';

  @override
  String get kubeLocalPort => 'Локальный порт';

  @override
  String get kubeLocalPortHelp => 'Укажите 0 для автоматического выбора свободного порта.';

  @override
  String get kubeStartForward => 'Начать проброс';

  @override
  String get kubeForwardCreated => 'Проброс запущен. Управляйте им в разделе «Туннели» или в панели кластера.';

  @override
  String get kubeForwardFailed =>
      'Не удалось пробросить порт. Проверьте состояние Pod, права и доступность локального порта.';

  @override
  String get kubePortForwards => 'Пробросы портов Kubernetes';

  @override
  String get kubeNoForwards => 'Нет активных пробросов Kubernetes.';

  @override
  String get kubeForwardRunning => 'Работает';

  @override
  String get kubeForwardStopped => 'Остановлен';

  @override
  String get kubeStopForward => 'Остановить проброс';

  @override
  String kubeForwardTraffic(int count, String sent, String received) {
    return 'Соединений: $count · отправлено $sent · получено $received';
  }

  @override
  String get kubePermissions => 'Права доступа';

  @override
  String get kubePermissionsHelp =>
      'Права, сообщённые Kubernetes для текущей учётной записи в этом пространстве имён. Защита production продолжает действовать.';

  @override
  String get kubePermissionsIncomplete =>
      'Сервер вернул неполный список прав. Отсутствие правила не означает, что действие запрещено.';

  @override
  String get kubeScale => 'Изменить число реплик';

  @override
  String get kubeRestart => 'Перезапустить workload';

  @override
  String get kubeCordon => 'Запретить размещение на узле';

  @override
  String get kubeUncordon => 'Разрешить размещение на узле';

  @override
  String get kubeDrain => 'Освободить узел';

  @override
  String get kubeDebugContainer => 'Добавить debug-контейнер';

  @override
  String get kubeMutationConfirmHelp =>
      'Проверьте действие и введите пароль хранилища для подтверждения. Права Kubernetes и защита production будут проверены перед выполнением.';

  @override
  String get kubeOperationFailed =>
      'Не удалось завершить действие. Проверьте права и состояние ресурса, затем обновите данные перед повтором.';

  @override
  String get kubeReplicas => 'Число реплик (0–1 000 000)';

  @override
  String get kubeContainerImage => 'Образ контейнера';

  @override
  String get kubeDebugHelp =>
      'Образ будет запущен внутри этого Pod с /bin/sh. Впоследствии удалить ephemeral-контейнер из Pod нельзя. Используйте доверенный образ.';

  @override
  String get kubeDrainHelp =>
      'Размещение на узле уже запрещено. Проверьте каждый Pod перед эвикцией. DaemonSet и зеркальные Pod пропускаются. Бюджеты недоступности соблюдаются; Pod без контроллера и данные emptyDir блокируют действие.';

  @override
  String kubeDrainProgress(int done, int total) {
    return 'Эвикция: $done из $total';
  }

  @override
  String get kubeDrainEmpty => 'Нет Pod для эвикции.';

  @override
  String get kubeDrainUnmanaged => 'Заблокировано: у Pod нет контроллера.';

  @override
  String get kubeDrainLocalData => 'Заблокировано: Pod использует локальные данные emptyDir.';

  @override
  String get kubeDrainBlocked => 'Заблокировано: проверьте этот Pod перед освобождением узла.';

  @override
  String get kubeMetrics => 'Потребление ресурсов';

  @override
  String get kubeMeasuredAt => 'Время измерения';

  @override
  String get kubeMetricsUnavailable => 'Метрики недоступны. Проверьте metrics-server и права доступа.';

  @override
  String get kubeWorkloadPods => 'Pod рабочей нагрузки';

  @override
  String get kubeEvents => 'События Kubernetes';

  @override
  String get kubeFluxCliTitle => 'Flux CLI';

  @override
  String get kubeFluxCliHelp =>
      'Дополнительные инструменты компьютера. Доступ к кластеру идёт через временный локальный прокси.';

  @override
  String get kubeFluxCliPath => 'Путь к Flux';

  @override
  String get kubeFluxCliFind => 'Найти';

  @override
  String get kubeFluxCliProbe => 'Проверить версию';

  @override
  String get kubeFluxCliVersion => 'Версия релиза';

  @override
  String get kubeFluxCliMethod => 'Способ установки';

  @override
  String get kubeFluxCliInstall => 'Установить';

  @override
  String get kubeFluxCliUpdate => 'Обновить';

  @override
  String get kubeFluxCliRemove => 'Удалить';

  @override
  String get kubeFluxCliOfficial => 'Официальный подписанный релиз';

  @override
  String get kubeFluxCliConfirm => 'Подтвердите операцию с CLI';

  @override
  String get kubeFluxCliVerify =>
      'Официальная загрузка требует проверки SHA-256 и подписи Cosign. Команды пакетного менеджера откроются в терминале.';

  @override
  String get kubeFluxCliFailed => 'Операция с CLI не выполнена или проверка недоступна.';

  @override
  String get kubeFluxCliOutput => 'Вывод CLI';

  @override
  String get kubeFluxCliRun => 'Запустить';

  @override
  String get kubeFluxCliTerminal => 'Открыть терминал Flux';

  @override
  String get kubeFluxCliLocalPath => 'Папка локального репозитория для diff';

  @override
  String get kubeFluxCliResource => 'Имя ресурса';

  @override
  String get kubeFluxCliKind => 'Тип ресурса';

  @override
  String get kubeNewConnection => 'Новое подключение';

  @override
  String get kubeConnectionSettings => 'Настройки подключения';

  @override
  String get kubeServerUrl => 'Адрес API Kubernetes (HTTPS)';

  @override
  String get kubeCertificateAuthority => 'Сертификат CA (PEM, необязательно)';

  @override
  String get kubeClientCertificate => 'Клиентский сертификат (PEM)';

  @override
  String get kubePrivateKey => 'Закрытый ключ клиента (PEM)';

  @override
  String get kubeManualReviewHelp =>
      'Далее проверьте и импортируйте контекст. Защита production включена по умолчанию.';

  @override
  String get kubeProductionContext => 'Production-контекст';

  @override
  String get kubeAllowExecInProd => 'Разрешить терминалы Pod в production';

  @override
  String get kubeAllowForwardInProd => 'Разрешить проброс портов в production';

  @override
  String get kubeRecordEvents => 'Сохранять историю событий';

  @override
  String get kubeRecordEventsHelp =>
      'В зашифрованном виде на этом устройстве в течение 7 дней. История событий не синхронизируется.';

  @override
  String get kubeScanFolder => 'Сканировать папку kubeconfig';

  @override
  String get kubeScanEmpty => 'В этой папке не найдено файлов kubeconfig.';

  @override
  String kubeQueuedFiles(int count) {
    return 'Файлов ожидают проверки: $count';
  }

  @override
  String get kubeNextFile => 'Проверить следующий файл';

  @override
  String get kubeExport => 'Экспорт kubeconfig';

  @override
  String get kubeExportHelp =>
      'Файл kubeconfig может содержать данные авторизации. Сохраняйте его только в доверенное место.';

  @override
  String get kubeIncludeCredentials => 'Включить данные авторизации';

  @override
  String get kubeExportSaved => 'Kubeconfig сохранён.';

  @override
  String get kubeAiAssistant => 'ИИ-помощник';

  @override
  String get kubeAiHelp =>
      'Инструменты только для чтения. Значения Secret исключены; доступ ограничен правами Kubernetes и выбранным контекстом. Ответы не запускают команды.';

  @override
  String get kubeAiProvider => 'Провайдер ИИ';

  @override
  String get kubeAiDefaultProvider => 'Провайдер по умолчанию на этом устройстве';

  @override
  String get kubeAiConfigMaps => 'Разрешить чтение значений ConfigMap для этого вопроса';

  @override
  String get kubeAiUnresolved => 'Часть проверок не выполнена; проверьте ошибки инструментов перед выводами.';

  @override
  String get kubeAiReadTool => 'Инструмент чтения';

  @override
  String get kubeAiTruncated => 'Результат сокращён.';

  @override
  String get kubeAiToolFailed => 'Проверка не завершена. Данные могут отсутствовать или доступ ограничен.';

  @override
  String get kubeAiFailed => 'Помощник не смог завершить запрос. Проверьте провайдера ИИ и подключение к кластеру.';

  @override
  String get kubeAiQuestion => 'Вопрос об этом кластере';

  @override
  String get kubeAiAsk => 'Спросить помощника';

  @override
  String get kubeAiExternalTitle => 'Передать контекст кластера этому провайдеру?';

  @override
  String get kubeAiExternalHelp =>
      'Для ответа очищенные метаданные ресурсов, состояния и логи могут быть переданы внешнему провайдеру, указанному выше. Значения Secret исключены. Разрешение действует только для этого вопроса.';

  @override
  String get kubeAiNoTools =>
      'Провайдер не вызывает инструменты; помощник использует только доступный контекст для чтения.';

  @override
  String get kubeAiExplain => 'Объяснить с ИИ';

  @override
  String kubeAiExplainQuestion(String problem, String resource) {
    return 'Объясни проблему «$problem» у $resource. Проверь связанные состояния и события без изменения ресурсов.';
  }

  @override
  String get sharingKubeContext => 'Контекст Kubernetes';

  @override
  String get sharingKubeCluster => 'Кластер';

  @override
  String get sharingKubeExclude =>
      'По умолчанию передаются только настройки подключения. Данные входа, локальные маршруты и разрешения остаются на этом устройстве.';

  @override
  String get sharingKubeIncludeCredentials => 'Передать данные для входа';

  @override
  String get sharingKubeCredentialWarning =>
      'Получатели смогут подключаться к кластеру с этими данными. Передавайте их только проверенным устройствам, которым доверяете. Отзыв доступа к публикации не отменяет уже скопированные получателем данные входа.';

  @override
  String get sharingKubeCredentialsIncluded => 'Данные для входа включены; значения скрыты.';

  @override
  String get sharingKubeCredentialsExcluded => 'Данные для входа не включены.';

  @override
  String get sharingKubeCopyHelp =>
      'Сохраните личную копию подключения и выберите свой маршрут. Данные для входа импортируются только по вашему выбору; запускаемые плагины всё равно потребуют разрешения на этом устройстве.';

  @override
  String get sharingKubeImportCredentials => 'Импортировать включённые данные для входа';

  @override
  String get sharingKubeRouteRequired =>
      'Для подключения нужен маршрут. Выберите сохранённый SSH-хост или профиль переходов на этом устройстве.';

  @override
  String get sharingKubeUnsupported => 'Этот сервер не поддерживает совместный доступ к контекстам Kubernetes.';

  @override
  String get sharingKubeSnapshotChanged =>
      'Если настройки или данные для входа изменились, проверьте подключение заново.';

  @override
  String get kubeHistory => 'История событий';

  @override
  String get kubeHistoryHelp =>
      'Локальная зашифрованная история за последние 7 дней. Запись включается в настройках подключения.';

  @override
  String get kubeHistoryRecording => 'Запись событий кластера';

  @override
  String get kubeHistoryDisabled => 'Запись истории отключена';

  @override
  String get kubeHistoryConnecting => 'Переподключение записи событий…';

  @override
  String get kubeHistoryUnavailable => 'Запись событий недоступна; сохранённая история доступна для чтения.';

  @override
  String get kubeHistoryWarnings => 'Только предупреждения';

  @override
  String get kubeHistoryClear => 'Очистить локальную историю событий';

  @override
  String get kubeHistoryClearHelp =>
      'Удалить сохранённую историю этого контекста на этом устройстве. Ресурсы кластера не изменятся.';

  @override
  String get kubeHelmComputedValues => 'Итоговые значения';

  @override
  String get kubeRevealSecret => 'Показать значения Secret';

  @override
  String get kubeRevealSecretHelp =>
      'Значения доступны только в этом защищённом диалоге в течение 30 секунд. Скопированные значения очищаются из буфера в течение 30 секунд. Они не передаются ИИ и не сохраняются в истории ресурсов.';

  @override
  String get kubeSecretExpired => 'Значения скрыты. При необходимости откройте их снова.';

  @override
  String get kubeCreateResource => 'Создать ресурс';

  @override
  String get kubeManifestTemplate => 'Вставить шаблон метаданных';

  @override
  String get kubeSchema => 'Схема ресурса Kubernetes';

  @override
  String get kubeHelmRollback => 'Откатить релиз Helm';

  @override
  String get kubeHelmRollbackHelp =>
      'Проверьте точный путь Helm, контрольную сумму и команду. Операция изменяет ресурсы кластера; хуки Helm отключены. Введите пароль хранилища для подтверждения.';

  @override
  String get kubeHelmRollbackFailed =>
      'Откат не завершён. Проверьте путь Helm, права и текущую ревизию релиза. Перед повтором заново проверьте команду.';

  @override
  String get kubeHelmBinary => 'Абсолютный путь к Helm (необязательно)';

  @override
  String get kubeAiHistory => 'История диалогов ИИ';

  @override
  String get kubeAiHistoryLocal => 'Сохранено на этом устройстве';

  @override
  String get kubeAiHistorySynced => 'Включена зашифрованная синхронизация';

  @override
  String get kubeAiHistoryDeleteHelp =>
      'Удалить сохранённый диалог. Если он синхронизируется, удаление также синхронизируется.';

  @override
  String get kubeRolloutHistory => 'История развёртываний';

  @override
  String get kubeDiffBefore => 'До';

  @override
  String get kubeDiffAfter => 'После';

  @override
  String get kubeCompare => 'Сравнить';

  @override
  String get kubeDiffTruncated => 'Различия показаны не полностью. Проверьте весь ресурс перед применением изменений.';

  @override
  String get kubeNoChanges => 'Изменений нет';

  @override
  String get kubeDiffReview => 'Проверьте изменённые поля. Значения секретов скрыты.';

  @override
  String get kubeFluxCliAdviceMissing => 'Flux CLI не найден. Установить?';

  @override
  String get kubeFluxCliAdviceUpdate => 'Версия CLI старше версии Flux в кластере. Обновить?';

  @override
  String get kubeFluxCliNeverSuggest => 'Больше не предлагать';

  @override
  String get kubeFluxCliLater => 'Не сейчас';

  @override
  String get kubeFluxCliPathHelp => 'Добавить Flux в PATH';

  @override
  String get kubeFluxCliPathHint =>
      'После изменения PATH откройте новое окно терминала. В macOS/Linux добавьте показанную строку в указанный файл профиля.';

  @override
  String get kubeFluxCliPathAdded => 'Папка добавлена в PATH пользователя.';

  @override
  String get kubeFluxCliController => 'Версия Flux в кластере';

  @override
  String get kubeGitOpsFlow => 'Поток ресурсов';

  @override
  String get kubeGraphList => 'Список';

  @override
  String get kubeGraphReset => 'Сбросить масштаб и положение';

  @override
  String get kubeIssueCertificateExpiring => 'Срок сертификата скоро истекает';

  @override
  String get kubeIssueCertificateExpired => 'Срок сертификата истёк';

  @override
  String get kubeIssueEvicted => 'Pod вытеснен';

  @override
  String get kubeIssuePendingResources => 'Недостаточно ресурсов кластера';

  @override
  String get kubeIssuePendingTaints => 'Метки taints узлов мешают размещению';

  @override
  String get kubeIssuePendingVolume => 'Pod ожидает том';

  @override
  String get kubeIssueSuspendedLong => 'Синхронизация приостановлена более 24 часов';

  @override
  String get kubeIssueSourceStale => 'Артефакт источника устарел';

  @override
  String get kubeIssueHelmDrift => 'Состояние Helm отличается от желаемого';

  @override
  String get kubeIssueDependencyNotReady => 'Зависимость не готова';

  @override
  String get kubeIssueDependencyMissing => 'Зависимость отсутствует или недоступна';

  @override
  String get kubeExitCode => 'Код завершения';

  @override
  String get kubeCommandOpen => 'Открыть контекст Kubernetes';

  @override
  String get kubeCommandAsk => 'Спросить ИИ о Kubernetes';

  @override
  String get kubeCommandContext => 'Контекст кластера';

  @override
  String get kubeRelatedObjects => 'Связанные ресурсы';

  @override
  String get kubeRelationOwner => 'Владелец';

  @override
  String get kubeRelationDependent => 'Дочерний ресурс';

  @override
  String get kubeRelationVolume => 'Связь с томом';

  @override
  String get kubeRelationConfiguration => 'Ссылка на конфигурацию';

  @override
  String get kubeMetricsTrend => 'Динамика CPU за текущий сеанс';

  @override
  String get kubeLogSince => 'За период';

  @override
  String get kubeLogAllTime => 'Всё время';

  @override
  String get kubeLogLastMinute => 'Последняя минута';

  @override
  String get kubeLogLastHour => 'Последний час';

  @override
  String get kubeLogLastDay => 'Последние сутки';

  @override
  String get kubeLogRegex => 'Регулярное выражение';

  @override
  String get kubeLogRegexInvalid => 'Некорректное или слишком сложное выражение.';

  @override
  String get kubeLogLevel => 'Уровень';

  @override
  String get kubeLogAllPods => 'Все поды';

  @override
  String get kubeLogAllLevels => 'Все уровни';

  @override
  String get kubeLogWrap => 'Переносить строки';

  @override
  String get kubeLogExport => 'Экспорт / отправка логов';

  @override
  String get kubeLogExportHelp =>
      'Логи могут содержать секреты. Будут сохранены только строки, видимые с текущим фильтром.';

  @override
  String get kubeLogMask => 'Скрыть распознанные секреты перед экспортом';

  @override
  String get kubeLogTargetsTruncated => 'Показаны первые 8 подов. Выберите конкретный под для остальных.';

  @override
  String get kubeLogSaved => 'Логи сохранены.';

  @override
  String get kubeTags => 'Метки через запятую';

  @override
  String get kubeTagsHelp => 'Метки и цвет кластера общие для его сохранённых контекстов.';

  @override
  String get kubeColor => 'Цвет кластера';

  @override
  String get kubeNoColor => 'Без цвета';

  @override
  String get kubeFavorite => 'Добавить или убрать избранное';

  @override
  String get kubeFavorites => 'Избранное';

  @override
  String get kubeAllKinds => 'Все типы';

  @override
  String get kubeGroupWorkloads => 'Нагрузки';

  @override
  String get kubeGroupNetwork => 'Сеть';

  @override
  String get kubeGroupConfig => 'Конфигурация';

  @override
  String get kubeGroupStorage => 'Хранилище';

  @override
  String get kubeGroupAccess => 'Доступ';

  @override
  String get kubeGroupCluster => 'Кластер / CRD';

  @override
  String get kubeLogDefaultContainer => 'Основной контейнер';

  @override
  String get kubeForwardChoosePod => 'Выберите Pod для проброса';

  @override
  String get kubeForwardChoosePodHelp =>
      'Выберите целевой Pod. Соединение откроется только после подтверждения портов.';

  @override
  String get kubeMemoryTrend => 'Динамика памяти за текущий сеанс';

  @override
  String get kubeRequests => 'Запрошено';

  @override
  String get kubeLimits => 'Лимиты';

  @override
  String get kubeCapacity => 'Ёмкость';

  @override
  String get kubeAllocatable => 'Доступно для размещения';

  @override
  String get kubeResourceBudgetsUnavailable => 'Запросы ресурсов и лимиты недоступны.';

  @override
  String get kubeBulkActions => 'Выбранные ресурсы';

  @override
  String get kubeBulkHelp =>
      'Проверьте список и введите пароль хранилища. Операции выполняются по очереди; отмена остановит их после текущей операции. Не более 100 ресурсов.';

  @override
  String get kubeBulkProductionBlocked => 'Массовые изменения недоступны в прод-контекстах.';

  @override
  String get kubeBulkFailed =>
      'Операция остановлена. Выполненные ресурсы отмечены; проверьте остальные перед повтором.';

  @override
  String get kubeContextTerminal => 'Локальный терминал кластера';

  @override
  String get kubeContextTerminalHelp =>
      'Терминал использует временный локальный API-прокси. Данные доступа остаются в ConsoleCrypt; ограничения прод-контекста продолжают действовать.';

  @override
  String get kubeOutput => 'Вывод';

  @override
  String get kubePinLogs => 'Открыть логи в нижней панели';

  @override
  String get kubeResizePanel => 'Изменить высоту нижней панели';

  @override
  String get kubeAiProposalReview => 'Проверить предложение';

  @override
  String get kubeAiProposalContext =>
      'Команда будет вставлена без выполнения в новый локальный терминал выбранного контекста.';

  @override
  String get kubeAiHistoryContext => 'Сообщений истории в запросе';

  @override
  String get kubeAiNewConversation => 'Новый разговор';

  @override
  String get kubeTableColumns => 'Столбцы';

  @override
  String get kubeAge => 'Возраст';

  @override
  String get kubeGitOpsSource => 'Источник';

  @override
  String get kubeGitOpsArtifact => 'Артефакт';

  @override
  String get kubeGitOpsInterval => 'Интервал сверки';

  @override
  String get kubeGitOpsRevision => 'Ревизия';

  @override
  String get kubeGitOpsSignature => 'Проверка подписи';

  @override
  String get kubeGitOpsInventory => 'Управляемые ресурсы';

  @override
  String get kubeGitOpsValuesFrom => 'Ссылки на значения';

  @override
  String get kubeGitOpsControllerLogs => 'Журнал контроллера';

  @override
  String get kubeGitOpsPath => 'Путь в репозитории';

  @override
  String get kubeGitOpsBranch => 'Ветка';

  @override
  String get kubeGitOpsTag => 'Тег / ограничение версии';

  @override
  String get kubeGitOpsChart => 'Чарт';

  @override
  String get kubeGitOpsDrift => 'Проверка расхождений';

  @override
  String get kubeGitOpsHistoryEmpty => 'История сверки пока недоступна.';

  @override
  String get kubeGitOpsControllerUnavailable => 'Журнал контроллера недоступен. Проверьте доступ к ресурсам кластера.';

  @override
  String get kubeGitOpsSignatureUnknown => 'Контроллер не сообщил результат';

  @override
  String get kubeGitOpsSourceReconcileHelp =>
      'Запросить новую сверку источника. Для продолжения введите пароль хранилища.';

  @override
  String get kubeGitOpsMetadataOnly => 'Здесь показаны только ссылки на объекты. Значения секретов скрыты.';

  @override
  String get kubeResourceConfiguration => 'Конфигурация нагрузки и узла';

  @override
  String get kubePort => 'Порт';

  @override
  String get kubePorts => 'Порты';

  @override
  String get kubeNode => 'Узел';

  @override
  String get kubeServiceAccount => 'Сервисная учётная запись';

  @override
  String get kubeNodeSelector => 'Выбор узла';

  @override
  String get kubeReady => 'Готовность';

  @override
  String get kubeRestarts => 'Перезапуски';

  @override
  String get kubeIssueExitCode => 'Код завершения';

  @override
  String get kubeVolumeMounts => 'Подключения томов';

  @override
  String get kubeVolumes => 'Тома';

  @override
  String get kubeUnschedulable => 'Планирование отключено';

  @override
  String get kubeTaints => 'Ограничения taints';

  @override
  String get kubeNodeInfo => 'Программное обеспечение узла';

  @override
  String get kubeGraphProblems => 'Пути к проблемам';

  @override
  String get kubeGraphSearch => 'Найти ресурс на графе';

  @override
  String get kubeGraphFold => 'Свернуть конечные ресурсы';

  @override
  String get kubeReplicaStatus => 'Готово / требуемое число реплик';

  @override
  String get kubeTableSort => 'Сортировка ресурсов';

  @override
  String get kubeForwardRestore => 'Восстанавливать после переподключения';

  @override
  String get kubeForwardRestoreHelp =>
      'Только пока этот контекст открыт и профиль разблокирован. При выходе из контекста выбор сбрасывается.';

  @override
  String get kubeForwardRestoreFailed =>
      'Не все перенаправления портов восстановлены. Проверьте под и локальный порт, затем запустите их снова.';

  @override
  String get kubeGitOpsReconcileWithSource => 'Синхронизировать вместе с источником';

  @override
  String get kubeGitOpsReconcileWithSourceHelp =>
      'Запросить обновление цепочки источников, затем синхронизацию этого ресурса. Готовность контроллеров отображается в живом статусе. Подтвердите точное имя ресурса.';

  @override
  String get kubeForwardNetwork => 'Разрешить доступ из локальной сети';

  @override
  String get kubeForwardBindAddress => 'Локальный IP-адрес для приёма подключений';

  @override
  String get kubeForwardNetworkWarning =>
      'Я понимаю, что другие устройства смогут обращаться к этому порту без авторизации Kubernetes. Разрешаю доступ из сети.';

  @override
  String get kubeHelmComputedPartial =>
      'Значения объединены из сохранённого релиза; значения по умолчанию дочерних чартов и глобальные импорты могут отсутствовать.';

  @override
  String get kubeManifestSnippets => 'Вставить сохранённый манифест';

  @override
  String get kubeManifestSnippetsHelp =>
      'Выберите сниппет kubectl с YAML или JSON. Проверьте имена и переменные в редакторе перед серверной проверкой.';

  @override
  String get kubeNoManifestSnippets =>
      'Сниппетов манифестов пока нет. Сохраните YAML или JSON как сниппет kubectl в разделе «Сниппеты».';

  @override
  String get kubeGitOpsSources => 'Источники';

  @override
  String get kubeGitOpsNotifications => 'Уведомления';

  @override
  String get kubeGitOpsImages => 'Автоматизация образов';

  @override
  String get kubeGitOpsRecent => 'Последние изменения статуса контроллеров';

  @override
  String get kubeDockRight => 'Переместить панель вправо';

  @override
  String get kubeDockBottom => 'Переместить панель вниз';

  @override
  String get kubeMaskedEditHelp =>
      'Манифест содержит скрытые значения. Изменение YAML отключено, чтобы не заменить учётные данные масками. Используйте отдельные действия ресурса или измените исходный доверенный манифест.';

  @override
  String get kubeAutoShell => 'Определить оболочку автоматически';

  @override
  String get kubeGitOpsLocalAlerts => 'Уведомления и напоминания на этом устройстве';

  @override
  String get kubeGitOpsNotify => 'Сообщать о сбоях Flux';

  @override
  String get kubeGitOpsNotifyHelp =>
      'Для этого контекста, пока приложение открыто. Новые переходы в Not ready или Stalled показываются уведомлением и значком.';

  @override
  String kubeGitOpsFailureNotice(String resource, int count) {
    return 'Flux требует внимания: $resource ($count)';
  }

  @override
  String kubeGitOpsReminderDue(String resource, int count) {
    return 'Напоминание о кластере: $resource ($count)';
  }

  @override
  String kubeGitOpsUnread(int count) {
    return 'Непрочитанных уведомлений: $count';
  }

  @override
  String get kubeGitOpsCancelReminder => 'Отменить напоминание';

  @override
  String get kubeGitOpsRemind => 'Локальное напоминание';

  @override
  String get kubeGitOpsNoReminder => 'Не напоминать';

  @override
  String kubeGitOpsRemindHours(int hours) {
    return 'Напомнить через $hours ч';
  }

  @override
  String get kubePreferencesFailed => 'Не удалось сохранить или загрузить настройки устройства. Попробуйте снова.';

  @override
  String get kubeContextWslHelp =>
      'WSL требует Bash и WSL 1 либо зеркальный режим сети WSL 2. Перед запуском проверяется доступ к локальному прокси; режим NAT не поддерживается.';

  @override
  String get kubeContextWslUnavailable =>
      'WSL не может подключиться к локальному прокси. Используйте WSL 1, зеркальный режим WSL 2 или PowerShell.';

  @override
  String get kubeKubectlMissing => 'kubectl не найден. Он нужен только для локального терминала кластера.';

  @override
  String get kubeKubectlInstall => 'Установить kubectl';

  @override
  String get kubeKubectlPackageHelp =>
      'Источник пакета, версию и папку установки определяет конфигурация пакетного менеджера. Версия kubectl должна отличаться от кластера не более чем на одну минорную версию. Команда запускается в видимом терминале; kubeconfig ей не передаётся.';

  @override
  String get kubeKubectlDocs => 'Инструкция по установке';

  @override
  String get kubeKubectlCheck => 'Проверить kubectl';

  @override
  String get kubeSchemaFields => 'Поля из схемы кластера';

  @override
  String get kubeSchemaHelp =>
      'Просматривайте поля и их описания из OpenAPI-схемы кластера. Вставка поля меняет только редактор; проверьте манифест перед применением.';

  @override
  String get kubeSchemaUnavailable =>
      'Схема кластера недоступна. Перед применением всё равно требуется серверная проверка.';

  @override
  String get kubeSchemaRequired => 'Обязательное';

  @override
  String get kubeSchemaInsert => 'Вставить поле у курсора';

  @override
  String get kubeSchemaNoFields => 'Подходящие поля схемы не найдены.';

  @override
  String get kubeSchemaParent => 'Родительское поле';

  @override
  String get kubeSchemaRoot => 'Корневые поля';

  @override
  String get kubeGitOpsAppliedRevision => 'Применённая ревизия';

  @override
  String get kubeGitOpsLatestRevision => 'Последняя ревизия источника';

  @override
  String get kubeGraphExpand => 'Развернуть объекты';

  @override
  String get kubeClusters => 'Кластеры';

  @override
  String get kubeExecInspect => 'Просмотреть команду';

  @override
  String get kubeExecImportHelp =>
      'Просмотрите команду перед импортом. Импорт её не запускает; разрешение на выполнение будет отдельно запрошено при подключении.';

  @override
  String get kubeExecBinaryUnavailable =>
      'Исполняемый файл недоступен на этом устройстве. Путь и SHA-256 будут проверены перед запуском.';

  @override
  String get kubeGitOpsTimeline => 'История сверки состояния';

  @override
  String get kubeHelmCompareLive => 'Сравнить с кластером';

  @override
  String get kubeHelmCompareHelp =>
      'Сравнение полей сохранённого манифеста с объектами кластера только для чтения. Значения по умолчанию и правила сверки контроллера не вычисляются.';

  @override
  String get kubeHelmCompareMasked => 'Скрытые поля не участвуют в сравнении.';

  @override
  String get kubeHelmCompareEqual => 'Видимые поля совпадают';

  @override
  String get kubeHelmCompareChanged => 'Есть отличия';

  @override
  String get kubeHelmCompareMissing => 'Объект отсутствует';

  @override
  String get kubeHelmTests => 'Тесты Helm';

  @override
  String get kubeGitOpsStalled => 'Сбой';

  @override
  String get kubeGitOpsStale => 'Устарели';

  @override
  String get kubeStatusUnknown => 'Ещё не проверен';

  @override
  String get kubeStatusChecking => 'Проверяется…';

  @override
  String get kubeStatusAvailable => 'Последняя проверка: доступен';

  @override
  String get kubeStatusForbidden => 'Последняя проверка: доступ запрещён';

  @override
  String get kubeStatusFailed => 'Последняя проверка: ошибка';

  @override
  String get kubeNavCluster => 'Кластер';

  @override
  String get kubeNavCustom => 'Пользовательские ресурсы';

  @override
  String get kubeNavCrdFilter => 'Фильтр CRD…';

  @override
  String get kubeNavOverview => 'Обзор';

  @override
  String get kubeNavHelmReleases => 'Релизы Helm';

  @override
  String get kubeLabelSelectorHint => 'Метки: app=api';

  @override
  String get kubeLiveShort => 'Онлайн';

  @override
  String get kubeSnapshotShort => 'Снимок';

  @override
  String kubeNoResourcesIn(String namespace) {
    return 'Нет ресурсов в $namespace';
  }

  @override
  String get kubePodIp => 'IP пода';

  @override
  String get kubeHostIp => 'IP узла';

  @override
  String get kubeQos => 'Класс QoS';

  @override
  String get kubeAnnotations => 'Аннотации';

  @override
  String get kubeContainers => 'Контейнеры';

  @override
  String get kubeControlledBy => 'Управляется';

  @override
  String get kubeDetailsSection => 'Сведения';

  @override
  String get kubeUsage => 'Потребление';

  @override
  String get kubeOperations => 'Операции';

  @override
  String get kubeSpecSection => 'Спецификация';

  @override
  String get kubeCloseInspector => 'Закрыть сведения';

  @override
  String get kubeImages => 'Образы';

  @override
  String get kubeShellShort => 'Терминал';

  @override
  String get kubeForwardShort => 'Порт';

  @override
  String get kubeAiShort => 'Спросить ИИ';

  @override
  String get kubeEditShort => 'Изменить';

  @override
  String get kubeHistoryShort => 'Ревизии';

  @override
  String get kubeRevealShort => 'Значения';

  @override
  String kubeAiClusterChat(String name) {
    return 'Кластер $name';
  }

  @override
  String get kubeAiGeneralChat => 'Общий чат';

  @override
  String get kubeToggleNavigator => 'Показать или скрыть ресурсы';

  @override
  String get kubeRevision => 'Ревизия';

  @override
  String get kubeAppVersion => 'Версия приложения';

  @override
  String get kubeUpdated => 'Обновлён';

  @override
  String get kubeScanHome => 'Найти в ~/.kube';

  @override
  String get kubeScanOtherFolder => 'Другая папка…';

  @override
  String get kubeImportPasteHint => 'Или вставьте YAML kubeconfig сюда';

  @override
  String get kubeEditorConnection => 'Подключение';

  @override
  String get kubeEditorOrganization => 'Организация';

  @override
  String get kubeEditorSafety => 'Безопасность';

  @override
  String get kubeReplicasReady => 'Реплики';

  @override
  String get kubeGitOpsAlertSettings => 'Уведомления и напоминания';

  @override
  String get kubeFluxCliOpen => 'Flux CLI';

  @override
  String get kubeOverviewNodes => 'Узлы';

  @override
  String get kubeOverviewPods => 'Поды запущены';

  @override
  String get kubeOverviewNamespaces => 'Пространства имён';

  @override
  String get kubeOverviewCpu => 'Процессор';

  @override
  String get kubeOverviewMemory => 'Память';

  @override
  String get kubeOverviewPodPhases => 'Поды по состояниям';

  @override
  String get kubeNavClusterOverview => 'Обзор кластера';

  @override
  String get settingsTerminalFontLabel => 'Шрифт';

  @override
  String get settingsTerminalFontSystem => 'Системный по умолчанию';

  @override
  String get settingsTerminalFontCustom => 'Другой шрифт…';

  @override
  String get settingsTerminalFontCustomTitle => 'Шрифт терминала';

  @override
  String get settingsTerminalFontCustomHint => 'Имя установленного шрифта, например JetBrains Mono';

  @override
  String get terminalTabRename => 'Переименовать…';

  @override
  String get terminalTabRenameTitle => 'Название вкладки';

  @override
  String get terminalTabResetName => 'Вернуть исходное название';

  @override
  String get terminalTabDuplicate => 'Дублировать';

  @override
  String get terminalTabClose => 'Закрыть';

  @override
  String get terminalTabCloseOthers => 'Закрыть другие вкладки';

  @override
  String get terminalTabCloseRight => 'Закрыть вкладки справа';

  @override
  String get kubeClusterProperties => 'Свойства…';

  @override
  String get kubeMarkAsTest => 'Пометить как тестовый';

  @override
  String get kubeMarkAsProduction => 'Пометить как продовый';

  @override
  String get kubeMarkAsTestConfirm =>
      'Для этого кластера отключатся защиты прода: дополнительные подтверждения и режим только чтения по умолчанию.';

  @override
  String get kubeClusterColorMenu => 'Цвет метки…';

  @override
  String get settingsTerminalTabSubtitle => 'Подпись вкладки';

  @override
  String get settingsTerminalTabSubtitleName => 'Имя из списка';

  @override
  String get settingsTerminalTabSubtitleAddress => 'Адрес или имя компьютера';

  @override
  String get settingsTerminalTabSubtitleOff => 'Не показывать';

  @override
  String get kubeApplyConflict =>
      'Часть полей принадлежит другому контроллеру, или объект изменился. Включите «Передать владение конфликтующими полями» или обновите объект.';

  @override
  String get kubeApplyForbidden => 'Кластер не разрешает вам изменять этот объект.';

  @override
  String get kubeApplyInvalid =>
      'Сервер отклонил YAML: проверьте синтаксис и поля, которые нельзя менять (большая часть spec у Pod неизменяема).';

  @override
  String kubeApplyOwnedBy(String managers) {
    return 'Эти поля ведёт $managers. Включите «Передать владение конфликтующими полями», чтобы перезаписать их; контроллер может вернуть значения обратно.';
  }

  @override
  String get kubeApplyRejectedFields => 'Сервер отклонил поля:';

  @override
  String kubeApplyPodImmutable(String owner) {
    return 'У запущенного пода можно менять только image, activeDeadlineSeconds, добавлять tolerations, а также labels и annotations. Меняйте владельца: $owner.';
  }

  @override
  String get shellAiNone => 'ИИ не настроен — открыть настройки';

  @override
  String shellAiDevice(String model) {
    return 'ИИ: встроенная модель на устройстве · $model';
  }

  @override
  String shellAiLocal(String name, String model) {
    return 'ИИ: локальный сервер $name · $model';
  }

  @override
  String shellAiApi(String name, String model) {
    return 'ИИ: через API $name · $model (запросы уходят с компьютера)';
  }

  @override
  String get sshConfigImportButton => 'Из ~/.ssh/config';

  @override
  String get sshConfigImportTitle => 'Импорт из ~/.ssh/config';

  @override
  String get sshConfigImportHelp =>
      'Записи Host станут хостами: адрес, порт, пользователь, jump-хосты и ключ. Блоки с масками (Host *) применяются как значения по умолчанию.';

  @override
  String sshConfigNotFound(String path) {
    return 'Файл $path не найден.';
  }

  @override
  String get sshConfigImportKeys => 'Импортировать ключи из IdentityFile';

  @override
  String get sshConfigImportKeysHelp =>
      'Ключ сохраняется в хранилище как есть; зашифрованный ключ остаётся со своей парольной фразой.';

  @override
  String sshConfigVia(String hosts) {
    return 'через $hosts';
  }

  @override
  String get sshConfigProxyCommand => 'ProxyCommand не поддерживается; хост импортируется без него';

  @override
  String sshConfigKeyFailed(String path) {
    return 'Ключ $path не импортирован';
  }

  @override
  String get settingsShortcutsTitle => 'Горячие клавиши';

  @override
  String get settingsShortcutsSubtitle =>
      'Основной модификатор — ⌘ на macOS и Ctrl на Windows/Linux. Сочетания приложения обрабатываются раньше терминала.';

  @override
  String get settingsShortcutsReset => 'Восстановить по умолчанию';

  @override
  String get settingsShortcutChange => 'Изменить';

  @override
  String settingsShortcutResetOne(String shortcut) {
    return 'Вернуть $shortcut';
  }

  @override
  String get settingsShortcutOff => 'Выключено';

  @override
  String settingsShortcutCaptureTitle(String command) {
    return 'Сочетание для «$command»';
  }

  @override
  String get settingsShortcutCaptureHint => 'Нажмите сочетание с ⌘/Ctrl или Alt. Esc — отмена.';

  @override
  String settingsShortcutConflict(String command) {
    return 'Уже назначено команде «$command» — у неё сочетание будет снято.';
  }

  @override
  String get settingsShortcutReserved =>
      'Это сочетание использует система или терминал — в терминал оно больше не попадёт.';

  @override
  String get settingsShortcutAssign => 'Назначить';

  @override
  String get settingsShortcutDisable => 'Выключить';

  @override
  String get settingsShortcutReservedBadge => 'Пересекается с системой или терминалом';

  @override
  String get kubeMetricsInstall => 'Установить metrics-server';

  @override
  String kubeMetricsInstallHelp(String version) {
    return 'Базовый metrics-server $version в kube-system: ServiceAccount, RBAC, Service, Deployment и APIService v1beta1.metrics.k8s.io. Образ кластер скачает сам с registry.k8s.io.';
  }

  @override
  String get kubeMetricsInsecureTls => 'Kubelet с самоподписанными сертификатами (--kubelet-insecure-tls)';

  @override
  String get kubeMetricsInsecureTlsHelp =>
      'Нужно для локальных кластеров (OrbStack, kind, Docker Desktop, minikube). Отключает проверку сертификата kubelet.';

  @override
  String kubeMetricsInstalled(int created, int existing) {
    return 'metrics-server установлен (создано $created, уже было $existing). Метрики появятся примерно через минуту.';
  }

  @override
  String kubeMetricsInstallDenied(String kind, String name) {
    return '$kind $name: кластер отказал. Проверьте права; для прод-кластера откройте окно изменений.';
  }

  @override
  String kubeMetricsInstallFailed(String kind, String name) {
    return '$kind $name не создан.';
  }

  @override
  String kubeMetricsUnknownKind(String kind, String apiVersion) {
    return 'Кластер не предоставляет $kind ($apiVersion).';
  }

  @override
  String get navPortForwards => 'Пробросы портов';

  @override
  String kubeForwardsOpen(int count) {
    return 'Пробросы: $count';
  }

  @override
  String get kubeForwardCopyAddress => 'Скопировать локальный адрес';

  @override
  String get kubeForwardsEmptyHelp => 'Запустите проброс из пода или сервиса: «Проброс порта».';

  @override
  String get dbTitle => 'Базы данных';

  @override
  String get dbSubtitle => 'Зашифрованные подключения; запросы идут напрямую с этого устройства.';

  @override
  String get dbUnavailable => 'Работа с базами данных недоступна в этой сборке.';

  @override
  String get dbNewConnection => 'Новое подключение к БД';

  @override
  String get dbImport => 'Импорт подключения';

  @override
  String get dbConnectionString => 'Строка подключения';

  @override
  String get dbImportHelp => 'Пароль остаётся в зашифрованном хранилище. Проверьте адрес перед сохранением.';

  @override
  String get dbImportPgpass => 'Выбрать файл .pgpass';

  @override
  String get dbImportExpired => 'Срок предварительного просмотра истёк. Повторите импорт.';

  @override
  String get dbEmpty => 'Подключений к базам данных пока нет.';

  @override
  String get dbFailed => 'Не удалось выполнить операцию. Проверьте настройки подключения и права доступа.';

  @override
  String get dbInvalid => 'Проверьте выделенные поля.';

  @override
  String get dbRequired => 'Обязательное поле';

  @override
  String get dbName => 'Имя подключения';

  @override
  String get dbEngine => 'Движок БД';

  @override
  String get dbHost => 'Адрес';

  @override
  String get dbPort => 'Порт';

  @override
  String get dbDatabase => 'База данных / индекс Redis';

  @override
  String get dbUsername => 'Пользователь';

  @override
  String get dbPassword => 'Пароль';

  @override
  String get dbSavePassword => 'Сохранить пароль в зашифрованном хранилище';

  @override
  String get dbSavedPassword => 'Сохранённый пароль останется прежним, если его не заменить или удалить.';

  @override
  String get dbClearPassword => 'Удалить сохранённый пароль';

  @override
  String get dbProduction => 'Прод — только чтение, запись лишь во временном окне';

  @override
  String get dbProductionHelp =>
      'Прод-подключение открывается только на чтение. Кнопка «Разблокировать прод» в рабочей области разрешает запись на 5–30 минут после пароля хранилища и имени подключения; каждое изменение по-прежнему проверяется. Для повседневной работы лучше учётная запись БД с правами только на чтение.';

  @override
  String get dbProd => 'ПРОД';

  @override
  String get dbReadOnly => 'ТОЛЬКО ЧТЕНИЕ';

  @override
  String get dbWriteAllowed => 'Запись разрешена в этой сессии';

  @override
  String get dbAllowWrite => 'Разрешить запись в этой сессии';

  @override
  String get dbAllowWriteHelp =>
      'Запись будет разрешена только в текущей непрод-сессии. Опасные запросы потребуют отдельного подтверждения.';

  @override
  String get dbRevokeWrite => 'Вернуть режим только чтения';

  @override
  String get dbPrivileges => 'Проверить права';

  @override
  String get dbPrivilegesWarning =>
      'У учётной записи могут быть права записи. Для прод-базы используйте роль только для чтения.';

  @override
  String get dbServerReadOnly => 'Серверная защита только для чтения включена.';

  @override
  String get dbRoleReadOnly => 'Проверена роль учётной записи только для чтения.';

  @override
  String get dbGuardOnly => 'Защита запросов включена; серверные права только для чтения не подтверждены.';

  @override
  String get dbTls => 'Защита TLS';

  @override
  String get dbTlsVerify => 'Проверять сертификат и имя сервера';

  @override
  String get dbTlsRequire => 'Требовать шифрование';

  @override
  String get dbTlsPrefer => 'Предпочитать шифрование';

  @override
  String get dbTlsDisable => 'Отключить TLS';

  @override
  String get dbTlsWarning =>
      'Этот режим не проверяет подлинность сервера БД. Рекомендуется проверка сертификата и имени сервера.';

  @override
  String get dbTlsCa => 'Сертификат центра сертификации (PEM)';

  @override
  String get dbTlsServerName => 'Имя сервера для проверки TLS';

  @override
  String get dbClientCertificate => 'Клиентский сертификат (PEM)';

  @override
  String get dbPrivateKey => 'Закрытый ключ клиента (PEM)';

  @override
  String get dbAuthMechanism => 'Механизм авторизации';

  @override
  String get dbAuthDefault => 'По умолчанию';

  @override
  String get dbRoute => 'Маршрут подключения';

  @override
  String get dbDirect => 'Напрямую';

  @override
  String get dbSsh => 'Хост SSH';

  @override
  String get dbJump => 'Профиль SSH-переходов';

  @override
  String get dbProxy => 'Прокси';

  @override
  String get dbRouteHelp => 'Через SSH-маршрут TLS также проверяет исходное имя сервера БД.';

  @override
  String get dbGroup => 'Группа';

  @override
  String get dbUngrouped => 'Без группы';

  @override
  String get dbTags => 'Метки через запятую';

  @override
  String get dbColor => 'Цвет';

  @override
  String get dbLimits => 'Ограничения';

  @override
  String get dbMaxRows => 'Начальное число строк результата';

  @override
  String get dbStatementTimeout => 'Таймаут запроса, секунды';

  @override
  String get dbConnectTimeout => 'Таймаут подключения, секунды';

  @override
  String get dbMemoryLimit => 'Память результата, МиБ';

  @override
  String get dbOptions => 'Параметры драйвера';

  @override
  String get dbApplicationName => 'Имя приложения';

  @override
  String get dbSearchPath => 'Путь поиска схем через запятую';

  @override
  String get dbCharset => 'Кодировка';

  @override
  String get dbMongoAuthSource => 'База авторизации';

  @override
  String get dbMongoReplicaSet => 'Набор реплик';

  @override
  String get dbMongoSrv => 'Поиск серверов через DNS SRV';

  @override
  String get dbMongoReadPreference => 'Предпочтение чтения';

  @override
  String get dbMongoReadHelp => 'Предпочтение чтения выбирает реплику и не запрещает запись.';

  @override
  String get dbTestConnection => 'Проверить подключение';

  @override
  String get dbTestSuccess => 'Подключение установлено.';

  @override
  String get dbDisconnect => 'Отключиться';

  @override
  String get dbRefresh => 'Обновить';

  @override
  String get dbDeleteHelp => 'Удалить сохранённое подключение? Активные сессии будут закрыты.';

  @override
  String get dbLocked => 'Разблокируйте хранилище для доступа к подключениям БД.';

  @override
  String get dbTree => 'Структура';

  @override
  String get dbDdl => 'Показать DDL';

  @override
  String get dbFirstRows => 'Первые 100 строк';

  @override
  String get dbInsertName => 'Вставить имя';

  @override
  String get dbInspect => 'Сведения об объекте';

  @override
  String get dbSampleSchema => 'Схема по выборке документов';

  @override
  String get dbSampleHelp =>
      'Для определения типов полей будут прочитаны документы выбранной коллекции. Данные не отправляются ИИ.';

  @override
  String get dbLoadMore => 'Загрузить ещё';

  @override
  String get dbEditor => 'Редактор запросов';

  @override
  String get dbNewQuery => 'Новый запрос';

  @override
  String get dbRun => 'Выполнить';

  @override
  String get dbRunCurrent => 'Текущий оператор';

  @override
  String get dbRunSelection => 'Выделение';

  @override
  String get dbRunScript => 'Весь скрипт';

  @override
  String get dbCancelQuery => 'Отменить запрос';

  @override
  String get dbFormat => 'Форматировать запрос';

  @override
  String get dbComment => 'Закомментировать строки';

  @override
  String get dbFindReplace => 'Поиск и замена';

  @override
  String get dbReplace => 'Заменить';

  @override
  String get dbReplaceAll => 'Заменить всё';

  @override
  String get dbVariables => 'Переменные запроса';

  @override
  String get dbVariablesHelp => 'Значения передаются параметрами драйвера и не склеиваются с текстом запроса.';

  @override
  String get dbValueType => 'Тип значения';

  @override
  String get dbValue => 'Значение';

  @override
  String get dbReviewQuery => 'Подтверждение запроса';

  @override
  String get dbQueryBlocked =>
      'Запрос запрещён политикой подключения. Используйте непрод-подключение с нужными правами.';

  @override
  String get dbQueryDangerous =>
      'Этот запрос может изменить или удалить данные. Проверьте запрос и введите пароль хранилища.';

  @override
  String get dbReviewExpired => 'Срок подтверждения истёк. Запустите запрос заново для повторной проверки.';

  @override
  String get dbResults => 'Результаты';

  @override
  String get dbGrid => 'Сетка';

  @override
  String get dbRecords => 'Записи';

  @override
  String get dbNoResults => 'Выполните запрос для просмотра результата.';

  @override
  String get dbNull => 'NULL';

  @override
  String get dbTruncated => 'Достигнут лимит результата. Для полной выгрузки используйте экспорт.';

  @override
  String get dbCancelled => 'Запрос отменён.';

  @override
  String get dbRunning => 'Запрос выполняется…';

  @override
  String get dbMessages => 'Сообщения сервера';

  @override
  String get dbShowValue => 'Показать значение полностью';

  @override
  String get dbPinColumn => 'Закрепить столбец';

  @override
  String get dbUnpinColumn => 'Открепить столбец';

  @override
  String get dbColumns => 'Столбцы';

  @override
  String get dbCopyCell => 'Скопировать ячейку';

  @override
  String get dbCopyRow => 'Скопировать строку';

  @override
  String get dbCopySelection => 'Скопировать выделение';

  @override
  String get dbCopyResult => 'Скопировать загруженный результат';

  @override
  String get dbSensitiveClipboard => 'Буфер обмена очищается автоматически согласно настройкам безопасности.';

  @override
  String get dbExport => 'Экспорт результата';

  @override
  String get dbExportFormat => 'Формат';

  @override
  String get dbExportHeader => 'Добавить имена столбцов';

  @override
  String get dbExportBom => 'Метка UTF-8 BOM';

  @override
  String get dbExportDelimiter => 'Разделитель';

  @override
  String get dbExportQuote => 'Символ кавычек';

  @override
  String get dbExportHelp =>
      'Экспорт записывает полный результат потоком в выбранный файл. Результаты могут содержать чувствительные данные.';

  @override
  String get dbExportDone => 'Экспорт завершён.';

  @override
  String get dbHistory => 'История запросов';

  @override
  String get dbFavorites => 'Избранное';

  @override
  String get dbAllQueries => 'Все запросы';

  @override
  String get dbSuccessful => 'Успешные';

  @override
  String get dbWithErrors => 'С ошибками';

  @override
  String get dbHistoryLocal => 'Локальная зашифрованная история';

  @override
  String get dbHistorySync => 'Зашифрованная синхронизация';

  @override
  String get dbHistoryDisabled => 'История отключена';

  @override
  String get dbClearHistory => 'Очистить историю запросов';

  @override
  String get dbClearHistoryHelp => 'Удалить сохранённую историю запросов этого подключения?';

  @override
  String get dbSaveSnippet => 'Сохранить как сниппет';

  @override
  String get dbSnippetName => 'Имя сниппета';

  @override
  String get dbExplain => 'EXPLAIN';

  @override
  String get dbExplainAnalyze => 'Выполнить с ANALYZE';

  @override
  String get dbExplainAnalyzeHelp =>
      'ANALYZE выполняет запрос для сбора измерений. Доступно только для непрод-подключений.';

  @override
  String get dbEditCell => 'Изменить ячейку';

  @override
  String get dbApplyEdits => 'Проверить изменения';

  @override
  String get dbDiscardEdits => 'Отменить изменения';

  @override
  String get dbCellEditsHelp =>
      'Проверьте параметризованные UPDATE. Изменения применяются одной транзакцией с откатом при ошибке.';

  @override
  String get dbAi => 'ИИ-помощник';

  @override
  String get dbAiQuestion => 'Опишите, что нужно';

  @override
  String get dbAiProvider => 'Провайдер ИИ';

  @override
  String get dbAiCompose => 'Составить запрос';

  @override
  String get dbAiExplain => 'Объяснить запрос';

  @override
  String get dbAiFix => 'Исправить с ИИ';

  @override
  String get dbAiOptimize => 'Оптимизировать с ИИ';

  @override
  String get dbAiTranslate => 'Перевести в другой диалект';

  @override
  String get dbAiSnippet => 'Сниппет с переменными';

  @override
  String get dbAiExamples => 'Типовые запросы';

  @override
  String get dbAiResult => 'Объяснить выбранные строки';

  @override
  String get dbAiDraft => 'Черновик ИИ — проверьте перед принятием';

  @override
  String get dbAiAccept => 'Принять черновик';

  @override
  String get dbAiReject => 'Отклонить';

  @override
  String get dbAiRefine => 'Уточнить';

  @override
  String get dbAiConsent => 'Проверьте контекст ИИ';

  @override
  String get dbAiSchema => 'Разрешить метаданные схемы';

  @override
  String get dbAiRows => 'Разрешить явно выбранные строки результата';

  @override
  String get dbAiSample => 'Разрешить sample_rows (до 20 строк)';

  @override
  String get dbAiReadonly => 'Разрешить run_readonly (до 50 строк)';

  @override
  String get dbAiConsentHelp =>
      'Схема и данные строк требуют отдельного согласия. Вызовы инструментов только читают данные, показываются ниже и не могут применять изменения.';

  @override
  String get dbAiExternal =>
      'Это внешний провайдер. Строгая приватность заменяет имена перед отправкой и восстанавливает их локально.';

  @override
  String get dbAiLocal => 'Этот провайдер работает локально.';

  @override
  String get dbAiContextLimit => 'Максимум объектов схемы в контексте';

  @override
  String get dbAiTools => 'Контекст, отправленный модели';

  @override
  String get dbAiNoExecute => 'Принятие черновика не выполняет его. Проверьте запрос и нажмите «Выполнить».';

  @override
  String get dbAiProdDdl => 'Предложения DDL для прод-базы служат рекомендацией и не выполняются.';

  @override
  String get dbAiRevoke => 'Отозвать согласие для ИИ';

  @override
  String get dbBackgroundIos => 'На iOS выполняющийся запрос отменяется через 30 секунд в фоне.';

  @override
  String get dbCloseRunning => 'Закрыть запрос и отменить его выполнение?';

  @override
  String get dbOperationFailed =>
      'Операция с базой данных не выполнена. Проверьте подключение и права, затем повторите.';

  @override
  String get dbTreeSearch => 'Поиск по загруженным объектам';

  @override
  String get dbInvalidValue => 'Проверьте значение и его тип.';

  @override
  String get dbComplete => 'Дополнить запрос';

  @override
  String get dbBinaryPreview => 'Просмотр hex (первые 4096 байт)';

  @override
  String get dbAiNewConversation => 'Новый разговор';

  @override
  String get dbAiHistoryMessages => 'Сообщений истории в запросе';

  @override
  String get dbVerifiedReadOnly => 'Режим только чтения проверен на стороне сервера.';

  @override
  String get dbRoleCanWrite =>
      'У этой роли базы данных есть права записи. Используйте отдельную роль только для чтения в production.';

  @override
  String get dbResultStale => 'Изменения применены. Выполните запрос снова для обновления результата.';

  @override
  String get dbAllowWritesHelp =>
      'Разрешить запись в этой непродуктивной сессии? Каждый изменяющий запрос всё равно требует проверки. Разрешение завершится при закрытии подключения.';

  @override
  String get dbCheckPrivileges => 'Проверить права';

  @override
  String get dbAllowWrites => 'Разрешить запись';

  @override
  String get dbUnverifiedReadOnly =>
      'Режим только чтения на сервере не подтверждён. Используйте отдельную роль базы данных только для чтения.';

  @override
  String dbElapsed(int ms) {
    return '$ms мс';
  }

  @override
  String get dbQueryBlockedTitle => 'Запрос заблокирован';

  @override
  String get dbScanInsteadOfKeys =>
      'KEYS перебирает все ключи разом и может остановить сервер. Используйте SCAN — он читает ключи порциями.';

  @override
  String get dbOpenSuggestion => 'Открыть в новой вкладке';

  @override
  String get dbPlanByTime => 'Полосы — доля времени выполнения, которую занял сам шаг.';

  @override
  String get dbPlanByCost => 'Полосы — доля оценочной стоимости, которую занял сам шаг.';

  @override
  String dbPlanTime(String ms) {
    return '$ms мс';
  }

  @override
  String dbPlanRows(int count) {
    return '$count строк';
  }

  @override
  String dbPlanCost(String cost) {
    return 'стоимость $cost';
  }

  @override
  String get dbPlanTree => 'Дерево плана';

  @override
  String get dbPlanJson => 'JSON';

  @override
  String get dbErrorTls =>
      'Не удалось установить TLS: сервер не поддерживает TLS или сертификат не прошёл проверку. Для проброса порта или локальной базы выберите TLS «prefer» или «disable»; для удалённого сервера проверьте сертификат и имя сервера.';

  @override
  String get dbErrorAuth => 'Сервер отклонил вход: проверьте пользователя, пароль и имя базы.';

  @override
  String get dbErrorConnection =>
      'Не удаётся подключиться к серверу: проверьте адрес, порт и что проброс порта или туннель запущен.';

  @override
  String get dbErrorTimeout => 'Сервер не ответил вовремя. Проверьте адрес и таймаут подключения.';

  @override
  String get dbErrorRoute => 'Выбранный маршрут недоступен: проверьте SSH-хост или jump-профиль.';

  @override
  String get dbTlsLocalHint => 'Локальный адрес: TLS переключён на «prefer» (у проброса порта обычно нет TLS).';

  @override
  String get dbTransportVerified => 'TLS (проверен)';

  @override
  String get dbTransportUnverified => 'TLS (без проверки сертификата)';

  @override
  String get dbTransportTunnel => 'Без TLS — защищено туннелем';

  @override
  String get dbTransportPlaintext => 'Без TLS';

  @override
  String get dbTlsPreferHelp => 'TLS, если сервер поддерживает; без TLS только через туннель или на этом компьютере.';

  @override
  String get dbTlsRequireHelp => 'Шифрование без проверки сертификата.';

  @override
  String get dbTlsAllowUnbound => 'Разрешить вход без привязки к TLS (менее безопасно)';

  @override
  String get dbTlsAllowUnboundHelp =>
      'Только для серверов с аутентификацией md5 или password либо без SCRAM-SHA-256-PLUS. Без проверки сертификата перехватчик сможет получить пароль.';

  @override
  String get dbErrorTlsUnavailable =>
      'Сервер не поддерживает TLS. Подключение без шифрования передаст пароль в открытом виде. Выберите режим «Отключить TLS» явно, если это допустимо.';

  @override
  String get dbErrorChannelBinding =>
      'Сервер не привязал пароль к этому TLS-соединению (SCRAM-SHA-256-PLUS), а сертификат не проверяется: перехватчик мог бы получить пароль. Выберите «Проверять сертификат и имя сервера», включите SCRAM на сервере или разрешите вход без привязки к TLS в разделе «Защита TLS».';

  @override
  String get dbErrorCleartext =>
      'Сервер запросил пароль в открытом виде по незашифрованному сетевому подключению. Включите TLS или подключайтесь через туннель.';

  @override
  String get colorPickerCustom => 'Свой цвет';

  @override
  String get kubeConnectDatabase => 'Подключиться к БД';

  @override
  String kubeConnectDatabaseHelp(String engine) {
    return 'Пробросить порт $engine на localhost и открыть в разделе «Базы данных»';
  }

  @override
  String get dockerTitle => 'Docker';

  @override
  String get dockerSubtitle => 'Контейнеры на этом компьютере или на сохранённом SSH-хосте.';

  @override
  String get dockerUnavailable => 'Docker недоступен';

  @override
  String get dockerUnavailableHelp => 'Эта сборка приложения не умеет подключаться к Docker.';

  @override
  String get dockerSearchHosts => 'Поиск хостов';

  @override
  String get dockerRefresh => 'Обновить';

  @override
  String get dockerHostsFailed => 'Не удалось загрузить список хостов.';

  @override
  String get dockerNoHosts => 'Нет хостов';

  @override
  String get dockerNoHostsHelp => 'Docker не найден на этом компьютере, и сохранённых SSH-хостов пока нет.';

  @override
  String get dockerLocalHost => 'Этот компьютер';

  @override
  String get dockerLocalHostHelp => 'Локальный Docker';

  @override
  String get dockerConnect => 'Подключиться';

  @override
  String get dockerConnecting => 'Подключение…';

  @override
  String get dockerReconnect => 'Подключиться снова';

  @override
  String get dockerPermissionDeniedTitle => 'Нет доступа к сокету Docker';

  @override
  String dockerPermissionDenied(String host, String group) {
    return 'У SSH-пользователя на $host нет прав на сокет Docker. Добавьте пользователя в группу «$group» на хосте и подключитесь снова:';
  }

  @override
  String get dockerPermissionDeniedNote =>
      'Выполните её на хосте сами — приложение её не запускает. Новая группа действует в новых SSH-сессиях.';

  @override
  String get dockerCopyCommand => 'Скопировать команду';

  @override
  String get dockerNotRunningTitle => 'Docker не запущен';

  @override
  String dockerNotRunning(String host) {
    return 'На $host Docker не запущен или не установлен. Запустите службу Docker и подключитесь снова.';
  }

  @override
  String get dockerConnectionFailedTitle => 'Не удалось подключиться';

  @override
  String dockerConnectionFailed(String host) {
    return 'Не удалось подключиться к $host.';
  }

  @override
  String get dockerRequestFailed => 'Запрос к Docker не выполнен';

  @override
  String get dockerRetry => 'Повторить';

  @override
  String get dockerDisconnect => 'Отключиться и выбрать другой хост';

  @override
  String dockerEngineVersion(String version) {
    return 'Engine $version';
  }

  @override
  String dockerRunningOfTotal(int running, int total) {
    return 'Запущено $running из $total';
  }

  @override
  String get dockerForwards => 'Проброс портов';

  @override
  String get dockerFilterContainers => 'Фильтр по имени или образу';

  @override
  String get dockerNoContainers => 'Нет контейнеров';

  @override
  String get dockerNoContainersHelp => 'Здесь появятся контейнеры, запущенные на этом Docker.';

  @override
  String get dockerNoMatches => 'Подходящих контейнеров нет';

  @override
  String get dockerContainerGone => 'Контейнер больше не существует';

  @override
  String get dockerStandalone => 'Контейнеры';

  @override
  String get dockerCompose => 'compose';

  @override
  String dockerGroupRunning(int running, int total) {
    return '$running/$total запущено';
  }

  @override
  String get dockerStateCreated => 'Создан';

  @override
  String get dockerStateRunning => 'Работает';

  @override
  String get dockerStatePaused => 'Приостановлен';

  @override
  String get dockerStateRestarting => 'Перезапускается';

  @override
  String get dockerStateRemoving => 'Удаляется';

  @override
  String get dockerStateExited => 'Остановлен';

  @override
  String get dockerStateDead => 'Неисправен';

  @override
  String get dockerStateUnknown => 'Неизвестно';

  @override
  String get dockerHealthHealthy => 'Исправен';

  @override
  String get dockerHealthUnhealthy => 'Неисправен';

  @override
  String get dockerHealthStarting => 'Проверяется';

  @override
  String get dockerActionStart => 'Запустить';

  @override
  String get dockerActionStop => 'Остановить';

  @override
  String get dockerActionRestart => 'Перезапустить';

  @override
  String get dockerActionPause => 'Приостановить';

  @override
  String get dockerActionUnpause => 'Возобновить';

  @override
  String get dockerActionKill => 'Завершить принудительно';

  @override
  String get dockerActionRemove => 'Удалить';

  @override
  String dockerKillTitle(String name) {
    return 'Принудительно завершить $name?';
  }

  @override
  String get dockerKillHelp => 'Процессы контейнера будут остановлены сразу, без корректного завершения.';

  @override
  String dockerRemoveTitle(String name) {
    return 'Удалить $name?';
  }

  @override
  String get dockerRemoveHelp => 'Контейнер и его записываемый слой будут удалены. Именованные тома сохранятся.';

  @override
  String get dockerLogs => 'Логи';

  @override
  String get dockerTerminal => 'Терминал';

  @override
  String get dockerForwardPort => 'Пробросить порт';

  @override
  String get dockerDetails => 'Сведения';

  @override
  String get dockerCopyId => 'Скопировать ID';

  @override
  String get dockerContainerId => 'ID контейнера';

  @override
  String get dockerOverview => 'Обзор';

  @override
  String get dockerInspectJson => 'JSON';

  @override
  String get dockerCopyJson => 'Скопировать JSON';

  @override
  String get dockerImage => 'Образ';

  @override
  String get dockerStatus => 'Состояние';

  @override
  String get dockerHealth => 'Проверка здоровья';

  @override
  String get dockerCreated => 'Создан';

  @override
  String get dockerStarted => 'Запущен';

  @override
  String get dockerCommand => 'Команда';

  @override
  String get dockerRestartCount => 'Перезапуски';

  @override
  String get dockerComposeProject => 'Проект compose';

  @override
  String get dockerComposeService => 'Сервис compose';

  @override
  String get dockerPorts => 'Порты';

  @override
  String get dockerNetworks => 'Сети';

  @override
  String get dockerMounts => 'Монтирования';

  @override
  String get dockerLabels => 'Метки';

  @override
  String get dockerEnvironment => 'Переменные окружения';

  @override
  String get dockerLogsPause => 'Пауза';

  @override
  String get dockerLogsResume => 'Продолжить';

  @override
  String get dockerLogsFollow => 'Следить';

  @override
  String get dockerLogsTimestamps => 'Время';

  @override
  String get dockerLogsCopy => 'Скопировать строки';

  @override
  String get dockerLogsClear => 'Очистить';

  @override
  String get dockerLogsFind => 'Найти в логах';

  @override
  String dockerLogsMatches(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count совпадения',
      many: '$count совпадений',
      few: '$count совпадения',
      one: '$count совпадение',
      zero: 'Совпадений нет',
    );
    return '$_temp0';
  }

  @override
  String get dockerLogsPaused => 'Пауза: новые строки сохранятся до продолжения';

  @override
  String dockerLogsTrimmed(int limit) {
    return 'Хранятся только последние $limit строк';
  }

  @override
  String get dockerLogsFailed => 'Поток логов прервался с ошибкой';

  @override
  String get dockerLogsWaiting => 'Ожидание строк лога…';

  @override
  String get dockerNoLogs => 'Строк лога нет';

  @override
  String get dockerForwardHelpSsh => 'Локальный порт пробрасывается на порт контейнера через SSH-подключение к хосту.';

  @override
  String get dockerForwardHelpLocal => 'Локальный порт пробрасывается на порт контейнера на этом компьютере.';

  @override
  String get dockerForwardContainerPort => 'Порт контейнера';

  @override
  String get dockerForwardLocalPort => 'Локальный порт';

  @override
  String get dockerForwardLocalHelp => '0 — выбрать свободный порт';

  @override
  String get dockerForwardStart => 'Начать проброс';

  @override
  String get dockerForwardCreated => 'Проброс работает. Он отображается на панели проброса портов.';

  @override
  String get dockerForwardCopyAddress => 'Скопировать адрес';

  @override
  String get dockerForwardStop => 'Остановить проброс';

  @override
  String get dockerForwardRunning => 'Работает';

  @override
  String get dockerForwardFailed => 'Ошибка';

  @override
  String get dockerForwardStopped => 'Остановлен';

  @override
  String dockerForwardTraffic(int connections, String sent, String received) {
    return '$connections подкл. · ↑ $sent · ↓ $received';
  }

  @override
  String get dockerNoForwards => 'Пробросов нет';

  @override
  String get dockerNoForwardsHelp => 'Выберите «Пробросить порт» у работающего контейнера.';

  @override
  String get dockerTerminalTitle => 'Терминал контейнера';

  @override
  String get dockerShell => 'Оболочка';

  @override
  String get dockerAutoShell => 'Определить автоматически (bash, ash или sh)';

  @override
  String get dockerOpenShell => 'Открыть терминал';

  @override
  String get dockerTerminalFailed => 'Не удалось открыть терминал в контейнере.';

  @override
  String get dockerSocketForwardingDisabled =>
      'SSH-сервер не разрешает проброс сокета Docker, а Docker CLI на нём нет. Включите «AllowStreamLocalForwarding yes» в sshd_config или установите docker CLI на хосте.';

  @override
  String get dockerStateFilter => 'Состояние контейнеров';

  @override
  String dockerFilterAll(int count) {
    return 'Все $count';
  }

  @override
  String dockerFilterRunning(int count) {
    return 'Запущенные $count';
  }

  @override
  String dockerFilterStopped(int count) {
    return 'Остановленные $count';
  }

  @override
  String get dockerRevealSecrets => 'Показать секреты';

  @override
  String get dockerHideSecrets => 'Скрыть секреты';

  @override
  String dockerRevealHelp(String name) {
    return 'Введите пароль хранилища, чтобы показать пароли и токены из окружения $name. Скопированные значения сами удалятся из буфера обмена.';
  }

  @override
  String get dockerVaultPassphrase => 'Пароль хранилища';

  @override
  String get dockerWrongPassphrase => 'Неверный пароль хранилища.';

  @override
  String get dockerMountType => 'Тип';

  @override
  String get dockerMountSource => 'Источник';

  @override
  String get dockerMountDestination => 'Путь в контейнере';

  @override
  String get dockerMountMode => 'Режим';

  @override
  String get dockerMountReadOnly => 'ro';

  @override
  String get dockerMountReadWrite => 'rw';

  @override
  String get dockerView => 'Показать';

  @override
  String get dockerVolumes => 'Тома';

  @override
  String get dockerVolumesOther => 'Прочие тома';

  @override
  String dockerVolumeCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count тома',
      many: '$count томов',
      few: '$count тома',
      one: '$count том',
    );
    return '$_temp0';
  }

  @override
  String get dockerVolumeUnused => 'не используется';

  @override
  String get dockerNoVolumes => 'Томов на этом движке нет.';

  @override
  String get workspaceForwardsEmptyHelp =>
      'Пробросьте порт пода Kubernetes или контейнера Docker — активные пробросы появятся здесь.';

  @override
  String shellAiDeviceIdle(String model) {
    return 'ИИ: встроенная модель не запущена (запустится при запросе) · $model';
  }

  @override
  String shellAiDeviceStarting(String model) {
    return 'ИИ: встроенная модель запускается · $model';
  }

  @override
  String shellAiDeviceError(String model) {
    return 'ИИ: встроенная модель не запустилась · $model';
  }

  @override
  String get dbEditorHint => 'Напишите запрос… ⌘↵ — выполнить';

  @override
  String dbRowCount(String count) {
    return 'Строк: $count';
  }

  @override
  String dbRowsAffected(int count) {
    return 'Затронуто: $count';
  }

  @override
  String get dbStatusReady => 'Готово';

  @override
  String dbRunStatementHelp(String shortcut) {
    return 'Выполнить выделенное или оператор под курсором ($shortcut)';
  }

  @override
  String dbRunScriptHelp(String shortcut) {
    return 'Выполнить весь скрипт по очереди ($shortcut)';
  }

  @override
  String get dbFolderTables => 'Таблицы';

  @override
  String get dbFolderViews => 'Представления';

  @override
  String get dbFolderFunctions => 'Функции';

  @override
  String get dbFolderSequences => 'Последовательности';

  @override
  String get dbFolderDictionaries => 'Словари';

  @override
  String get dbFolderColumns => 'Колонки';

  @override
  String get dbFolderIndexes => 'Индексы';

  @override
  String get dbFolderConstraints => 'Ограничения';

  @override
  String get dbFolderTriggers => 'Триггеры';

  @override
  String get dbFolderProjections => 'Проекции';

  @override
  String get dbCollapseAll => 'Свернуть всё';

  @override
  String dockerConnectDatabaseHelp(String engine) {
    return 'Открыть базу $engine этого контейнера в разделе «Базы данных»';
  }

  @override
  String dbRouteKubernetes(String name) {
    return 'Через Kubernetes: $name';
  }

  @override
  String get dbContainerPort => 'Порт контейнера';

  @override
  String get dbKubeTarget => 'Куда пробрасывать';

  @override
  String get dbKubeTargetName => 'Имя объекта';

  @override
  String get dbKubeRouteHelp =>
      'Проброс порта восстанавливается при каждом подключении, а уже запущенный используется повторно. Для Service и Deployment выбирается готовый Pod, поэтому подключение переживает перезапуск подов.';

  @override
  String get dbErrorKubeUnreachable =>
      'Не удаётся подключиться к кластеру Kubernetes, чтобы восстановить проброс порта: проверьте контекст, сеть и вход в кластер.';

  @override
  String get dbErrorKubeTarget =>
      'Pod для проброса порта не найден: проверьте namespace и имя объекта в маршруте подключения.';

  @override
  String get dbErrorKubeDenied => 'Кластер запретил доступ к Pod или к пробросу его порта.';

  @override
  String get dbErrorKubeForward =>
      'Не удалось запустить проброс порта: остановите ненужные пробросы и повторите попытку.';

  @override
  String get dbRouteKubeHint => 'Проброс порта Kubernetes, восстанавливается при подключении';

  @override
  String dbRouteTunnel(String name) {
    return 'Туннель: $name';
  }

  @override
  String get dbRouteDocker => 'Контейнер Docker';

  @override
  String get dbDockerEngine => 'Движок Docker';

  @override
  String get dbDockerContainer => 'Имя контейнера';

  @override
  String get dbDockerRouteHelp =>
      'Проброс порта в контейнер восстанавливается при каждом подключении, а уже запущенный используется повторно. Контейнер ищется по имени, поэтому пересозданный контейнер тоже подойдёт.';

  @override
  String get dbTunnelRouteHelp =>
      'Сохранённый туннель запускается при подключении, если он не запущен. Адрес и порт берутся из цели туннеля.';

  @override
  String dbUseTunnel(String name) {
    return 'Использовать сохранённый туннель «$name»';
  }

  @override
  String get dbErrorTunnel => 'Не удалось запустить сохранённый туннель: проверьте его SSH-хост в разделе «Туннели».';

  @override
  String get dbErrorTunnelKind => 'Подключение к БД возможно только через туннель локального проброса порта.';

  @override
  String get dbErrorDockerUnreachable =>
      'Не удаётся подключиться к Docker, чтобы восстановить проброс порта: проверьте, что движок запущен, и SSH-хост.';

  @override
  String get dbErrorDockerContainer => 'Контейнер не найден или не запущен: проверьте его имя в маршруте подключения.';

  @override
  String get dbErrorDockerForward =>
      'Не удалось запустить проброс порта Docker: остановите ненужные пробросы и повторите попытку.';

  @override
  String get dockerOpenInSftp => 'Открыть в SFTP';

  @override
  String get dockerVolumeInVm =>
      'Том хранится внутри виртуальной машины Docker, поэтому его папку нельзя открыть с этого компьютера.';

  @override
  String get dbRouteTunnelHint => 'Сохранённый SSH-туннель, запускается при подключении';

  @override
  String get dbRouteDockerHint => 'Проброс порта Docker, восстанавливается при подключении';

  @override
  String get dockerComposeActions => 'Действия с проектом';

  @override
  String get dockerComposeStartAll => 'Запустить все';

  @override
  String get dockerComposeStopAll => 'Остановить все';

  @override
  String get dockerComposeRestartAll => 'Перезапустить все';

  @override
  String get dockerComposeViewYaml => 'Показать YAML';

  @override
  String get dockerComposeEditYaml => 'Изменить YAML';

  @override
  String dockerComposeStopAllTitle(String project) {
    return 'Остановить все контейнеры проекта $project?';
  }

  @override
  String dockerComposeRestartAllTitle(String project) {
    return 'Перезапустить все контейнеры проекта $project?';
  }

  @override
  String dockerComposeBulkHelp(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Затронет $count контейнера.',
      many: 'Затронет $count контейнеров.',
      few: 'Затронет $count контейнера.',
      one: 'Затронет $count контейнер.',
    );
    return '$_temp0';
  }

  @override
  String dockerComposeBulkFailed(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Действие не удалось для $count контейнера.',
      many: 'Действие не удалось для $count контейнеров.',
      few: 'Действие не удалось для $count контейнеров.',
      one: 'Действие не удалось для $count контейнера.',
    );
    return '$_temp0';
  }

  @override
  String dockerComposeFilesTitle(String project) {
    return 'Файлы compose: $project';
  }

  @override
  String dockerComposeWorkingDir(String path) {
    return 'Папка проекта: $path';
  }

  @override
  String dockerComposeEnvFiles(String files) {
    return 'Файлы окружения: $files';
  }

  @override
  String get dockerComposeFiles => 'Файлы';

  @override
  String dockerComposeUnlockHelp(String project) {
    return 'Файлы compose могут содержать пароли и определяют, что запущено на хосте. Введите пароль хранилища, чтобы изменить файлы проекта $project.';
  }

  @override
  String get dockerComposeUnlockExpired =>
      'Разрешение на изменение истекло. Введите пароль хранилища ещё раз, чтобы сохранить.';

  @override
  String get dockerComposeUnlock => 'Разрешить изменение';

  @override
  String get dockerComposeSaveApply => 'Сохранить и применить';

  @override
  String get dockerComposeSaveOnly => 'Сохранить без применения';

  @override
  String get dockerComposeValidating => 'Проверка конфигурации на хосте…';

  @override
  String get dockerComposeInvalid => 'В конфигурации есть ошибки. Ничего не сохранено.';

  @override
  String dockerComposeConflict(String path) {
    return 'Файл $path изменился на хосте после открытия. Ничего не сохранено: скопируйте свои изменения и откройте файл заново.';
  }

  @override
  String dockerComposeWriteDenied(String path) {
    return 'Нет прав на запись $path или его папки. Файл не изменён.';
  }

  @override
  String dockerComposeWriteFailed(String path) {
    return 'Не удалось записать $path. Файл не изменён.';
  }

  @override
  String get dockerComposeMissing =>
      'На хосте нет утилиты compose, поэтому изменения нельзя проверить. Ничего не сохранено.';

  @override
  String dockerComposeRunFailed(String detail) {
    return 'Не удалось выполнить проверку ($detail). Ничего не сохранено.';
  }

  @override
  String dockerComposeSavedBackups(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Сохранено. Прежние версии сохранены как:',
      many: 'Сохранено. Прежние версии сохранены как:',
      few: 'Сохранено. Прежние версии сохранены как:',
      one: 'Сохранено. Прежняя версия сохранена как:',
    );
    return '$_temp0';
  }

  @override
  String get dockerComposeSavedNotApplied => 'Запущенные контейнеры пока используют прежнюю конфигурацию.';

  @override
  String get dockerComposeApplying => 'Применение изменений…';

  @override
  String get dockerComposeApplied => 'Изменения применены.';

  @override
  String dockerComposeApplyFailed(int code) {
    return 'Не удалось применить изменения (код выхода $code).';
  }

  @override
  String get dockerComposeApplyError => 'Не удалось применить изменения.';

  @override
  String get dockerComposeOutput => 'Вывод';

  @override
  String get dockerComposeDiscardTitle => 'Отменить изменения?';

  @override
  String get dockerComposeDiscardHelp => 'Изменённые файлы не сохранены.';

  @override
  String get dockerComposeDiscard => 'Не сохранять';

  @override
  String get dockerComposeFileNotFound => 'Этого файла больше нет на хосте.';

  @override
  String get dockerComposeFileDenied =>
      'Нет прав на чтение этого файла: возможно, он принадлежит другому пользователю, например root.';

  @override
  String get dockerComposeFileTooLarge => 'Файл больше 1 МБ и не показывается.';

  @override
  String get dockerComposeFileNotText => 'Файл не является текстом в UTF-8.';

  @override
  String get dockerComposeFileReadFailed => 'Не удалось прочитать файл.';

  @override
  String get dockerComposeProjectGone => 'У проекта больше нет контейнеров.';

  @override
  String get dockerComposeFilesUnknown =>
      'Контейнеры проекта не хранят сведений о том, из каких файлов compose они созданы.';

  @override
  String get sharingDbConnection => 'Подключение к базе данных';

  @override
  String get sharingDbUnsupported => 'Этот сервер не поддерживает общий доступ к подключениям баз данных.';

  @override
  String get sharingDbSnapshotChanged =>
      'Проверьте подключение ещё раз, если его параметры или учётные данные изменились.';

  @override
  String get sharingDbExclude =>
      'По умолчанию передаются только параметры подключения. Пароль, ключ клиента, группа, история запросов и настройки ИИ остаются на этом устройстве.';

  @override
  String get sharingDbIncludeCredentials => 'Передать сохранённый пароль и ключ клиента';

  @override
  String get sharingDbCredentialWarning =>
      'Получатели смогут использовать эти учётные данные для доступа к базе данных. Передавайте их только проверенным устройствам, которым доверяете. Отзыв доступа не отменяет уже скопированные учётные данные: смените пароль на сервере базы данных.';

  @override
  String get sharingDbCredentialsIncluded => 'Пароль или ключ клиента переданы; значения скрыты.';

  @override
  String get sharingDbCredentialsExcluded => 'Пароль и ключ клиента не переданы.';

  @override
  String get sharingDbRouteRelay =>
      'Подключение идёт через SSH-хост, профиль переходов, прокси или туннель, которые не передаются. Получатели выбирают свой маршрут.';

  @override
  String get sharingDbRouteKubernetes =>
      'Подключение пробрасывается из рабочей нагрузки Kubernetes. Получатели выбирают свой контекст Kubernetes; пространство имён и нагрузка сохраняются.';

  @override
  String get sharingDbRouteDocker =>
      'Подключение пробрасывается из контейнера Docker. Получатели выбирают свой движок Docker; имя контейнера сохраняется.';

  @override
  String get sharingDbCopyHelp =>
      'Сохраните личную копию подключения в разделе «Базы данных». Выберите свой маршрут. Пароль и ключ клиента импортируются только по вашему явному выбору.';

  @override
  String get sharingDbRouteRequired =>
      'Это подключение недоступно напрямую. Выберите совместимый маршрут, сохранённый на этом устройстве.';

  @override
  String get sharingDbNoRoute => 'На этом устройстве нет совместимого маршрута. Сначала добавьте его.';

  @override
  String get sharingDbImportCredentials => 'Импортировать переданные пароль и ключ клиента';

  @override
  String get sharingDbRouteKind => 'Маршрут';

  @override
  String get sharingDbRouteKindRelay => 'SSH, профиль переходов, прокси или туннель';

  @override
  String get sharingDbWorkload => 'Рабочая нагрузка';

  @override
  String get sharingDbContainer => 'Контейнер';

  @override
  String get dbSharedTitle => 'Общие подключения';

  @override
  String get dbSharedBadge => 'Общее';

  @override
  String get dbSharedOwned => 'Вы поделились';

  @override
  String get dbSharedOpen => 'Подробнее';

  @override
  String dbSharedCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count общего подключения',
      many: '$count общих подключений',
      few: '$count общих подключения',
      one: '$count общее подключение',
    );
    return '$_temp0';
  }

  @override
  String shellNewDeviceTitle(String device, String platform) {
    return 'Новое устройство вошло в аккаунт: $device, $platform';
  }

  @override
  String shellNewDeviceMessage(String ago) {
    return 'Вход $ago. Если это не вы, отзовите его и смените пароль аккаунта. Первые дни оно не может отзывать ваши доверенные устройства.';
  }

  @override
  String get shellNewDeviceItsMe => 'Это я';

  @override
  String get shellNewDeviceRevoke => 'Отозвать…';

  @override
  String shellNewDeviceRevoked(String device) {
    return '«$device» отозвано. Смените пароль аккаунта: тот, кто вошёл, его знает.';
  }

  @override
  String get devicesStatusUnconfirmed => 'Не подтверждено';

  @override
  String devicesQuarantineBadge(String date) {
    return 'Новое, ограничения до $date';
  }

  @override
  String get errorDeviceInQuarantine =>
      'Это устройство вошло недавно и пока не может отзывать доверенные устройства или завершать их сеансы. Сделайте это на доверенном устройстве.';

  @override
  String errorDeviceInQuarantineUntil(String date) {
    return 'Это устройство вошло недавно и до $date не может отзывать доверенные устройства или завершать их сеансы. Сделайте это на доверенном устройстве.';
  }

  @override
  String get errorEnvelopeRateLimited => 'Слишком много попыток открыть хранилище с новых устройств. Повторите позже.';

  @override
  String errorEnvelopeRateLimitedUntil(String time) {
    return 'Слишком много попыток открыть хранилище с новых устройств. Повторите после $time.';
  }

  @override
  String get settingsSignOutEverywhere => 'Выйти на всех устройствах…';

  @override
  String get settingsSignOutEverywhereTitle => 'Выйти на всех устройствах?';

  @override
  String get settingsSignOutEverywhereMessage =>
      'Все сеансы аккаунта будут завершены, включая этот. Устройства снова войдут с паролем аккаунта; их локальные данные сохранятся.';

  @override
  String get settingsSignOutEverywhereConfirm => 'Выйти везде';

  @override
  String get settingsSignOutEverywhereDone => 'Выполнен выход на всех устройствах.';

  @override
  String settingsSignOutEverywhereKept(int count, String date) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other:
          'Это устройство новое, поэтому $count доверенного устройства не затронуты (до $date). Сеансы новых устройств завершены.',
      many:
          'Это устройство новое, поэтому $count доверенных устройств не затронуты (до $date). Сеансы новых устройств завершены.',
      few:
          'Это устройство новое, поэтому $count доверенных устройства не затронуты (до $date). Сеансы новых устройств завершены.',
      one:
          'Это устройство новое, поэтому $count доверенное устройство не затронуто (до $date). Сеансы новых устройств завершены.',
    );
    return '$_temp0';
  }

  @override
  String get settingsDialogAccountPasswordChangedNewDevice =>
      'Пароль аккаунта изменён. Это устройство новое, поэтому доверенные устройства остались в аккаунте.';

  @override
  String get dockerShowFull => 'Показать полностью';

  @override
  String get dockerSecretsMasked => 'Пароли, токены и ключи скрыты.';

  @override
  String dockerJsonRevealHelp(String name) {
    return 'Введите пароль хранилища, чтобы показать полный JSON inspect контейнера $name, включая пароли и токены. Скопированный текст будет автоматически удалён из буфера обмена.';
  }

  @override
  String dockerComposeRevealHelp(String project) {
    return 'Файлы compose могут содержать пароли. Введите пароль хранилища, чтобы показать файлы проекта $project полностью.';
  }

  @override
  String dockerComposeTampered(String path) {
    return 'Во время сохранения кто-то другой заменил или изменил на хосте временный файл для $path. В $path ничего не записано.';
  }

  @override
  String dockerComposeTamperedRestored(String path, String backup) {
    return 'Во время сохранения кто-то другой подменил $path на хосте. Исходный файл восстановлен из резервной копии $backup.';
  }

  @override
  String dockerComposeTamperedNotRestored(String path, String backup) {
    return 'Во время сохранения кто-то другой подменил $path на хосте, и вернуть исходный файл не удалось. Восстановите его из резервной копии $backup.';
  }

  @override
  String get dockerComposeUnsafeDirTitle => 'Сохранить в общий каталог?';

  @override
  String dockerComposeUnsafeDirHelp(String dirs) {
    return 'Каталог проекта доступен на запись другим пользователям: $dirs. Сохранение может быть небезопасным: они могут подменить временный файл или резервную копию во время записи.';
  }

  @override
  String get dockerComposeSaveAnyway => 'Всё равно сохранить';

  @override
  String get dockerTerminalHint => 'Терминал даёт полный доступ к контейнеру, включая переменные окружения.';

  @override
  String get updateWindowsInstallHelp =>
      'ConsoleCrypt закроется, установщик обновит приложение и запустит его снова. SSH-сеансы будут закрыты, профили и хранилища сохранятся.';

  @override
  String get updatePortableHelp =>
      'Эта копия ConsoleCrypt запущена из распакованного ZIP-архива и не может обновиться сама. Откройте страницу загрузки и скачайте установщик или новый ZIP-архив. Профили и хранилища сохранятся.';

  @override
  String get updateOpenDownloadPage => 'Открыть страницу загрузки';

  @override
  String get updateErrorNetwork =>
      'Не удалось скачать обновление: сервер недоступен или соединение прервалось. Проверьте подключение и повторите попытку.';

  @override
  String get updateErrorIntegrity =>
      'Скачанный файл не совпал с подписанным размером или контрольной суммой и не установлен. Повторите загрузку.';

  @override
  String get updateErrorFeed =>
      'Не удалось проверить сведения об обновлении (подпись, формат или срок действия). Ничего не установлено.';

  @override
  String get updateErrorRollback =>
      'Сервер вернул более старые сведения об обновлениях, чем это устройство уже получало. Возможна подмена; ничего не установлено.';

  @override
  String updateErrorLaunch(String code) {
    return 'Не удалось запустить установщик (код ошибки Windows $code). Возможно, его заблокировал антивирус. Скачайте установщик со страницы загрузки.';
  }

  @override
  String get updateErrorStorage =>
      'Не удалось сохранить обновление во временную папку. Освободите место на диске и повторите попытку.';

  @override
  String updateErrorBrowser(String url) {
    return 'Не удалось открыть браузер. Страница загрузки: $url';
  }

  @override
  String get kubeGitOpsReconcileTooltip =>
      'Flux немедленно сверит выбранные ресурсы с Git и применит изменения (flux reconcile)';

  @override
  String get kubeGitOpsReconcileWithSourceTooltip =>
      'Flux сначала получит свежую версию источника (Git-репозитория или чарта), затем синхронизирует выбранный ресурс (flux reconcile --with-source)';

  @override
  String get kubeGitOpsSuspendTooltip =>
      'Flux перестанет применять изменения из Git к выбранным ресурсам, пока синхронизацию не возобновят (flux suspend)';

  @override
  String get kubeGitOpsResumeTooltip =>
      'Flux снова начнёт применять изменения из Git к выбранным ресурсам и сразу синхронизирует их (flux resume)';

  @override
  String get kubeGitOpsResetTooltip =>
      'Обнулит счётчик неудачных установок и обновлений HelmRelease, чтобы Flux попробовал снова (flux reconcile helmrelease --reset)';

  @override
  String get kubeGitOpsForceTooltip =>
      'Flux однократно выполнит установку или обновление Helm, даже если ничего не изменилось (flux reconcile helmrelease --force)';

  @override
  String get copyWhatDdl => 'DDL';

  @override
  String get copyWhatValue => 'значение';

  @override
  String get dbOpenInEditor => 'Открыть в редакторе';

  @override
  String get dbUnlockProduction => 'Разблокировать прод';

  @override
  String get dbUnlockProductionTitle => 'Разблокировать прод для изменений';

  @override
  String dbUnlockProductionWarning(String name) {
    return 'Изменения пойдут прямо в прод-базу «$name». Каждый изменяющий запрос по-прежнему показывается на проверку и требует пароль хранилища. Запись снова блокируется, когда истечёт время, по кнопке «Заблокировать», при отключении или блокировке хранилища.';
  }

  @override
  String get dbUnlockFor => 'Разрешить запись на';

  @override
  String dbMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count минуты',
      many: '$count минут',
      few: '$count минуты',
      one: '$count минуту',
    );
    return '$_temp0';
  }

  @override
  String dbProdWriteUntil(String time) {
    return 'Запись в прод до $time';
  }

  @override
  String dbProdWriteLeft(String time) {
    return 'Осталось $time';
  }

  @override
  String get dbLockProduction => 'Заблокировать';

  @override
  String get dbProdWriteLocked => 'Запись в прод снова заблокирована.';

  @override
  String get dbProdRoleReadOnly =>
      'Роль этого подключения в базе не может изменять данные, поэтому сервер отклонит запись даже при разблокированном проде.';

  @override
  String get dbHistoryProdWrite => 'Изменение в проде';

  @override
  String get dbSetNull => 'Установить NULL';

  @override
  String get dbCellEditHint => 'Enter — сохранить, Esc — отменить, Tab — следующая ячейка';

  @override
  String get dbPendingEdit => 'Изменено, ещё не применено';

  @override
  String dbSelectedRows(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Выбрано строк: $count',
      many: 'Выбрано строк: $count',
      few: 'Выбрано строк: $count',
      one: 'Выбрано строк: $count',
    );
    return '$_temp0';
  }

  @override
  String get vaultPassphraseConfirmLabel => 'Пароль хранилища';

  @override
  String get vaultPassphraseConfirmWrong => 'Неверный пароль хранилища.';

  @override
  String dbPreviewTabTitle(String name) {
    return '$name — данные';
  }

  @override
  String dbDetailsSection(String id) {
    String _temp0 = intl.Intl.selectLogic(id, {
      'general': 'Общее',
      'size': 'Размер',
      'structure': 'Структура',
      'partitioning': 'Секционирование',
      'maintenance': 'Обслуживание',
      'other': '$id',
    });
    return '$_temp0';
  }

  @override
  String dbDetailsField(String id) {
    String _temp0 = intl.Intl.selectLogic(id, {
      'kind': 'Тип объекта',
      'persistence': 'Хранение',
      'schema': 'Схема',
      'owner': 'Владелец',
      'tablespace': 'Табличное пространство',
      'comment': 'Комментарий',
      'rows': 'Строк',
      'live_rows': 'Живых строк',
      'dead_rows': 'Мёртвых строк',
      'total_size': 'Общий размер',
      'table_size': 'Данные таблицы',
      'index_size': 'Индексы',
      'toast_size': 'TOAST',
      'columns': 'Колонок',
      'indexes': 'Индексов',
      'constraints': 'Ограничений',
      'triggers': 'Триггеров',
      'definition_length': 'Длина определения, символов',
      'partition_key': 'Ключ секционирования',
      'partitions': 'Секций',
      'partition_of': 'Секция таблицы',
      'last_vacuum': 'Последний VACUUM',
      'last_autovacuum': 'Последний autovacuum',
      'last_analyze': 'Последний ANALYZE',
      'last_autoanalyze': 'Последний autoanalyze',
      'engine': 'Движок',
      'row_format': 'Формат строк',
      'collation': 'Сопоставление',
      'definer': 'Определил',
      'updatable': 'Обновляемое',
      'security': 'Права выполнения',
      'average_row': 'Средняя строка',
      'free_space': 'Свободно',
      'auto_increment': 'Следующий AUTO_INCREMENT',
      'created': 'Создана',
      'updated': 'Изменена',
      'uncompressed_size': 'Без сжатия',
      'parts': 'Активных частей',
      'sorting_key': 'Ключ сортировки',
      'primary_key': 'Первичный ключ',
      'sampling_key': 'Ключ выборки',
      'ttl': 'TTL',
      'metadata_modified': 'Метаданные изменены',
      'type': 'Тип',
      'encoding': 'Кодировка',
      'database': 'База',
      'memory': 'Память',
      'keys': 'Ключей',
      'length_bytes': 'Длина, байт',
      'length_fields': 'Полей',
      'length_elements': 'Элементов',
      'length_members': 'Элементов',
      'length_entries': 'Записей',
      'other': '$id',
    });
    return '$_temp0';
  }

  @override
  String dbDetailsKeyword(String id) {
    String _temp0 = intl.Intl.selectLogic(id, {
      'table': 'Таблица',
      'partitioned_table': 'Секционированная таблица',
      'view': 'Представление',
      'materialized_view': 'Материализованное представление',
      'foreign_table': 'Внешняя таблица',
      'unlogged': 'Без журнала (UNLOGGED)',
      'temporary': 'Временная',
      'other': '$id',
    });
    return '$_temp0';
  }

  @override
  String get dbDetailsNoExpiry => 'Без срока';

  @override
  String get dbDetailsJson => 'JSON';

  @override
  String get dbDetailsEmpty => 'Для этого объекта сведений нет.';

  @override
  String get copyWhatObjectDetails => 'Сведения об объекте';

  @override
  String get dbWrapLines => 'Переносить строки';

  @override
  String settingsTerminalFontMissing(String family, String fallback) {
    return 'Шрифт «$family» не установлен — используется $fallback.';
  }

  @override
  String get settingsCaptureProtectionLabel => 'Скрывать окно от записи экрана';

  @override
  String get settingsCaptureProtectionOff => 'Никогда';

  @override
  String get settingsCaptureProtectionSecrets => 'Когда видны секреты';

  @override
  String get settingsCaptureProtectionAlways => 'Всегда';

  @override
  String get settingsCaptureProtectionHelp =>
      'Окно не попадёт в скриншоты, запись и демонстрацию экрана (Zoom, Teams). В режиме «Всегда» коллеги не увидят окно ConsoleCrypt при показе экрана.';

  @override
  String get sftpActiveContentTitle => 'Этот файл может выполнить код';

  @override
  String sftpActiveContentBody(String name) {
    return 'Файл «$name» может выполнить код. Он откроется как текст в редакторе.';
  }

  @override
  String get sftpActiveContentNoRun =>
      'ConsoleCrypt не запускает загруженные файлы. Чтобы выполнить его, используйте терминал на сервере.';

  @override
  String get sftpActiveContentOpenAsText => 'Открыть как текст';

  @override
  String get sftpNoTextEditorTitle => 'Текстовый редактор не найден';

  @override
  String sftpNoTextEditorBody(String name) {
    return 'Файл «$name» может выполнить код, поэтому он открывается только как текст, но текстовый редактор не найден. Выберите редактор.';
  }

  @override
  String get sftpChooseEditor => 'Выбрать редактор…';

  @override
  String get errorNoTextEditor =>
      'Не найден текстовый редактор, чтобы безопасно открыть этот файл. Выберите редактор через «Открыть в программе…».';

  @override
  String get dbExportEscapeFormulas => 'Экранировать формулы (для Excel)';

  @override
  String settingsClearClipboardSeconds(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count секунды',
      many: '$count секунд',
      few: '$count секунды',
      one: '$count секунду',
    );
    return 'Через $_temp0 после копирования секрета';
  }

  @override
  String settingsClearClipboardMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count минуты',
      many: '$count минут',
      few: '$count минуты',
      one: '$count минуту',
    );
    return 'Через $_temp0 после копирования секрета';
  }

  @override
  String settingsClearClipboardHours(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count часа',
      many: '$count часов',
      few: '$count часа',
      one: '$count час',
    );
    return 'Через $_temp0 после копирования секрета';
  }

  @override
  String settingsClearClipboardDays(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count дня',
      many: '$count дней',
      few: '$count дня',
      one: '$count день',
    );
    return 'Через $_temp0 после копирования секрета';
  }

  @override
  String sharingUnreadableEntries(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count записи не удалось прочитать, они скрыты.',
      many: '$count записей не удалось прочитать, они скрыты.',
      few: '$count записи не удалось прочитать, они скрыты.',
      one: '$count запись не удалось прочитать, она скрыта.',
    );
    return '$_temp0';
  }

  @override
  String get snippetPackPostgresql => 'SQL · PostgreSQL';

  @override
  String get snippetPackPostgresqlDescription =>
      'Активность, блокировки, размеры, индексы, vacuum, репликация и настройки. Для psql или раздела «Базы данных».';

  @override
  String get snippetPackMysql => 'SQL · MySQL/MariaDB';

  @override
  String get snippetPackMysqlDescription =>
      'Processlist, блокировки InnoDB, размеры, индексы, репликация и дайджесты запросов. Для клиента mysql или раздела «Базы данных».';

  @override
  String get snippetPackClickhouse => 'SQL · ClickHouse';

  @override
  String get snippetPackClickhouseDescription =>
      'Текущие и медленные запросы, куски, слияния, мутации, репликация и диски. Для clickhouse-client или раздела «Базы данных».';

  @override
  String get snippetPackRedis => 'Redis';

  @override
  String get snippetPackRedisDescription =>
      'Память, клиенты, медленные команды, ключи и репликация. Для redis-cli или раздела «Базы данных».';

  @override
  String get snippetPackMongodb => 'MongoDB';

  @override
  String get snippetPackMongodbDescription =>
      'Операции, статистика сервера и коллекций, индексы, набор реплик и профилировщик. Для mongosh; часть работает и в разделе «Базы данных».';

  @override
  String get snippetDbPgActivity => 'Активные запросы';

  @override
  String get snippetDbPgActivityNote => 'Сессии не в состоянии idle, самые долгие сверху.';

  @override
  String get snippetDbPgLongRunning => 'Долгие запросы';

  @override
  String get snippetDbPgLongRunningNote => 'Запросы, которые выполняются дольше заданного числа секунд.';

  @override
  String get snippetDbPgBlocking => 'Дерево блокировок';

  @override
  String get snippetDbPgBlockingNote => 'Блокирующие сессии и ожидающие их сессии с отступом по глубине (PG 9.6+).';

  @override
  String get snippetDbPgTableSizes => 'Самые большие таблицы';

  @override
  String get snippetDbPgTableSizesNote =>
      'Полный размер, а также таблица, индексы и TOAST; число строк — оценка планировщика.';

  @override
  String get snippetDbPgDatabaseSizes => 'Размеры баз данных';

  @override
  String get snippetDbPgDatabaseSizesNote => 'Размер каждой базы, к которой разрешено подключение.';

  @override
  String get snippetDbPgIndexUsage => 'Использование индексов';

  @override
  String get snippetDbPgIndexUsageNote =>
      'Сначала редко используемые индексы; idx_scan = 0 — не использовался с последнего сброса статистики. Проверьте и реплики; уникальные индексы не удаляйте.';

  @override
  String get snippetDbPgDeadTuples => 'Мёртвые строки и autovacuum';

  @override
  String get snippetDbPgDeadTuplesNote => 'Оценка раздувания по мёртвым строкам и время последних vacuum и analyze.';

  @override
  String get snippetDbPgCacheHit => 'Попадания в кэш';

  @override
  String get snippetDbPgCacheHitNote => 'Доля блоков из shared buffers по базам; ниже 99% для OLTP стоит разобраться.';

  @override
  String get snippetDbPgReplication => 'Состояние репликации';

  @override
  String get snippetDbPgReplicationNote =>
      'Подключённые реплики и отставание в байтах и по времени. Выполняйте на primary (PG 10+).';

  @override
  String get snippetDbPgSlots => 'Слоты репликации';

  @override
  String get snippetDbPgSlotsNote => 'Слоты и удерживаемый ими WAL; неактивный слот держит WAL бесконечно (PG 13+).';

  @override
  String get snippetDbPgConnections => 'Подключения по пользователям и приложениям';

  @override
  String get snippetDbPgConnectionsNote => 'Клиентские подключения по базе, пользователю, приложению и состоянию.';

  @override
  String get snippetDbPgSettingsChanged => 'Изменённые настройки';

  @override
  String get snippetDbPgSettingsChangedNote => 'Параметры, значение которых отличается от встроенного по умолчанию.';

  @override
  String get snippetDbPgColumns => 'Колонки таблицы';

  @override
  String get snippetDbPgColumnsNote => 'Колонки с типами, значениями по умолчанию и комментариями.';

  @override
  String get snippetDbPgForeignKeys => 'Внешние ключи таблицы';

  @override
  String get snippetDbPgForeignKeysNote => 'Внешние ключи таблицы и ключи, которые ссылаются на неё.';

  @override
  String get snippetDbPgSequences => 'Последовательности у предела';

  @override
  String get snippetDbPgSequencesNote =>
      'Последовательности, израсходовавшие больше заданного процента диапазона; проверьте и тип колонки (PG 10+).';

  @override
  String get snippetDbPgFkWithoutIndex => 'Внешние ключи без индекса';

  @override
  String get snippetDbPgFkWithoutIndexNote =>
      'Внешние ключи, колонки которых не стоят в начале ни одного индекса; удаление и изменение родителя сканирует дочернюю таблицу.';

  @override
  String get snippetDbPgStatements => 'Тяжёлые запросы (pg_stat_statements)';

  @override
  String get snippetDbPgStatementsNote =>
      'Нужно расширение pg_stat_statements (shared_preload_libraries и CREATE EXTENSION); без него будет ошибка «relation does not exist». Имена колонок — для PG 13+.';

  @override
  String get snippetDbPgExplainAnalyze => 'EXPLAIN ANALYZE';

  @override
  String get snippetDbPgExplainAnalyzeNote =>
      'Выполняет запрос, чтобы измерить реальный план; изменяющие данные запросы оборачивайте в BEGIN … ROLLBACK.';

  @override
  String get snippetDbPgCancel => 'Отменить запрос';

  @override
  String get snippetDbPgCancelNote => 'Отменяет текущий запрос процесса; сессия остаётся подключённой.';

  @override
  String get snippetDbPgTerminate => 'Завершить сессию';

  @override
  String get snippetDbPgTerminateNote => 'Отключает процесс и откатывает его открытую транзакцию.';

  @override
  String get snippetDbMysqlProcesslist => 'Активные запросы';

  @override
  String get snippetDbMysqlProcesslistNote => 'Подключения не в состоянии Sleep, самые долгие сверху.';

  @override
  String get snippetDbMysqlLongRunning => 'Долгие запросы';

  @override
  String get snippetDbMysqlLongRunningNote => 'Запросы, которые выполняются дольше заданного числа секунд.';

  @override
  String get snippetDbMysqlLockWaits => 'Ожидания блокировок InnoDB';

  @override
  String get snippetDbMysqlLockWaitsNote =>
      'Кто кого ждёт и блокирующий запрос. Нужна схема sys (MySQL 5.7+, MariaDB 10.6+).';

  @override
  String get snippetDbMysqlTransactions => 'Открытые транзакции';

  @override
  String get snippetDbMysqlTransactionsNote =>
      'Транзакции InnoDB, самые старые сверху, с числом заблокированных и изменённых строк.';

  @override
  String get snippetDbMysqlTableSizes => 'Самые большие таблицы';

  @override
  String get snippetDbMysqlTableSizesNote =>
      'Данные, индексы и свободное место в МБ; статистика information_schema приблизительная и кэшируется.';

  @override
  String get snippetDbMysqlDatabaseSizes => 'Размеры баз данных';

  @override
  String get snippetDbMysqlDatabaseSizesNote => 'Суммарный размер данных и индексов по схемам в МБ.';

  @override
  String get snippetDbMysqlIndexUsage => 'Использование индексов';

  @override
  String get snippetDbMysqlIndexUsageNote =>
      'Чтения и записи по индексам с момента запуска сервера (performance_schema).';

  @override
  String get snippetDbMysqlUnusedIndexes => 'Неиспользуемые индексы';

  @override
  String get snippetDbMysqlUnusedIndexesNote =>
      'Индексы без чтений с момента запуска сервера. Нужна схема sys; перед удалением проверьте реплики.';

  @override
  String get snippetDbMysqlAutoIncrement => 'AUTO_INCREMENT у предела';

  @override
  String get snippetDbMysqlAutoIncrementNote => 'Какая доля диапазона типа колонки уже израсходована. Нужна схема sys.';

  @override
  String get snippetDbMysqlIndexes => 'Индексы таблицы';

  @override
  String get snippetDbMysqlIndexesNote => 'Колонки индексов по порядку и их кардинальность.';

  @override
  String get snippetDbMysqlColumns => 'Колонки таблицы';

  @override
  String get snippetDbMysqlColumnsNote => 'Колонки с типами, ключами, значениями по умолчанию и комментариями.';

  @override
  String get snippetDbMysqlForeignKeys => 'Внешние ключи таблицы';

  @override
  String get snippetDbMysqlForeignKeysNote => 'Внешние ключи таблицы и ключи, которые ссылаются на неё.';

  @override
  String get snippetDbMysqlReplicaStatus => 'Состояние реплики';

  @override
  String get snippetDbMysqlReplicaStatusNote =>
      'MySQL 8.0.22+ и MariaDB 10.5.1+; на старых серверах — SHOW SLAVE STATUS.';

  @override
  String get snippetDbMysqlVariables => 'Переменные сервера';

  @override
  String get snippetDbMysqlVariablesNote => 'Глобальные переменные по шаблону LIKE.';

  @override
  String get snippetDbMysqlStatus => 'Основные счётчики';

  @override
  String get snippetDbMysqlStatusNote =>
      'Время работы, подключения, медленные запросы, ожидания блокировок строк и чтения buffer pool.';

  @override
  String get snippetDbMysqlConnections => 'Подключения по пользователям и хостам';

  @override
  String get snippetDbMysqlConnectionsNote => 'Подключения по пользователю, хосту клиента, схеме и команде.';

  @override
  String get snippetDbMysqlDigest => 'Тяжёлые запросы (дайджесты)';

  @override
  String get snippetDbMysqlDigestNote =>
      'Нормализованные запросы по суммарному времени из performance_schema (в MySQL включена по умолчанию, в MariaDB — выключена).';

  @override
  String get snippetDbMysqlInnodbStatus => 'Состояние InnoDB';

  @override
  String get snippetDbMysqlInnodbStatusNote =>
      'Отчёт движка: последний дедлок, семафоры, buffer pool; нужна привилегия PROCESS.';

  @override
  String get snippetDbMysqlExplainAnalyze => 'EXPLAIN ANALYZE';

  @override
  String get snippetDbMysqlExplainAnalyzeNote =>
      'Выполняет запрос, чтобы измерить реальный план (MySQL 8.0.18+; в MariaDB — ANALYZE SELECT …).';

  @override
  String get snippetDbMysqlKillQuery => 'Отменить запрос';

  @override
  String get snippetDbMysqlKillQueryNote => 'Останавливает текущий запрос подключения; само подключение остаётся.';

  @override
  String get snippetDbClickhouseProcesses => 'Выполняющиеся запросы';

  @override
  String get snippetDbClickhouseProcessesNote =>
      'Исходные запросы со временем выполнения, памятью и прочитанными строками.';

  @override
  String get snippetDbClickhouseLongRunning => 'Долгие запросы';

  @override
  String get snippetDbClickhouseLongRunningNote => 'Запросы, которые выполняются дольше заданного числа секунд.';

  @override
  String get snippetDbClickhouseSlowQueries => 'Медленные запросы';

  @override
  String get snippetDbClickhouseSlowQueriesNote =>
      'Самые медленные завершённые запросы из system.query_log за последние часы.';

  @override
  String get snippetDbClickhouseFailedQueries => 'Запросы с ошибками';

  @override
  String get snippetDbClickhouseFailedQueriesNote => 'Последние запросы, завершившиеся исключением.';

  @override
  String get snippetDbClickhouseQueryPatterns => 'Самые тяжёлые шаблоны запросов';

  @override
  String get snippetDbClickhouseQueryPatternsNote =>
      'Завершённые запросы, сгруппированные по нормализованному тексту, по суммарному времени.';

  @override
  String get snippetDbClickhouseTableSizes => 'Самые большие таблицы';

  @override
  String get snippetDbClickhouseTableSizesNote => 'Сжатый и несжатый размер активных кусков и степень сжатия.';

  @override
  String get snippetDbClickhouseDatabaseSizes => 'Размеры баз данных';

  @override
  String get snippetDbClickhouseDatabaseSizesNote => 'Место на диске под активные куски по базам.';

  @override
  String get snippetDbClickhousePartitions => 'Партиции таблицы';

  @override
  String get snippetDbClickhousePartitionsNote => 'Куски, строки и размер по партициям.';

  @override
  String get snippetDbClickhousePartsCount => 'Партиции с большим числом кусков';

  @override
  String get snippetDbClickhousePartsCountNote =>
      'Слишком много активных кусков замедляет вставки и чтение («Too many parts»).';

  @override
  String get snippetDbClickhouseColumnSizes => 'Размеры колонок таблицы';

  @override
  String get snippetDbClickhouseColumnSizesNote => 'Сжатый размер, степень сжатия и кодек по колонкам.';

  @override
  String get snippetDbClickhouseMerges => 'Текущие слияния';

  @override
  String get snippetDbClickhouseMergesNote => 'Выполняющиеся слияния и мутации.';

  @override
  String get snippetDbClickhouseMutations => 'Незавершённые мутации';

  @override
  String get snippetDbClickhouseMutationsNote => 'Ещё выполняющиеся ALTER … UPDATE/DELETE и причина последней ошибки.';

  @override
  String get snippetDbClickhouseReplicationQueue => 'Очередь репликации';

  @override
  String get snippetDbClickhouseReplicationQueueNote =>
      'Самые старые задачи репликации с повторами и последней ошибкой (таблицы Replicated*).';

  @override
  String get snippetDbClickhouseReplicas => 'Состояние реплик';

  @override
  String get snippetDbClickhouseReplicasNote => 'Задержка, очередь и режим только чтения у таблиц Replicated*.';

  @override
  String get snippetDbClickhouseDisks => 'Диски';

  @override
  String get snippetDbClickhouseDisksNote => 'Свободное и общее место на настроенных дисках.';

  @override
  String get snippetDbClickhouseErrors => 'Ошибки сервера';

  @override
  String get snippetDbClickhouseErrorsNote => 'Счётчики ошибок с момента запуска и последнее сообщение.';

  @override
  String get snippetDbClickhouseSettingsChanged => 'Изменённые настройки';

  @override
  String get snippetDbClickhouseSettingsChangedNote => 'Настройки сессии, отличающиеся от значений по умолчанию.';

  @override
  String get snippetDbClickhouseMetrics => 'Текущие метрики';

  @override
  String get snippetDbClickhouseMetricsNote => 'Ненулевые текущие показатели: запросы, слияния, подключения, память.';

  @override
  String get snippetDbClickhouseExplain => 'EXPLAIN с индексами';

  @override
  String get snippetDbClickhouseExplainNote =>
      'План с использованными первичным ключом и индексами пропуска; запрос не выполняется.';

  @override
  String get snippetDbClickhouseKillQuery => 'Отменить запрос';

  @override
  String get snippetDbClickhouseKillQueryNote => 'Останавливает запрос с указанным query_id.';

  @override
  String get snippetDbRedisInfo => 'Сведения о сервере';

  @override
  String get snippetDbRedisInfoNote =>
      'Один раздел INFO: server, clients, memory, persistence, stats, replication, cpu, keyspace.';

  @override
  String get snippetDbRedisMemory => 'Память';

  @override
  String get snippetDbRedisMemoryNote => 'Занятая и пиковая память, фрагментация и политика maxmemory.';

  @override
  String get snippetDbRedisMemoryDoctor => 'Диагностика памяти';

  @override
  String get snippetDbRedisMemoryDoctorNote => 'Советы Redis по проблемам с памятью.';

  @override
  String get snippetDbRedisSlowlog => 'Журнал медленных команд';

  @override
  String get snippetDbRedisSlowlogNote => 'Последние медленные команды с длительностью в микросекундах.';

  @override
  String get snippetDbRedisClients => 'Список клиентов';

  @override
  String get snippetDbRedisClientsNote => 'Подключённые клиенты: адрес, возраст, простой и последняя команда.';

  @override
  String get snippetDbRedisDbsize => 'Число ключей';

  @override
  String get snippetDbRedisDbsizeNote => 'Ключи в выбранной базе.';

  @override
  String get snippetDbRedisKeyspace => 'Пространство ключей';

  @override
  String get snippetDbRedisKeyspaceNote => 'Число ключей и ключей с TTL по базам.';

  @override
  String get snippetDbRedisScan => 'Поиск ключей по шаблону';

  @override
  String get snippetDbRedisScanNote =>
      'Пошаговый SCAN вместо KEYS; повторяйте с возвращённым курсором, пока он не станет 0.';

  @override
  String get snippetDbRedisType => 'Тип ключа';

  @override
  String get snippetDbRedisTypeNote => 'Тип данных, хранящихся в ключе.';

  @override
  String get snippetDbRedisTtl => 'TTL ключа';

  @override
  String get snippetDbRedisTtlNote => 'Секунд до истечения; -1 — без срока, -2 — ключа нет.';

  @override
  String get snippetDbRedisMemoryUsage => 'Память ключа';

  @override
  String get snippetDbRedisMemoryUsageNote => 'Сколько байт занимают ключ и значение.';

  @override
  String get snippetDbRedisLatency => 'Диагностика задержек';

  @override
  String get snippetDbRedisLatencyNote => 'Отчёт о задержках; нужен latency-monitor-threshold > 0.';

  @override
  String get snippetDbRedisConfig => 'Параметр конфигурации';

  @override
  String get snippetDbRedisConfigNote => 'Параметры конфигурации по glob-шаблону.';

  @override
  String get snippetDbRedisReplication => 'Репликация';

  @override
  String get snippetDbRedisReplicationNote => 'Роль, подключённые реплики и смещения репликации.';

  @override
  String get snippetDbRedisCommandstats => 'Статистика команд';

  @override
  String get snippetDbRedisCommandstatsNote => 'Вызовы и время по командам с последнего сброса.';

  @override
  String get snippetDbMongodbCurrentOps => 'Активные операции';

  @override
  String get snippetDbMongodbCurrentOpsNote => 'Операции, выполняющиеся сейчас (mongosh).';

  @override
  String get snippetDbMongodbLongOps => 'Долгие операции';

  @override
  String get snippetDbMongodbLongOpsNote => 'Операции, которые выполняются дольше заданного числа секунд (mongosh).';

  @override
  String get snippetDbMongodbConnections => 'Подключения';

  @override
  String get snippetDbMongodbConnectionsNote => 'Текущие, доступные и всего созданные подключения (mongosh).';

  @override
  String get snippetDbMongodbOpcounters => 'Счётчики операций';

  @override
  String get snippetDbMongodbOpcountersNote =>
      'Вставки, запросы, обновления, удаления и команды с момента запуска (mongosh).';

  @override
  String get snippetDbMongodbMemory => 'Память';

  @override
  String get snippetDbMongodbMemoryNote => 'Резидентная и виртуальная память в МБ (mongosh).';

  @override
  String get snippetDbMongodbDbStats => 'Статистика базы';

  @override
  String get snippetDbMongodbDbStatsNote => 'Коллекции, документы, размер данных и индексов в МБ.';

  @override
  String get snippetDbMongodbCollectionStats => 'Статистика коллекции';

  @override
  String get snippetDbMongodbCollectionStatsNote => 'Документы, место на диске и размер каждого индекса в МБ.';

  @override
  String get snippetDbMongodbIndexes => 'Индексы коллекции';

  @override
  String get snippetDbMongodbIndexesNote => 'Ключи и параметры индексов.';

  @override
  String get snippetDbMongodbIndexStats => 'Использование индексов';

  @override
  String get snippetDbMongodbIndexStatsNote => 'Число операций по каждому индексу с последнего перезапуска (mongosh).';

  @override
  String get snippetDbMongodbExplain => 'План запроса со статистикой';

  @override
  String get snippetDbMongodbExplainNote =>
      'Выполняет find и показывает план, просмотренные ключи и документы (mongosh).';

  @override
  String get snippetDbMongodbRsStatus => 'Состояние набора реплик';

  @override
  String get snippetDbMongodbRsStatusNote => 'Участники набора, их состояние и здоровье (mongosh).';

  @override
  String get snippetDbMongodbRsLag => 'Отставание репликации';

  @override
  String get snippetDbMongodbRsLagNote => 'Насколько каждая secondary отстаёт от primary (mongosh, MongoDB 4.4+).';

  @override
  String get snippetDbMongodbProfilerStatus => 'Уровень профилировщика';

  @override
  String get snippetDbMongodbProfilerStatusNote => 'Уровень профилировщика и порог slowms текущей базы (mongosh).';

  @override
  String get snippetDbMongodbSlowOps => 'Медленные операции (профилировщик)';

  @override
  String get snippetDbMongodbSlowOpsNote =>
      'Последние операции из профилировщика медленнее порога; включение — db.setProfilingLevel(1).';

  @override
  String get snippetDbMongodbKillOp => 'Прервать операцию';

  @override
  String get snippetDbMongodbKillOpNote => 'Останавливает операцию с указанным opid из currentOp (mongosh).';

  @override
  String snippetStarterDbOrigin(String note, String client, String package) {
    return '$note Для $client или раздела «Базы данных». Из базового набора «$package».';
  }

  @override
  String snippetStarterDbTerminalOrigin(String note, String client, String package) {
    return '$note Выполняется в $client в терминале; раздел «Базы данных» его не разрешает. Из базового набора «$package».';
  }

  @override
  String get snippetClientSessionRequired =>
      'Сначала запустите клиент базы данных (psql, mysql, clickhouse-client, redis-cli, mongosh) в подключённой вкладке терминала, затем выполните сниппет в ней.';

  @override
  String get dbSnippetPickerTitle => 'Вставить сниппет';

  @override
  String dbSnippetPickerSearch(String engine) {
    return 'Поиск сниппетов $engine';
  }

  @override
  String dbSnippetPickerEmpty(String engine) {
    return 'Сниппетов для $engine пока нет. Добавьте их в разделе «Сниппеты» → «Базовые наборы» или сохраните запрос из истории.';
  }

  @override
  String get sharingLeave => 'Выйти из общего доступа';

  @override
  String get sharingLeaveHelp =>
      'Элемент перестанет приходить на все ваши устройства и исчезнет из списка. Сохранённые личные копии останутся. Владелец увидит, что вы вышли.';

  @override
  String get sharingLeft => 'Вы вышли из общего доступа';

  @override
  String get sharingStopRestoreHelp =>
      'Получатели сразу потеряют доступ. В течение 30 дней элемент можно восстановить в разделе «Недавно удалённые».';

  @override
  String sharingMembersLeft(int count) {
    return 'Вышли из общего доступа: $count';
  }

  @override
  String get sharingRecentlyDeleted => 'Недавно удалённые';

  @override
  String get sharingRecentlyDeletedHelp =>
      'Удалённые общие элементы можно восстановить в течение 30 дней. Восстановленный элемент снова передаётся тем же людям, кроме вышедших; они принимают его заново.';

  @override
  String get sharingRecentlyDeletedEmpty => 'Нет недавно удалённых элементов';

  @override
  String get sharingRestore => 'Восстановить';

  @override
  String get sharingRestored => 'Элемент восстановлен и снова передан';

  @override
  String sharingRestoreUntil(String date, int count) {
    return 'Можно восстановить до $date · получателей: $count';
  }

  @override
  String loginSsoButton(String name) {
    return 'Войти через $name';
  }

  @override
  String get loginSsoButtonDefault => 'Войти через SSO';

  @override
  String get loginSsoEmailLabel => 'E-mail (необязательно)';

  @override
  String get loginSsoEmailHelper => 'Подставится на странице входа вашей организации.';

  @override
  String get loginSsoWaiting => 'Завершите вход в браузере…';

  @override
  String get loginSsoWaitingHelp =>
      'В браузере открыта страница входа вашей организации. Вернитесь сюда, когда она об этом попросит.';

  @override
  String get loginLdapLoginLabel => 'Логин (email или DOMAIN\\user)';

  @override
  String get loginLdapPasswordLabel => 'Пароль';

  @override
  String get loginLdapCaption => 'Учётная запись вашей организации';

  @override
  String get loginLdapLoginRequired => 'Введите логин';

  @override
  String get corporateAuthNoticeTitle => 'Корпоративный вход';

  @override
  String corporateAuthNotice(String name) {
    return 'Вход через корпоративный сервер авторизации $name. Пароль организации не открывает хранилище — для этого нужен пароль хранилища.';
  }

  @override
  String get authProviderOidc => 'OIDC';

  @override
  String get authProviderLdap => 'Active Directory';

  @override
  String get authProviderSsoFallback => 'SSO';

  @override
  String get serverNeedsNewerApp =>
      'Сервер использует способ входа, который эта версия не поддерживает. Обновите ConsoleCrypt.';

  @override
  String get signInMethodEmail => 'email и пароль';

  @override
  String signInMethodOidc(String name) {
    return '$name (OIDC)';
  }

  @override
  String signInMethodLdap(String name) {
    return '$name (Active Directory)';
  }

  @override
  String signInLastTime(String method, String login) {
    return 'В прошлый раз вы входили через $method как $login';
  }

  @override
  String signInLastTimeNoLogin(String method) {
    return 'В прошлый раз вы входили через $method';
  }

  @override
  String get signInForget => 'Забыть';

  @override
  String authModeChangedToEnterprise(String name) {
    return 'Сервер перешёл на вход через корпоративный сервер авторизации $name. Прежний пароль аккаунта больше не используется.';
  }

  @override
  String get authModeChangedToStandard =>
      'Сервер перешёл с корпоративного входа на вход по email и паролю аккаунта. Используйте пароль, который администратор задал для вашей учётной записи.';

  @override
  String get recentServersLabel => 'Недавние серверы';

  @override
  String get authBadgeEmail => 'Email';

  @override
  String authBadgeSso(String name) {
    return 'SSO: $name';
  }

  @override
  String get authBadgeAd => 'AD';

  @override
  String get settingsAuthLabel => 'Авторизация';

  @override
  String settingsAuthCorporate(String name, String kind) {
    return 'Корпоративный сервер — $name ($kind)';
  }

  @override
  String get settingsAuthStandard => 'Email и пароль аккаунта';

  @override
  String get settingsAuthCorporateHelp =>
      'Кто входит, проверяет сервер авторизации вашей организации. Доступа к хранилищу он не получает: пароль хранилища остаётся на ваших устройствах.';

  @override
  String settingsAuthSignedIn(String method, String date) {
    return 'Это устройство вошло через $method $date';
  }

  @override
  String settingsAuthSignedInAs(String method, String login, String date) {
    return 'Это устройство вошло через $method как $login $date';
  }

  @override
  String devicesAuthCorporate(String name) {
    return 'Аккаунты на этом сервере входят через корпоративный сервер авторизации $name.';
  }

  @override
  String get accountDisabledTitle => 'Доступ отключён администратором';

  @override
  String accountDisabledBody(String server) {
    return 'Ваша организация отключила эту учётную запись на сервере $server. Синхронизация остановлена, профиль заблокирован. Данные на этом устройстве остаются зашифрованными.';
  }

  @override
  String get accountDisabledSignIn => 'Войти снова';

  @override
  String get accountDisabledDelete => 'Удалить профиль с этого устройства';

  @override
  String accountDisabledDeleteTitle(String name) {
    return 'Удалить профиль «$name»?';
  }

  @override
  String get accountDisabledDeleteMessage =>
      'Зашифрованная локальная копия будет удалена с этого устройства. На сервере ничего не изменится.';

  @override
  String get errorAccountDisabled => 'Доступ отключён администратором.';

  @override
  String get errorEnterpriseAuthRequired =>
      'Этот сервер принимает только вход вашей организации. Проверьте сервер ещё раз или обновите приложение.';

  @override
  String get errorIdentityConflict =>
      'Учётная запись организации уже связана с другим аккаунтом на этом сервере. Обратитесь к администратору.';

  @override
  String get errorDomainNotAllowed => 'Домен вашей почты не допущен на этом сервере.';

  @override
  String get errorGroupRequired =>
      'Ваша учётная запись не входит в группу, которой разрешён этот сервер. Обратитесь к администратору.';

  @override
  String get errorMfaRequired => 'Ваша организация требует вход со вторым фактором. Войдите снова с его помощью.';

  @override
  String get errorIdpUnavailable => 'Служба входа вашей организации недоступна. Повторите попытку позже.';

  @override
  String get errorReasonEnterpriseCallbackInvalid => 'Браузер вернул неожиданный ответ. Начните вход заново.';

  @override
  String get errorReasonEnterpriseSignInTimeout => 'Вход занял слишком много времени. Начните заново.';

  @override
  String get errorReasonEnterpriseBrowserUnavailable => 'Не удалось открыть браузер для входа.';

  @override
  String get teamsTitle => 'Команды';

  @override
  String get teamsSubtitle =>
      'Ресурсы, закреплённые за командой. Их видят все участники; роль определяет, кто может их менять.';

  @override
  String get teamsEmpty => 'Вы пока не состоите ни в одной команде.';

  @override
  String get teamsCreate => 'Создать команду';

  @override
  String get teamsRefresh => 'Обновить';

  @override
  String get teamsUnnamed => 'Команда без названия';

  @override
  String get teamsPendingConfirmation => 'Ждём, когда администратор сверит код';

  @override
  String get teamsUnavailable => 'Сейчас недоступна';

  @override
  String teamsRowMeta(String role, int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count участника',
      many: '$count участников',
      few: '$count участника',
      one: '$count участник',
    );
    return '$role · $_temp0';
  }

  @override
  String get teamsWarningBadge => 'На паузе';

  @override
  String get teamsRotationBadge => 'Смените ключ';

  @override
  String get teamsOfflineBadge => 'Офлайн';

  @override
  String get teamsReadOnly => 'Только чтение';

  @override
  String teamsChipSemantics(String name) {
    return 'Команда $name';
  }

  @override
  String teamsInvitationFrom(String email, String role) {
    return '$email приглашает вас в команду с ролью «$role».';
  }

  @override
  String teamsInvitationNote(String note) {
    return 'Комментарий: $note';
  }

  @override
  String get teamsAccept => 'Принять';

  @override
  String get teamsDecline => 'Отклонить';

  @override
  String get teamsCreateTitle => 'Новая команда';

  @override
  String get teamsCreateBody =>
      'Название, цвет и всё, что хранит команда, зашифровано сквозным шифрованием. Сервер видит только состав участников.';

  @override
  String get teamsNameLabel => 'Название команды';

  @override
  String get teamsDescriptionLabel => 'Описание (необязательно)';

  @override
  String get teamsColorLabel => 'Цвет';

  @override
  String get teamsColorNone => 'Цвет команды';

  @override
  String get teamsColorCustom => 'Свой…';

  @override
  String teamsCreated(String name) {
    return 'Команда «$name» создана';
  }

  @override
  String get teamsJoinCodeTitle => 'Сверьте код';

  @override
  String teamsJoinCodeBody(String email) {
    return 'Продиктуйте этот код $email (лично, по телефону или в мессенджере). У администратора на экране тот же код — он подтвердит совпадение.';
  }

  @override
  String get teamsJoinCodeWait => 'Команда откроется на ваших устройствах, как только администратор подтвердит.';

  @override
  String teamsJoinRequest(String email, String role) {
    return '$email принял(а) приглашение (роль «$role»). Сверьте код, прежде чем добавлять.';
  }

  @override
  String get teamsCompareCode => 'Сверить код';

  @override
  String teamsCompareTitle(String email) {
    return 'Добавить $email?';
  }

  @override
  String teamsCompareRole(String role) {
    return 'Роль: $role';
  }

  @override
  String teamsCompareBody(String email) {
    return 'Попросите $email продиктовать код со своего экрана. Добавляйте, только если совпадают все группы.';
  }

  @override
  String teamsCodesMatchCheckbox(String email) {
    return 'Код на экране $email совпадает';
  }

  @override
  String get teamsCompareWarning =>
      'Другой код означает, что приглашение могут перехватывать. Не добавляйте участника.';

  @override
  String get teamsCodesDontMatch => 'Коды не совпадают';

  @override
  String get teamsAddMember => 'Добавить в команду';

  @override
  String teamsMemberAdded(String email) {
    return '$email добавлен(а) в команду';
  }

  @override
  String get teamsCodeRejectedTitle => 'Вступление отклонено';

  @override
  String get teamsCodeRejectedBody =>
      'Никто не добавлен. Пригласите человека снова и сверьте код по каналу, которому доверяете.';

  @override
  String get teamsTabMembers => 'Участники';

  @override
  String get teamsTabSettings => 'Настройки';

  @override
  String get teamsTabHistory => 'История';

  @override
  String get teamsWarningInvalidLog =>
      'Сервер показал изменение состава, которое никто не подписал. Команда на паузе (только чтение), ключи никому не передаются.';

  @override
  String get teamsWarningRollback =>
      'Сервер показал более старый или другой состав команды, чем раньше. Команда на паузе (только чтение).';

  @override
  String get teamsRotationRequired =>
      'Участник покинул команду. Смените ключ, чтобы он не смог прочитать то, что будет добавлено дальше.';

  @override
  String get teamsRotateKey => 'Сменить ключ';

  @override
  String get teamsKeyRotated => 'Ключ команды сменён';

  @override
  String teamsMemberYou(String email) {
    return '$email (вы)';
  }

  @override
  String get teamsPendingInvitations => 'Отправленные приглашения';

  @override
  String get teamsCancelInvitation => 'Отменить';

  @override
  String get teamsInvite => 'Пригласить участника';

  @override
  String get teamsTransferOwner => 'Сделать владельцем';

  @override
  String teamsTransferOwnerConfirm(String email) {
    return '$email станет владельцем команды, вы останетесь администратором.';
  }

  @override
  String get teamsRemoveMember => 'Удалить из команды';

  @override
  String teamsRemoveMemberConfirm(String email) {
    return '$email потеряет доступ к команде. Ключ команды будет сменён, чтобы новое было им недоступно.';
  }

  @override
  String teamsInviteTitle(String name) {
    return 'Пригласить в «$name»';
  }

  @override
  String get teamsInviteBody =>
      'Нужна учётная запись ConsoleCrypt на этом сервере. После принятия приглашения вы сверите с человеком код — только после этого он получит доступ.';

  @override
  String get teamsInviteEmail => 'Почта';

  @override
  String get teamsRoleLabel => 'Роль';

  @override
  String get teamsInviteNote => 'Комментарий к приглашению (необязательно)';

  @override
  String get teamsInviteNoteWarning => 'Комментарий не шифруется: сервер может его прочитать.';

  @override
  String get teamsInviteSend => 'Отправить приглашение';

  @override
  String teamsInvited(String email) {
    return 'Приглашение отправлено: $email';
  }

  @override
  String get teamsSettingsReadOnly => 'Настройки команды меняют только владелец и администраторы.';

  @override
  String get teamsPoliciesTitle => 'Политики';

  @override
  String get teamsPolicyProduction => 'Отмечать новые контексты Kubernetes и базы данных как рабочие (прод)';

  @override
  String get teamsPolicyBlockAi => 'Не отправлять эти ресурсы ИИ-помощникам';

  @override
  String get teamsPolicyConfirmRisky => 'Спрашивать перед опасными командами';

  @override
  String get teamsPolicyClipboard => 'Очищать скопированные секреты через';

  @override
  String get teamsPolicyClipboardDefault => 'Как в приложении';

  @override
  String teamsPolicySeconds(int seconds) {
    return '$seconds с';
  }

  @override
  String get teamsSettingsSaved => 'Настройки команды сохранены';

  @override
  String get teamsMyColorTitle => 'Мой цвет для этой команды';

  @override
  String get teamsMyColorBody => 'Меняет цвет только на ваших устройствах.';

  @override
  String get teamsLeave => 'Покинуть команду';

  @override
  String teamsLeaveConfirm(String name) {
    return 'Вы потеряете доступ к «$name» на всех своих устройствах.';
  }

  @override
  String teamsKeyEpoch(int epoch) {
    return 'Поколение ключа: $epoch';
  }

  @override
  String teamsLogGenesis(String actor) {
    return '$actor создал(а) команду';
  }

  @override
  String teamsLogAdd(String actor, String member, String role) {
    return '$actor добавил(а) $member с ролью «$role»';
  }

  @override
  String teamsLogRole(String actor, String member, String role) {
    return '$actor изменил(а) роль $member на «$role»';
  }

  @override
  String teamsLogRemove(String actor, String member) {
    return '$actor удалил(а) $member';
  }

  @override
  String teamsLogLeave(String actor) {
    return '$actor покинул(а) команду';
  }

  @override
  String teamsLogTransfer(String actor, String member) {
    return '$actor передал(а) владение $member';
  }

  @override
  String teamsLogRotate(String actor, int epoch) {
    return '$actor сменил(а) ключ (поколение $epoch)';
  }

  @override
  String get teamsRoleOwner => 'Владелец';

  @override
  String get teamsRoleAdmin => 'Администратор';

  @override
  String get teamsRoleEditor => 'Редактор';

  @override
  String get teamsRoleViewer => 'Наблюдатель';

  @override
  String get teamsErrorReadOnly => 'Ресурсы этой команды доступны вам только для просмотра.';

  @override
  String get teamsErrorUntrusted => 'Команда на паузе: не удалось проверить её состав.';

  @override
  String get teamsErrorCrossVault =>
      'Ресурс команды может использовать только ресурсы той же команды. Сначала скопируйте учётные данные или промежуточный хост в команду.';

  @override
  String get teamsErrorPersonalOnly => 'Этот тип данных хранится только в личном хранилище.';

  @override
  String get teamsErrorAiBlocked => 'Политика команды запрещает отправлять этот ресурс ИИ-помощникам.';

  @override
  String get teamsErrorOwnerCannotLeave => 'Перед выходом передайте владение другому участнику.';

  @override
  String get teamsFilterAll => 'Все';

  @override
  String get teamsFilterPersonal => 'Личное';

  @override
  String get teamsCopyTo => 'Копировать в команду…';

  @override
  String get teamsMoveTo => 'Переместить в команду…';

  @override
  String get teamsMoveToPersonal => 'Переместить в личное';

  @override
  String get teamsPickTitle => 'Выберите команду';

  @override
  String teamsCopied(String name) {
    return 'Скопировано в «$name»';
  }

  @override
  String teamsMoved(String name) {
    return 'Перемещено в «$name»';
  }

  @override
  String get teamsNewGroup => 'Новая группа команды';

  @override
  String get teamsNewGroupHint => 'Хосты, созданные в группе команды, принадлежат команде.';

  @override
  String teamsPolicyAck(String host) {
    return 'Я проверил(а) эту команду для $host (политика команды)';
  }

  @override
  String dbInsertSnippetHelp(String shortcut) {
    return 'Сниппеты: вставить в редактор ($shortcut)';
  }
}
