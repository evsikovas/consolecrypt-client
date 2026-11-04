import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:consolecrypt/keepass/kdbx_documents.dart';
import 'package:consolecrypt/keepass/kdbx_workspace.dart';
import 'package:consolecrypt/keepass/password_generator_dialog.dart';
import 'package:consolecrypt/keepass/password_manager.dart';
import 'package:consolecrypt/keepass/webdav.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path/path.dart' as p;

Future<void> showKdbxManager(BuildContext context) =>
    showAppDialog<void>(context, secure: true, barrierDismissible: false, builder: (_) => const KdbxManagerDialog());

class KdbxManagerScreen extends ConsumerWidget {
  const KdbxManagerScreen({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profile = ref.watch(activeProfileProvider.select((p) => p?.id));
    final vault = ref.watch(vaultStatusProvider.select((v) => v.value?.vaultId));
    return KdbxManagerDialog(key: ValueKey((profile, vault)), embedded: true);
  }
}

class KdbxManagerDialog extends ConsumerStatefulWidget {
  const KdbxManagerDialog({super.key, this.embedded = false});
  final bool embedded;
  @override
  ConsumerState<KdbxManagerDialog> createState() => _KdbxManagerDialogState();
}

class _KdbxManagerDialogState extends ConsumerState<KdbxManagerDialog> {
  PasswordManager get _manager => ref.read(passwordManagerProvider);
  PasswordVaultSession? get _session => _manager.sessions[_manager.selected];
  KdbxData? get _data => _session?.data;
  String? _selectedEntry;
  KdbxData? _pendingInput;
  String? _group;
  String _search = '';
  String? _error;
  bool _busy = false;
  bool _allowPop = false;
  double _sidebarWidth = 240;
  double _detailsWidth = 410;
  final _valid = ValueNotifier(true);
  late final _profile = ref.read(activeProfileProvider)?.id;
  late final _vault = ref.read(vaultStatusProvider).value?.vaultId;
  final _searchController = TextEditingController();

  bool get _current =>
      mounted &&
      _valid.value &&
      ref.read(activeProfileProvider)?.id == _profile &&
      ref.read(vaultStatusProvider).value?.vaultId == _vault &&
      ref.read(vaultStatusProvider).value?.isUnlocked == true;

  void _expire() {
    _valid.value = false;
    _pendingInput?.clear();
    _pendingInput = null;
    _searchController.clear();
    setState(() {
      _error = 'closed';
    });
  }

  @override
  void dispose() {
    _valid.value = false;
    _pendingInput?.clear();
    _pendingInput = null;
    _searchController.dispose();
    _valid.dispose();
    super.dispose();
  }

  List<KdbxData> _rows(String key) => [for (final row in _data?[key] as List<Object?>? ?? []) row! as KdbxData];

  Future<void> _run(Future<void> Function() action) async {
    if (_busy || !_current) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await action();
    } on KdbxFailure catch (e) {
      if (mounted && e.code != 'cancelled') setState(() => _error = e.code);
    } catch (error) {
      if (mounted) showSnack(context, errorMessage(context.l10n, error), error: true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<bool> _discard([PasswordVaultSession? session]) async {
    if ((session ?? _session)?.data['dirty'] != true) return true;
    return showConfirmDialog(
      context,
      title: context.l10n.keepassDiscard,
      message: context.l10n.keepassDiscardHelp,
      confirmLabel: context.l10n.keepassDiscard,
      destructive: true,
    );
  }

  Future<void> _open({bool create = false}) => _run(() async {
    await chooseAndUnlockKdbx(context, ref, create: create);
    if (mounted && _current) _chooseSession();
  });

  Future<void> _edit([KdbxData? entry]) => _run(() async {
    final manager = _manager;
    final session = _session!;
    final client = session.client;
    final groups = session.rows('groups');
    final entryIds = session.rows('entries').map((e) => e['id']).toSet();
    final group = entry?['group'] as String? ?? _group ?? session.data['root'] as String?;
    final fields = entry == null
        ? <String, Object?>{}
        : (await client.request('get', {'id': entry['id']}))! as KdbxData;
    if (!mounted || !_current || !manager.current(session)) {
      fields.clear();
      return;
    }
    _pendingInput = fields;
    final KdbxData? input;
    try {
      input = await showAppDialog<KdbxData>(
        context,
        secure: true,
        builder: (_) => _KdbxFormDialog(
          valid: session.validity,
          title: context.l10n.keepassEntry,
          location: session.source.name,
          initial: fields,
          groups: groups,
          group: group,
        ),
      );
    } finally {
      fields.clear();
      if (identical(_pendingInput, fields)) _pendingInput = null;
    }
    if (input == null) return;
    final savedGroup = input['group'] as String? ?? group;
    _pendingInput = input;
    try {
      if (_current && manager.current(session)) {
        await manager.request(session, 'put', {...input, 'id': entry?['id'], 'group': savedGroup});
        if (mounted && _current && identical(_session, session)) {
          setState(() {
            _group = savedGroup;
            _selectedEntry =
                entry?['id'] as String? ??
                session.rows('entries').where((e) => !entryIds.contains(e['id'])).firstOrNull?['id'] as String?;
          });
        }
      }
    } finally {
      input.clear();
      if (identical(_pendingInput, input)) _pendingInput = null;
    }
  });

  Future<void> _newGroup() => _run(() async {
    final manager = _manager;
    final session = _session!;
    final parent = _group;
    final input = await showAppDialog<KdbxData>(
      context,
      secure: true,
      builder: (_) => _KdbxFormDialog(
        valid: session.validity,
        title: context.l10n.keepassNewGroup,
        groupOnly: true,
        location: session.source.name,
      ),
    );
    try {
      if (input != null && _current && manager.current(session)) {
        await manager.request(session, 'group', {'name': input['Title'], 'parent': parent});
      }
    } finally {
      input?.clear();
    }
  });

  bool _groupCurrent(PasswordManager manager, PasswordVaultSession session, String id) =>
      _current &&
      manager.current(session) &&
      identical(_session, session) &&
      id != session.data['root'] &&
      session.rows('groups').any((row) => row['id'] == id && row['recycled'] != true);

  Future<void> _renameGroup(String id) => _run(() async {
    final manager = _manager;
    final session = _session!;
    if (!_groupCurrent(manager, session, id)) return;
    final row = session.rows('groups').firstWhere((row) => row['id'] == id);
    final input = await showAppDialog<KdbxData>(
      context,
      secure: true,
      builder: (_) => _KdbxFormDialog(
        valid: session.validity,
        title: context.l10n.keepassRenameGroup,
        groupOnly: true,
        location: session.source.name,
        initial: {'Title': row['label']},
      ),
    );
    try {
      if (input != null && _groupCurrent(manager, session, id)) {
        await manager.request(session, 'renameGroup', {'id': id, 'name': input['Title']});
      }
    } finally {
      input?.clear();
    }
  });

  Future<void> _deleteGroup(String id) => _run(() async {
    final manager = _manager;
    final session = _session!;
    if (!_groupCurrent(manager, session, id)) return;
    final row = session.rows('groups').firstWhere((row) => row['id'] == id);
    final parent = row['parent'] as String?;
    final confirmed = await showConfirmDialog(
      context,
      title: context.l10n.keepassDeleteGroup(row['label']! as String),
      message: context.l10n.keepassDeleteGroupHelp,
      confirmLabel: context.l10n.commonDelete,
      destructive: true,
    );
    if (confirmed && _groupCurrent(manager, session, id)) {
      await manager.request(session, 'deleteGroup', {'id': id});
      if (mounted && _current && identical(_session, session)) {
        setState(() {
          // A selected descendant must not leave the UI inside the recycled tree.
          if (session.rows('groups').any((row) => row['id'] == _group && row['recycled'] == true)) {
            _group = parent == session.data['root'] ? null : parent;
          }
          _selectedEntry = null;
        });
      }
    }
  });

  Future<void> _delete(KdbxData entry) => _run(() async {
    final manager = _manager;
    final session = _session!;
    final confirmed = await showConfirmDialog(
      context,
      title: context.l10n.keepassDelete,
      message: context.l10n.keepassDeleteHelp,
      confirmLabel: context.l10n.commonDelete,
      destructive: true,
    );
    if (confirmed && _current && manager.current(session)) {
      await manager.request(session, 'delete', {'id': entry['id']});
    }
  });

  Future<void> _close() async {
    if (!mounted) return;
    setState(() => _allowPop = true);
    await Future<void>.delayed(Duration.zero);
    if (mounted) closeDialog<void>(context);
  }

  String _errorText(AppLocalizations l) => switch (_error) {
    'dav_url' => l.keepassDavUrlError,
    'dav_auth' => l.keepassDavAuthError,
    'dav_etag' => l.keepassDavEtagError,
    'dav_timeout' => l.keepassDavTimeout,
    'dav_locked' => l.keepassDavLocked,
    'dav_redirect' => l.keepassDavRedirectError,
    'dav_http' || 'dav_io' => l.keepassDavIoError,
    'key' => l.keepassKeyError,
    'unsupported' => l.keepassUnsupported,
    'changed' => l.keepassChanged,
    'io' => l.keepassIoError,
    'closed' => l.keepassClosed,
    'limits' => l.keepassLimits,
    'exists' => l.keepassExists,
    'group' => l.keepassGroupUnavailable,
    _ => l.keepassFormatError,
  };

  void _chooseSession() {
    setState(() {
      _group = null;
      _selectedEntry = null;
      _search = '';
    });
    _searchController.clear();
  }

  Future<void> _select(KdbxSource source) => _run(() async {
    await unlockKdbxSource(context, ref, source);
    if (mounted && _current) _chooseSession();
  });

  Future<void> _lock({KdbxSource? source}) => _run(() async {
    final manager = _manager;
    final session = manager.sessions[source?.id ?? manager.selected];
    final active = manager.selected == session?.source.id;
    if (session == null || !await _discard(session) || !_current || !manager.current(session)) return;
    manager.lock(session.source);
    if (active) _chooseSession();
  });

  Future<void> _connection([PasswordVaultConnection? existing]) => _run(() async {
    await showAppDialog<void>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (_) => WebDavConnectionDialog(existing: existing),
    );
  });

  Future<void> _export({KdbxSource? source}) => _run(() async {
    final session = _manager.sessions[source?.id ?? _manager.selected];
    if (session == null) return;
    final dialogs = ref.read(fileDialogServiceProvider);
    final path = await dialogs.chooseSaveFile(suggestedName: 'Passwords-copy.kdbx', extensions: const ['kdbx']);
    if (path == null || !_current || !_manager.current(session)) return;
    await session.client.request('exportFile', {'path': path});
    if (!_current || !_manager.current(session)) return;
    final saved = await dialogs.finishSaveFile(path);
    if (saved && mounted && _current) showSnack(context, context.l10n.keepassSaved);
  });

  Future<void> _protect(String method, {KdbxSource? source}) => _run(() async {
    final session = _manager.sessions[source?.id ?? _manager.selected];
    if (session == null) return;
    final info = await _manager.protection(session.source, method);
    if (!mounted || !_current) return;
    final l = context.l10n;
    final enabled = info['enabled'] == true;
    if (!await showConfirmDialog(
      context,
      secure: true,
      title: l.keepassProtection,
      message: l.keepassQuickHelp,
      confirmLabel: enabled ? l.keepassQuickDisabled : (method == 'hello' ? l.keepassHello : l.keepassSecurityKey),
    )) {
      return;
    }
    if (!mounted || !_current) return;
    final pin = !enabled && info['app_pin'] == true ? await _securityPin(context, _manager) : null;
    if (!enabled && info['app_pin'] == true && pin == null) return;
    await _manager.protect(session, method, pin: pin, forget: enabled, authReason: l.keepassUnlock);
    if (mounted && _current) showSnack(context, enabled ? l.keepassQuickDisabled : l.keepassQuickEnabled);
  });

  Future<void> _copyField(KdbxData entry, String key) => _run(() async {
    final session = _session!;
    final manager = _manager;
    final value = await manager.field(session, entry['id']! as String, key);
    if (mounted && _current && manager.current(session)) {
      await copySecretWithNotice(context, ref, value, isCurrent: () => _current && manager.current(session));
    }
  });

  Future<void> _generator() => _run(() async {
    final result = await showPasswordGenerator(context, valid: _manager.validity);
    result?.wipe();
  });

  Future<void> _removeSource(KdbxSource source) => _run(() async {
    final manager = _manager;
    final active = manager.selected == source.id;
    final l = context.l10n;
    final help = source.remote ? l.keepassRemoveRemoteHelp(source.name) : l.keepassRemoveLocalHelp(source.name);
    final dirty = manager.sessions[source.id]?.data['dirty'] == true;
    if (!await showConfirmDialog(
          context,
          title: l.keepassRemoveSource,
          message: dirty ? '$help\n\n${l.keepassDiscardHelp}' : help,
          confirmLabel: l.keepassRemoveSource,
          destructive: true,
        ) ||
        !_current) {
      return;
    }
    await manager.forgetSource(source);
    if (mounted && _current && active) _chooseSession();
  });

  void _sourceAction(KdbxSource source, String action) {
    switch (action) {
      case 'open':
        unawaited(_select(source));
      case 'lock':
        unawaited(_lock(source: source));
      case 'export':
        unawaited(_export(source: source));
      case 'settings':
        unawaited(_connection(source.connection));
      case 'remove':
        unawaited(_removeSource(source));
      case 'hello' || 'security_key':
        unawaited(_protect(action, source: source));
      case 'save':
        unawaited(
          _run(() async {
            final session = _manager.sessions[source.id];
            if (session != null) await _manager.save(session);
          }),
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(activeProfileProvider.select((p) => p?.id), (previous, next) {
      if (previous != next) _expire();
    });
    ref.listen(vaultStatusProvider.select((s) => (s.value?.vaultId, s.value?.isUnlocked == true)), (previous, next) {
      if (previous != next && (previous?.$1 != next.$1 || !next.$2)) _expire();
    });
    ref.watch(passwordManagerChangesProvider);
    ref.watch(vaultSettingsProvider);
    ref.watch(localSettingsProvider);
    final manager = ref.watch(passwordManagerProvider);
    if (_group != null && !_rows('groups').any((g) => g['id'] == _group)) _group = null;
    final l = context.l10n;
    final current = _current;
    final busy = _busy || _session?.busy == true;
    final entries = _rows('entries')
        .where(
          (entry) =>
              (entry['recycled'] != true || _group == entry['group'] && _search.isEmpty) &&
              (_group == null || entry['group'] == _group || _search.isNotEmpty) &&
              ['title', 'username', 'url'].any((key) => (entry[key]! as String).toLowerCase().contains(_search)),
        )
        .toList();
    final selected = entries.where((e) => e['id'] == _selectedEntry).firstOrNull;
    final content = KdbxWorkspace(
      sidebarWidth: _sidebarWidth,
      detailsWidth: _detailsWidth,
      onSidebarResize: (delta, minimum, maximum) =>
          setState(() => _sidebarWidth = (_sidebarWidth.clamp(minimum, maximum) + delta).clamp(minimum, maximum)),
      onDetailsResize: (delta, minimum, maximum) =>
          setState(() => _detailsWidth = (_detailsWidth.clamp(minimum, maximum) - delta).clamp(minimum, maximum)),
      manager: manager,
      session: _session,
      entries: entries,
      group: _group,
      selected: selected,
      search: _searchController,
      busy: busy || !current,
      error: _error != null
          ? _errorText(l)
          : _session?.saveError == null
          ? null
          : passwordManagerError(l, _session!.saveError!),
      onOpen: _open,
      onCreate: () => _open(create: true),
      onWebDav: _connection,
      onSource: _select,
      onEditSource: (source) => _connection(source.connection),
      onSourceAction: _sourceAction,
      onGenerator: _generator,
      onGroup: (id) => setState(() {
        _group = id;
        _selectedEntry = null;
      }),
      onGroupAction: (id, action) => unawaited(action == 'rename' ? _renameGroup(id) : _deleteGroup(id)),
      onEntry: (id) => setState(() => _selectedEntry = id),
      onEdit: _edit,
      onDelete: _delete,
      onCopy: _copyField,
      onMove: (drag, group) => _run(() async {
        final session = _session;
        if (session == null || session.source.id != drag.source) return;
        await manager.request(session, 'move', {'id': drag.entry, 'group': group});
        if (mounted && _current) setState(() => _selectedEntry = null);
      }),
      onNewEntry: _edit,
      onNewGroup: _newGroup,
      onLock: _lock,
      onExport: _export,
      onProtect: _protect,
      onRetrySave: () => _run(() async {
        final session = _session;
        if (session != null) await manager.save(session);
      }),
      onSearch: (text) => setState(() {
        _search = text.toLowerCase();
        _selectedEntry = null;
      }),
    );
    if (widget.embedded) {
      return Padding(key: const ValueKey('keepass-manager'), padding: const EdgeInsets.all(16), child: content);
    }
    return PopScope(
      canPop: _allowPop,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) unawaited(_close());
      },
      child: GlassDialog(
        key: const ValueKey('keepass-manager'),
        title: l.keepassTitle,
        icon: Icons.folder_copy_outlined,
        width: 1200,
        content: SizedBox(height: MediaQuery.sizeOf(context).height * .7, child: content),
        primaryAction: GlassButton(label: l.commonClose, onPressed: _close),
      ),
    );
  }
}

class _KdbxFormDialog extends ConsumerStatefulWidget {
  const _KdbxFormDialog({
    required this.valid,
    required this.title,
    this.unlock = false,
    this.confirmPassword = false,
    this.groupOnly = false,
    this.initial = const {},
    this.quickMethods = const {},
    this.pinOnly = false,
    this.location,
    this.groups = const [],
    this.group,
  });
  final ValueListenable<bool> valid;
  final String title;
  final bool unlock;
  final bool confirmPassword;
  final bool groupOnly;
  final KdbxData initial;
  final Set<String> quickMethods;
  final bool pinOnly;
  final String? location;
  final List<KdbxData> groups;
  final String? group;
  @override
  ConsumerState<_KdbxFormDialog> createState() => _KdbxFormDialogState();
}

class _KdbxFormDialogState extends ConsumerState<_KdbxFormDialog> {
  late final Map<String, TextEditingController> _fields;
  final _custom = <(TextEditingController, TextEditingController)>[];
  final _removed = <(TextEditingController, TextEditingController)>[];
  String? _keyFile;
  late String? _chosenGroup = widget.group;
  String? _error;
  Future<void> _generatePassword() async {
    final generated = await showPasswordGenerator(context, valid: widget.valid, allowUse: true);
    if (generated == null) return;
    try {
      if (mounted && widget.valid.value) {
        _fields['Password']!.text = generated.expose();
        if (widget.confirmPassword) _fields['confirm']!.text = _fields['Password']!.text;
      }
    } finally {
      generated.wipe();
    }
  }

  @override
  void initState() {
    super.initState();
    _fields = {
      for (final key in ['Title', 'UserName', 'Password', 'URL', 'Notes', 'confirm'])
        key: TextEditingController(text: widget.initial[key] as String? ?? ''),
    };
    if (widget.initial['custom'] case final Map<Object?, Object?> fields) {
      for (final field in fields.entries) {
        _custom.add((
          TextEditingController(text: field.key! as String),
          TextEditingController(text: field.value! as String),
        ));
      }
    }
    // The controllers own the editable values now. Do not retain a second
    // plaintext copy in widget properties while the dialog waits to close.
    if (widget.initial.isNotEmpty) widget.initial.clear();
    if (!widget.valid.value) {
      for (final field in _fields.values) {
        field.clear();
      }
      for (final field in _custom) {
        field.$1.clear();
        field.$2.clear();
      }
    }
    widget.valid.addListener(_scopeChanged);
  }

  void _scopeChanged() {
    if (!widget.valid.value) {
      for (final field in _fields.values) {
        field.clear();
      }
      for (final field in _custom) {
        field.$1.clear();
        field.$2.clear();
      }
      setState(() {});
    }
  }

  @override
  void dispose() {
    widget.valid.removeListener(_scopeChanged);
    for (final field in _fields.values) {
      field.clear();
      field.dispose();
    }
    for (final field in [..._custom, ..._removed]) {
      field.$1.clear();
      field.$2.clear();
      field.$1.dispose();
      field.$2.dispose();
    }
    super.dispose();
  }

  void _submit() {
    if (!widget.valid.value) return;
    final l = context.l10n;
    if (widget.confirmPassword && _fields['Password']!.text != _fields['confirm']!.text) {
      setState(() => _error = l.keepassMismatch);
      return;
    }
    if (widget.confirmPassword && _fields['Password']!.text.isEmpty && _keyFile == null) {
      setState(() => _error = l.keepassKeyError);
      return;
    }
    if (!widget.unlock && _fields['Title']!.text.trim().isEmpty) return;
    final customNames = <String>{};
    for (final field in _custom) {
      final name = field.$1.text.trim().toLowerCase();
      if (name.isEmpty ||
          name.length > 256 ||
          ['title', 'username', 'password', 'url', 'notes'].contains(name) ||
          !customNames.add(name)) {
        setState(() => _error = l.keepassFieldNameError);
        return;
      }
    }
    final result = <String, Object?>{
      for (final e in _fields.entries)
        if (e.key != 'confirm') e.key: e.value.text,
      'keyFile': _keyFile,
      if (widget.groups.isNotEmpty) 'group': _chosenGroup,
      'custom': {for (final field in _custom) field.$1.text: field.$2.text},
    };
    closeDialog(context, result);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return GlassDialog(
      title: widget.title,
      width: 560,
      content: !widget.valid.value
          ? Text(l.keepassClosed)
          : SingleChildScrollView(
              child: Column(
                spacing: 16,
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (widget.location != null) Text(widget.location!, maxLines: 3, overflow: TextOverflow.ellipsis),
                  for (final method in widget.quickMethods)
                    GlassButton(
                      label: method == 'hello' ? l.keepassHello : l.keepassSecurityKey,
                      icon: method == 'hello' ? Icons.fingerprint_rounded : Icons.security_rounded,
                      onPressed: () => closeDialog(context, <String, Object?>{'method': method}),
                    ),
                  if (!widget.unlock) ...[
                    if (widget.groups.isNotEmpty)
                      Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          Text(l.keepassGroup, style: Theme.of(context).textTheme.labelMedium),
                          const SizedBox(height: 6),
                          GlassMenuButton<String>(
                            entries: [
                              for (final group in widget.groups)
                                GlassMenuItem(
                                  value: group['id']! as String,
                                  label: group['name']! as String,
                                  checked: _chosenGroup == group['id'],
                                  icon: Icons.folder_outlined,
                                ),
                            ],
                            onSelected: (value) => setState(() => _chosenGroup = value),
                            builder: (context, open) => GlassButton(
                              key: const ValueKey('keepass-entry-group'),
                              label:
                                  widget.groups.where((g) => g['id'] == _chosenGroup).firstOrNull?['label']
                                      as String? ??
                                  l.keepassGroup,
                              icon: Icons.folder_outlined,
                              onPressed: widget.valid.value ? open : null,
                            ),
                          ),
                        ],
                      ),
                    TextField(
                      key: const ValueKey('keepass-title'),
                      controller: _fields['Title'],
                      autofocus: true,
                      decoration: InputDecoration(
                        labelText: widget.groupOnly ? l.keepassGroupName : l.keepassEntryTitle,
                      ),
                    ),
                    if (!widget.groupOnly) ...[
                      TextField(
                        controller: _fields['UserName'],
                        decoration: InputDecoration(labelText: l.keepassUsername),
                        autocorrect: false,
                        enableSuggestions: false,
                      ),
                      TextField(
                        controller: _fields['URL'],
                        decoration: InputDecoration(labelText: l.keepassUrl),
                        autocorrect: false,
                        enableSuggestions: false,
                      ),
                    ],
                  ],
                  if (!widget.groupOnly) ...[
                    SecretField(
                      key: const ValueKey('keepass-password'),
                      controller: _fields['Password']!,
                      label: widget.unlock ? l.keepassPassword : l.keepassEntryPassword,
                      autofocus: widget.unlock,
                    ),
                    if (!widget.unlock || widget.confirmPassword)
                      GlassButton.plain(
                        key: const ValueKey('keepass-generate-entry'),
                        label: l.keepassGenerator,
                        icon: Icons.auto_fix_high_rounded,
                        onPressed: _generatePassword,
                      ),
                    if (widget.confirmPassword) SecretField(controller: _fields['confirm']!, label: l.keepassConfirm),
                    if (widget.unlock && !widget.pinOnly) ...[
                      GlassButton.plain(
                        label: l.keepassKeyFile,
                        icon: Icons.key_rounded,
                        onPressed: () async {
                          final path = await ref.read(fileDialogServiceProvider).chooseOpenFile(extensions: const []);
                          if (mounted && widget.valid.value && path != null) setState(() => _keyFile = path);
                        },
                      ),
                      if (_keyFile != null)
                        Row(
                          children: [
                            Expanded(child: Text(p.basename(_keyFile!))),
                            GlassIconButton(
                              icon: Icons.close_rounded,
                              tooltip: l.keepassClearKey,
                              onPressed: () => setState(() => _keyFile = null),
                            ),
                          ],
                        ),
                    ] else if (!widget.unlock)
                      TextField(
                        controller: _fields['Notes'],
                        minLines: 3,
                        maxLines: 6,
                        decoration: InputDecoration(labelText: l.keepassNotes),
                        autocorrect: false,
                        enableSuggestions: false,
                      ),
                    if (!widget.unlock) ...[
                      for (final (index, field) in _custom.indexed)
                        Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Expanded(
                              child: TextField(
                                key: ValueKey('keepass-custom-name-$index'),
                                controller: field.$1,
                                decoration: InputDecoration(labelText: l.keepassField),
                              ),
                            ),
                            const SizedBox(width: 12),
                            Expanded(
                              child: SecretField(
                                key: ValueKey('keepass-custom-value-$index'),
                                controller: field.$2,
                                label: l.keepassValue,
                              ),
                            ),
                            const SizedBox(width: 12),
                            SizedBox(
                              width: 36,
                              height: 48,
                              child: Center(
                                child: GlassIconButton(
                                  key: ValueKey('keepass-custom-remove-$index'),
                                  icon: Icons.delete_outline_rounded,
                                  tooltip: l.commonDelete,
                                  onPressed: () {
                                    field.$1.clear();
                                    field.$2.clear();
                                    setState(() {
                                      _custom.remove(field);
                                      _removed.add(field);
                                    });
                                  },
                                ),
                              ),
                            ),
                          ],
                        ),
                      GlassButton.plain(
                        key: const ValueKey('keepass-add-field'),
                        label: l.keepassFieldAdd,
                        icon: Icons.add_rounded,
                        onPressed: () =>
                            setState(() => _custom.add((TextEditingController(), TextEditingController()))),
                      ),
                    ],
                  ],
                  if (_error != null) Text(_error!, style: TextStyle(color: GlassTokens.of(context).palette.danger)),
                ],
              ),
            ),
      onSubmit: _submit,
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('keepass-submit'),
        label: widget.unlock ? (widget.confirmPassword ? l.keepassCreate : l.keepassOpen) : l.commonSave,
        onPressed: widget.valid.value ? _submit : null,
      ),
    );
  }
}

/// Also used by Ctrl+K without navigating away from a terminal.
Future<PasswordVaultSession?> chooseAndUnlockKdbx(BuildContext context, WidgetRef ref, {bool create = false}) async {
  final manager = ref.read(passwordManagerProvider);
  manager.check();
  final documents = ref.read(kdbxDocumentsProvider);
  final source = await documents.choose(ref.read(fileDialogServiceProvider), create: create);
  if (source == null) return null;
  try {
    if (!context.mounted || !manager.valid) return null;
    return await unlockKdbxSource(context, ref, source, create: create);
  } finally {
    if (manager.sessions.containsKey(source.id)) {
      documents.keep(source);
    } else {
      await documents.discard(source);
    }
  }
}

/// Unlock a remembered file or WebDAV source; no picker is needed.
Future<PasswordVaultSession?> unlockKdbxSource(
  BuildContext context,
  WidgetRef ref,
  KdbxSource source, {
  bool create = false,
}) async {
  final authReason = context.l10n.keepassUnlock;
  final manager = ref.read(passwordManagerProvider);
  manager.check();
  if (source.remote && !KdbxWebDav.isFileUri(Uri.parse(source.path))) throw const KdbxFailure('dav_url');
  final existing = manager.sessions[source.id];
  if (existing != null) {
    manager.selected = source.id;
    manager.notify();
    return existing;
  }
  final methods = <String>{};
  final appPins = <String>{};
  if (!create) {
    for (final method in ['hello', 'security_key']) {
      try {
        final info = await manager.protection(source, method);
        if (info['available'] == true && info['enabled'] == true) methods.add(method);
        if (info['app_pin'] == true) appPins.add(method);
      } catch (_) {
        manager.check();
      }
    }
  }
  if (!context.mounted || !manager.valid) return null;
  final input = await showAppDialog<KdbxData>(
    context,
    secure: true,
    barrierDismissible: false,
    builder: (_) => _KdbxFormDialog(
      valid: manager.validity,
      title: create ? context.l10n.keepassCreate : context.l10n.keepassUnlock,
      location: source.path,
      unlock: true,
      confirmPassword: create,
      quickMethods: methods,
    ),
  );
  if (input == null) return null;
  try {
    manager.check();
    final method = input['method'] as String?;
    String? pin;
    if (method != null && appPins.contains(method)) {
      if (!context.mounted) return null;
      pin = await _securityPin(context, manager);
      if (pin == null) return null;
    }
    return await manager.open(
      source,
      password: method == null ? SecretText(input['Password']! as String) : null,
      keyFile: input['keyFile'] as String?,
      create: create,
      method: method,
      pin: pin,
      authReason: authReason,
    );
  } finally {
    input.clear();
  }
}

Future<String?> _securityPin(BuildContext context, PasswordManager manager) async {
  final input = await showAppDialog<KdbxData>(
    context,
    secure: true,
    builder: (_) =>
        _KdbxFormDialog(valid: manager.validity, title: context.l10n.keepassPin, unlock: true, pinOnly: true),
  );
  try {
    if (!manager.valid) return null;
    return input?['Password'] as String?;
  } finally {
    input?.clear();
  }
}

String passwordManagerError(AppLocalizations l, Object error) => error is KdbxFailure
    ? switch (error.code) {
        'group' => l.keepassGroupUnavailable,
        'key' => l.keepassKeyError,
        'changed' => l.keepassChanged,
        'closed' => l.keepassClosed,
        'limits' => l.keepassLimits,
        'exists' => l.keepassExists,
        'unsupported' => l.keepassUnsupported,
        'dav_url' => l.keepassDavUrlError,
        'dav_auth' => l.keepassDavAuthError,
        'dav_etag' => l.keepassDavEtagError,
        'dav_timeout' => l.keepassDavTimeout,
        'dav_locked' => l.keepassDavLocked,
        'dav_redirect' => l.keepassDavRedirectError,
        'dav_io' || 'dav_http' => l.keepassDavIoError,
        'io' => l.keepassIoError,
        _ => l.keepassFormatError,
      }
    : errorMessage(l, error);

class WebDavConnectionDialog extends ConsumerStatefulWidget {
  const WebDavConnectionDialog({super.key, this.existing});
  final PasswordVaultConnection? existing;
  @override
  ConsumerState<WebDavConnectionDialog> createState() => _WebDavConnectionDialogState();
}

class _WebDavConnectionDialogState extends ConsumerState<WebDavConnectionDialog> {
  late final _manager = ref.read(passwordManagerProvider);
  late final _name = TextEditingController(text: widget.existing?.name);
  late final _url = TextEditingController(text: widget.existing?.url);
  late final _username = TextEditingController(text: widget.existing?.username);
  final _password = TextEditingController();
  bool _busy = false;
  String? _error;
  @override
  void initState() {
    super.initState();
    _manager.validity.addListener(_scope);
  }

  void _scope() {
    if (!_manager.valid) {
      for (final c in [_name, _url, _username, _password]) {
        c.clear();
      }
      if (mounted) setState(() {});
    }
  }

  @override
  void dispose() {
    _manager.validity.removeListener(_scope);
    for (final c in [_name, _url, _username, _password]) {
      c.clear();
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _save({bool remove = false}) async {
    if (_busy || !_manager.valid) return;
    final l = context.l10n;
    final uri = Uri.tryParse(_url.text.trim());
    if (!remove && (_name.text.trim().isEmpty || uri == null || !KdbxWebDav.isFileUri(uri))) {
      setState(() => _error = l.keepassDavUrlError);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final oldSource = widget.existing == null ? null : KdbxSource.dav(widget.existing!);
      if (oldSource != null && _manager.sessions[oldSource.id]?.data['dirty'] == true) {
        if (!await showConfirmDialog(
          context,
          title: l.keepassDiscard,
          message: l.keepassDiscardHelp,
          confirmLabel: l.keepassDiscard,
          destructive: true,
        )) {
          return;
        }
      }
      if (!mounted || !_manager.valid) return;
      if (remove &&
          !await showConfirmDialog(
            context,
            title: l.keepassConnectDelete,
            message: l.keepassConnectDeleteHelp,
            confirmLabel: l.commonDelete,
            destructive: true,
          )) {
        return;
      }
      _manager.check();
      String? credential = widget.existing?.credentialId;
      if (!remove && _password.text.isNotEmpty) {
        final secret = SecretText(_password.text);
        _password.clear();
        try {
          final saved = await ref
              .read(inventoryServiceProvider)
              .createPasswordCredential(
                name: 'WebDAV · ${_name.text.trim()}',
                password: secret,
                username: _username.text.trim(),
              ); // l10n-ignore: protocol name.
          credential = saved.id.value;
        } finally {
          secret.wipe();
        }
      }
      _manager.check();
      final settings = ref.read(vaultSettingsProvider).value;
      if (settings == null) throw const KdbxFailure('closed');
      final config = PasswordVaultConnection(
        id: widget.existing?.id ?? generateUuidV4(),
        name: _name.text.trim(),
        url: _url.text.trim(),
        username: _username.text.trim(),
        credentialId: credential,
      );
      final list = settings.passwordVaults.where((c) => c.id != config.id).toList();
      if (!remove) list.add(config);
      await ref.read(settingsServiceProvider).updateVault(settings.copyWith(passwordVaults: list));
      _manager.check();
      if (widget.existing != null) {
        final old = KdbxSource.dav(widget.existing!);
        _manager.lock(old);
      }
      _manager.notify();
      if (mounted) closeDialog<void>(context);
    } catch (error) {
      if (mounted) setState(() => _error = passwordManagerError(l, error));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final enabled = _manager.valid && !_busy;
    return GlassDialog(
      title: l.keepassDavAdd,
      width: 600,
      content: SingleChildScrollView(
        child: Column(
          spacing: 16,
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.keepassDavHelp),
            TextField(
              key: const ValueKey('keepass-webdav-name'),
              controller: _name,
              enabled: enabled,
              decoration: InputDecoration(labelText: l.keepassDavName),
            ),
            TextField(
              key: const ValueKey('keepass-webdav-url'),
              controller: _url,
              enabled: enabled,
              autocorrect: false,
              enableSuggestions: false,
              decoration: InputDecoration(labelText: l.keepassDavUrl),
            ),
            TextField(
              key: const ValueKey('keepass-webdav-username'),
              controller: _username,
              enabled: enabled,
              autocorrect: false,
              enableSuggestions: false,
              decoration: InputDecoration(labelText: l.keepassUsername),
            ),
            SecretField(controller: _password, label: l.keepassDavPassword),
            if (_busy) const LinearProgressIndicator(),
            if (_error != null) Text(_error!, style: TextStyle(color: GlassTokens.of(context).palette.danger)),
          ],
        ),
      ),
      primaryAction: GlassButton.prominent(
        key: const ValueKey('keepass-webdav-save'),
        label: l.commonSave,
        onPressed: enabled ? _save : null,
      ),
      secondaryActions: [
        if (widget.existing != null)
          GlassButton(label: l.commonDelete, onPressed: enabled ? () => _save(remove: true) : null),
        GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
      ],
    );
  }
}
