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
  String get shellSearchPlaceholder => 'Поиск сниппетов или вопрос ИИ…';

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
  String get errorOsAuthFailed => 'Системная аутентификация не пройдена или отменена.';

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
      'Все данные хранятся на этом компьютере в зашифрованном виде. Без сервера, без аккаунта, без сетевого трафика синхронизации. Синхронизацию можно включить позже.';

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
  String get paletteSearchHint => 'Найдите сниппет, выберите действие или опишите команду…';

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
  String get unlockOsAuthReason => 'Разблокировать хранилище ConsoleCrypt';

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
    return 'Хранилище откроется ключом этого устройства (защита: $method). Затем задайте новую парольную фразу; другие устройства продолжат работать.';
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
  String get aboutOpenSource =>
      'ConsoleCrypt — программа с открытым исходным кодом. Сервер никогда не видит ваше хранилище: всё шифруется на ваших устройствах.';

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
      'Подтверждайте вход отпечатком пальца. macOS также может предложить пароль учётной записи.';

  @override
  String get deviceUnlockLocalOnly =>
      'Только для этого хранилища на этом устройстве. Парольная фраза и ключ восстановления остаются доступны.';

  @override
  String get deviceUnlockEnableReason => 'Разрешите разблокировку хранилища ConsoleCrypt на этом Mac';

  @override
  String get deviceUnlockNeedsTrust => 'Сначала разблокируйте хранилище парольной фразой на этом устройстве.';

  @override
  String get deviceUnlockNotEnrolled =>
      'Отпечаток не настроен. Добавьте его в Системных настройках macOS → Touch ID и пароль, затем проверьте доступность снова.';

  @override
  String get deviceUnlockUnsupported =>
      'Биометрическая разблокировка сейчас недоступна на этом устройстве. Если Mac поддерживает Touch ID, настройте его в Системных настройках → Touch ID и пароль.';

  @override
  String get deviceUnlockMacPassword =>
      'Сейчас macOS предлагает вход своим паролем. Для отпечатка нужен доступный Touch ID с настроенным отпечатком: Системные настройки → Touch ID и пароль.';

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
}
