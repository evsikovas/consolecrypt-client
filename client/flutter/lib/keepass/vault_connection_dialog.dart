import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/keepass/external_vault.dart';
import 'package:consolecrypt/keepass/external_vault_store.dart';
import 'package:consolecrypt/keepass/password_manager.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showVaultConnections(BuildContext context, {String? id}) => showAppDialog<void>(
  context,
  secure: true,
  barrierDismissible: false,
  builder: (_) => ExternalVaultWorkspace(id: id),
);

class ExternalVaultWorkspace extends ConsumerStatefulWidget {
  const ExternalVaultWorkspace({super.key, this.id, this.onClose});
  final String? id;
  final VoidCallback? onClose;

  @override
  ConsumerState<ExternalVaultWorkspace> createState() => _VaultConnectionsState();
}

class _VaultConnectionsState extends ConsumerState<ExternalVaultWorkspace> {
  late final _owner = ref.read(passwordManagerProvider);
  final _store = ExternalVaultCredentialStore();
  final _name = TextEditingController();
  final _endpoint = TextEditingController();
  final _user = TextEditingController();
  final _authMount = TextEditingController();
  final _mount = TextEditingController();
  final _namespace = TextEditingController();
  final _credential = TextEditingController();
  final _path = TextEditingController();
  final _json = TextEditingController();
  final _search = TextEditingController();
  ExternalVaultAuth _auth = ExternalVaultAuth.userpass;
  ExternalVaultConnection? _selected;
  ExternalVaultSession? _client;
  ExternalVaultData? _data;
  List<String> _keys = [];
  List<ExternalVaultMount> _mounts = [];
  String _folder = '';
  String? _secretPath, _error, _listError, _revealed;
  Timer? _hide;
  bool _configuring = false;
  bool _editing = false;
  bool _versioned = true;
  bool _rememberCredential = false;
  bool _verified = false;
  bool _busy = false;

  bool get _current => mounted && _owner.valid;

  @override
  void initState() {
    super.initState();
    _owner.validity.addListener(_expire);
    if (widget.id != null) {
      final target = ref.read(externalVaultConnectionsProvider).where((item) => item.id == widget.id).firstOrNull;
      if (target != null) _configure(target);
    }
  }

  void _clearDetail() {
    _data?.values.clear();
    _data = null;
    _secretPath = null;
    _revealed = null;
    _hide?.cancel();
    _editing = false;
    _json.clear();
  }

  void _closeSession() {
    _client?.close();
    _client = null;
    _keys = [];
    _mounts = [];
    _verified = false;
    _folder = '';
    _path.clear();
    _search.clear();
    _clearDetail();
    _credential.clear();
  }

  void _expire() {
    _closeSession();
    _selected = null;
    if (mounted) setState(() => _error = 'closed');
  }

  @override
  void dispose() {
    _owner.validity.removeListener(_expire);
    _closeSession();
    for (final controller in [
      _name,
      _endpoint,
      _user,
      _authMount,
      _mount,
      _namespace,
      _credential,
      _path,
      _json,
      _search,
    ]) {
      controller.dispose();
    }
    super.dispose();
  }

  void _configure(ExternalVaultConnection? connection) {
    _closeSession();
    _selected = connection;
    _name.text = connection?.name ?? '';
    _endpoint.text = connection?.endpoint.toString() ?? '';
    _auth = connection?.auth ?? ExternalVaultAuth.userpass;
    _user.text = connection?.username ?? '';
    _authMount.text = connection?.authMount ?? 'userpass';
    _mount.text = connection?.mount ?? 'secret';
    _namespace.text = connection?.namespace ?? '';
    _versioned = connection?.versioned ?? true;
    _rememberCredential = false;
    _configuring = true;
    _error = null;
  }

  ExternalVaultConnection _configuration() {
    final vault = ref.read(vaultStatusProvider).value?.vaultId?.value;
    if (vault == null || !_current) throw const ExternalVaultFailure('closed');
    final random = Random.secure();
    return ExternalVaultConnection(
      id: _selected?.id ?? base64UrlEncode(List.generate(18, (_) => random.nextInt(256))),
      profile: _owner.profile,
      vault: vault,
      name: _name.text.trim(),
      endpoint: _endpoint.text,
      auth: _auth,
      username: _auth == ExternalVaultAuth.userpass ? _user.text.trim() : '',
      authMount: _auth == ExternalVaultAuth.userpass ? _authMount.text.trim() : '',
      mount: _mount.text.trim(),
      namespace: _namespace.text.trim(),
      versioned: _versioned,
    );
  }

  Future<void> _run(Future<void> Function() fn) async {
    if (_busy || !_current) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await fn();
    } on ExternalVaultFailure catch (error) {
      if (_current) _error = error.code;
    } catch (_) {
      if (_current) _error = 'store';
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _persist(ExternalVaultConnection connection, {String? wrapped, bool remove = false}) async {
    if (!_current) throw const ExternalVaultFailure('closed');
    final service = ref.read(settingsServiceProvider);
    final local = service.currentLocal;
    final rows = [
      for (final row in local.externalVaultConnections)
        if (row['id'] != connection.id || row['profile'] != connection.profile || row['vault'] != connection.vault) row,
      if (!remove) connection.toStorage(cipherText: wrapped),
    ];
    if (rows.length > 256) throw const ExternalVaultFailure('store');
    await service.updateLocal(local.copyWith(externalVaultConnections: rows));
  }

  Future<void> _discardCredential(ExternalVaultConnection connection, String? wrapped) async {
    if (!_current || wrapped == null) return;
    try {
      await _store.forget(connection, wrapped);
    } catch (_) {
      // The settings reference has been removed. Its key cannot open another connection.
    }
  }

  Future<void> _connect({bool saved = false}) => _run(() async {
    final configuration = _configuration();
    FocusScope.of(context).unfocus();
    final previous = _selected;
    final previousWrapper = previous?.binding == configuration.binding ? previous?.wrapped : null;
    SecretText? secret;
    if (saved) {
      if (previous == null || previousWrapper == null) throw const ExternalVaultFailure('configuration');
      secret = await _store.unlock(previous, () => _current);
    } else {
      secret = SecretText(_credential.text);
    }
    _credential.clear();
    if (!_current) {
      secret.wipe();
      return;
    }
    final client = ExternalVaultSession(configuration);
    _client = client;
    var connected = false;
    try {
      await client.signIn(secret);
      List<ExternalVaultMount> discovered;
      try {
        discovered = await client.discoverMounts();
      } on ExternalVaultFailure {
        // Older or restricted servers can still use the configured KV mount.
        discovered = [];
      }
      if (!_current || _client != client) return;
      final configured = client.mount;
      final match = discovered.where((mount) => mount.path == configured.path).firstOrNull;
      if (match != null) {
        client.selectMount(match);
      } else if (configured.path == 'secret' && discovered.isNotEmpty) {
        client.selectMount(discovered.first);
      } else {
        discovered = [...discovered, configured];
      }
      _mounts = discovered;
      await _browse('');
      if (!_current || _client != client) return;
      if (_rememberCredential && !saved) {
        if (!_verified && configuration.auth == ExternalVaultAuth.token) {
          await client.verifyToken();
        }
        if (!await _store.available(configuration)) throw const ExternalVaultFailure('biometric');
        final wrapper = await _store.remember(configuration, secret, () => _current);
        try {
          await _persist(configuration, wrapped: wrapper);
        } catch (_) {
          await _discardCredential(configuration, wrapper);
          rethrow;
        }
      } else {
        await _persist(configuration, wrapped: previousWrapper);
      }
      if (previous != null && previous.wrapped != null && (previousWrapper == null || _rememberCredential && !saved)) {
        await _discardCredential(previous, previous.wrapped);
      }
      if (!_current) return;
      _configuring = false;
      connected = true;
    } finally {
      secret.wipe();
      if (!connected && _client == client) _closeSession();
    }
  });

  Future<void> _browse(String folder) async {
    final client = _client!;
    _listError = null;
    List<String> keys;
    try {
      keys = await client.list(folder);
      if (!_current || _client != client) return;
      _verified = true;
    } on ExternalVaultFailure catch (error) {
      if (error.code != 'permission') rethrow;
      if (!_current || _client != client) return;
      _listError = 'listPermission';
      keys = [];
    }
    _clearDetail();
    _folder = folder;
    _keys = keys;
    _path.clear();
    _search.clear();
  }

  Future<void> _selectMount(ExternalVaultMount mount) => _run(() async {
    final client = _client!;
    final previous = client.mount;
    client.selectMount(mount);
    try {
      await _browse('');
    } catch (_) {
      if (_current && _client == client) client.selectMount(previous);
      rethrow;
    }
  });

  String _underFolder(String name) => _folder.isEmpty ? name : '$_folder/$name';

  Future<void> _open(String path) async {
    if (path.isEmpty) throw const ExternalVaultFailure('configuration');
    FocusScope.of(context).unfocus();
    final client = _client!;
    final data = await client.read(path);
    if (!_current || client != _client) {
      data.values.clear();
      return;
    }
    _clearDetail();
    _secretPath = path;
    _data = data;
    _verified = true;
  }

  void _edit() {
    final data = _data;
    _path.text = _secretPath ?? (_folder.isEmpty ? '' : '$_folder/');
    _json.text = const JsonEncoder.withIndent('  ').convert(data?.values ?? {'username': '', 'password': ''});
    _editing = true;
    _revealed = null;
    _error = null;
  }

  Future<void> _write() => _run(() async {
    final path = _path.text.trim();
    final client = _client!;
    Map<String, Object?> values;
    try {
      values = jsonDecode(_json.text) as Map<String, Object?>;
    } catch (_) {
      throw const ExternalVaultFailure('json');
    }
    try {
      if (!client.mount.versioned && _data == null) {
        try {
          final existing = await client.read(path);
          existing.values.clear();
          throw const ExternalVaultFailure('conflict');
        } on ExternalVaultFailure catch (error) {
          if (error.code != 'missing') rethrow;
        }
      }
      await client.write(path, values, version: _data?.version);
      if (!_current || client != _client) return;
      _editing = false;
      _json.clear();
      _clearDetail();
      try {
        await _open(path);
      } on ExternalVaultFailure catch (error) {
        if (error.code != 'permission') rethrow;
        await _browse(_folder);
      }
      if (mounted && _current) showSnack(context, context.l10n.keepassSaved);
    } finally {
      values.clear();
    }
  });

  Future<void> _delete() => _run(() async {
    final client = _client!;
    final path = _secretPath!;
    final l = context.l10n;
    if (!await showConfirmDialog(
          context,
          secure: true,
          title: l.commonDelete,
          destructive: true,
          confirmLabel: l.commonDelete,
          message: client.mount.versioned ? l.externalVaultDeleteVersion : l.externalVaultDeletePermanent,
        ) ||
        !_current ||
        client != _client) {
      return;
    }
    await client.delete(path, _data?.version);
    if (_current && _client == client) {
      _clearDetail();
      await _browse(_folder);
    }
  });

  String _errorText(AppLocalizations l, String code) => switch (code) {
    'configuration' => l.externalVaultInvalidConfig,
    'authentication' => l.externalVaultAuthError,
    'permission' => l.externalVaultPermission,
    'listPermission' => l.externalVaultListPermission,
    'network' || 'server' => l.externalVaultNetwork,
    'tls' || 'redirect' => l.externalVaultTls,
    'missing' => l.externalVaultMissing,
    'conflict' => l.externalVaultConflict,
    'json' => l.externalVaultJsonInvalid,
    'biometric' => l.externalVaultBiometricUnavailable,
    'closed' => l.keepassLock,
    'store' => l.externalVaultStoreError,
    _ => l.externalVaultRequestError,
  };

  Widget _field(TextEditingController controller, String label, {String? hint, ValueChanged<String>? onChanged}) =>
      TextField(
        controller: controller,
        enabled: !_busy,
        autocorrect: false,
        enableSuggestions: false,
        enableIMEPersonalizedLearning: false,
        onChanged: onChanged,
        decoration: InputDecoration(labelText: label, hintText: hint),
      );

  Widget _configForm(AppLocalizations l) => SingleChildScrollView(
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      spacing: GlassSpacing.s8,
      children: [
        _field(_name, l.keepassEntryTitle),
        _field(
          _endpoint,
          l.externalVaultEndpoint,
          hint: 'https://vault.example.com',
          onChanged: (_) => setState(() {}),
        ),
        if (Uri.tryParse(_endpoint.text.trim())?.scheme == 'http')
          Text(l.externalVaultHttpNotice, style: Theme.of(context).textTheme.bodySmall),
        GlassSelect<ExternalVaultAuth>(
          value: _auth,
          expand: true,
          semanticLabel: l.externalVaultAuth,
          items: [
            GlassSelectItem(value: ExternalVaultAuth.userpass, label: l.externalVaultUserpass),
            GlassSelectItem(value: ExternalVaultAuth.token, label: l.externalVaultToken),
          ],
          onChanged: _busy
              ? null
              : (value) => setState(() {
                  _auth = value;
                  _credential.clear();
                }),
        ),
        if (_auth == ExternalVaultAuth.userpass) _field(_user, l.keepassUsername),
        SecretField(
          controller: _credential,
          enabled: !_busy,
          allowReveal: false,
          label: _auth == ExternalVaultAuth.userpass ? l.keepassEntryPassword : l.externalVaultToken,
        ),
        ExpansionTile(
          title: Text(l.externalVaultApiSettings),
          childrenPadding: const EdgeInsets.only(bottom: GlassSpacing.s8),
          children: [
            Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              spacing: GlassSpacing.s8,
              children: [
                _field(_mount, l.externalVaultMount),
                _field(_namespace, l.externalVaultNamespace),
                if (_auth == ExternalVaultAuth.userpass) _field(_authMount, l.externalVaultAuthMount),
                GlassSelect<bool>(
                  value: _versioned,
                  expand: true,
                  items: const [
                    GlassSelectItem(value: true, label: 'KV v2'),
                    GlassSelectItem(value: false, label: 'KV v1'),
                  ],
                  onChanged: _busy ? null : (value) => setState(() => _versioned = value),
                ),
              ],
            ),
          ],
        ),
        CheckboxListTile(
          value: _rememberCredential,
          title: Text(l.externalVaultRemember),
          onChanged: _busy ? null : (value) => setState(() => _rememberCredential = value!),
        ),
        Wrap(
          spacing: GlassSpacing.toolbarItemGap,
          runSpacing: GlassSpacing.s8,
          children: [
            GlassButton.prominent(label: l.commonConnect, onPressed: _busy ? null : _connect),
            if (_selected?.wrapped != null)
              GlassButton(
                label: l.externalVaultUseSaved,
                icon: Icons.fingerprint,
                onPressed: _busy ? null : () => _connect(saved: true),
              ),
            if (_selected != null)
              GlassButton.plain(
                label: l.externalVaultRemoveConnection,
                onPressed: _busy
                    ? null
                    : () => _run(() async {
                        final chosen = _selected!;
                        if (!await showConfirmDialog(
                              context,
                              title: l.externalVaultRemoveConnection,
                              message: chosen.name,
                              confirmLabel: l.commonDelete,
                              destructive: true,
                              secure: true,
                            ) ||
                            !_current) {
                          return;
                        }
                        await _persist(chosen, remove: true);
                        await _discardCredential(chosen, chosen.wrapped);
                        _selected = null;
                        _configuring = false;
                        _credential.clear();
                      }),
              ),
            GlassButton.plain(
              label: l.commonBack,
              onPressed: _busy
                  ? null
                  : () => setState(() {
                      _configuring = false;
                      _error = null;
                      _credential.clear();
                    }),
            ),
          ],
        ),
      ],
    ),
  );

  void _openSearchPath() {
    final query = _search.text.trim();
    if (query.isEmpty) return;
    final path = query.contains('/') ? query : _underFolder(query);
    unawaited(_run(() => path.endsWith('/') ? _browse(path.substring(0, path.length - 1)) : _open(path)));
  }

  Widget _browser(AppLocalizations l) {
    final client = _client!;
    final path = vaultPath(_folder, optional: true);
    final query = _search.text.trim().toLowerCase();
    final entries = _keys.where((entry) => entry.toLowerCase().contains(query)).toList();
    return CustomScrollView(
      slivers: [
        SliverToBoxAdapter(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            spacing: GlassSpacing.s8,
            children: [
              Wrap(
                spacing: GlassSpacing.toolbarItemGap,
                runSpacing: GlassSpacing.s8,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  GlassButton.plain(
                    label: l.commonBack,
                    icon: Icons.arrow_back,
                    onPressed: _busy ? null : () => setState(_closeSession),
                  ),
                  if (_mounts.length > 1)
                    ConstrainedBox(
                      constraints: const BoxConstraints(maxWidth: 220),
                      child: GlassSelect<ExternalVaultMount>(
                        value: client.mount,
                        expand: true,
                        semanticLabel: l.externalVaultMount,
                        items: [for (final mount in _mounts) GlassSelectItem(value: mount, label: mount.path)],
                        onChanged: _busy ? null : _selectMount,
                      ),
                    ),
                  GlassButton(
                    label: l.externalVaultNewSecret,
                    icon: Icons.add,
                    onPressed: _busy ? null : () => setState(_edit),
                  ),
                ],
              ),
              SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  spacing: GlassSpacing.s4,
                  children: [
                    GlassButton.plain(
                      label: client.mount.path,
                      icon: Icons.folder_outlined,
                      onPressed: _busy || path.isEmpty ? null : () => _run(() => _browse('')),
                    ),
                    for (var i = 0; i < path.length; i++) ...[
                      const Icon(Icons.chevron_right, size: 16),
                      _folderCrumb(path[i], path.take(i + 1).join('/'), current: i == path.length - 1),
                    ],
                  ],
                ),
              ),
              Row(
                spacing: GlassSpacing.s8,
                children: [
                  Expanded(
                    child: TextField(
                      key: const ValueKey('external-vault-search'),
                      controller: _search,
                      enabled: !_busy,
                      autocorrect: false,
                      enableSuggestions: false,
                      enableIMEPersonalizedLearning: false,
                      textInputAction: TextInputAction.search,
                      decoration: InputDecoration(
                        labelText: l.externalVaultSearch,
                        prefixIcon: const Icon(Icons.search),
                      ),
                      onChanged: (_) => setState(() {}),
                      onSubmitted: (_) => _openSearchPath(),
                    ),
                  ),
                  GlassButton(label: l.commonOpen, onPressed: _busy || query.isEmpty ? null : _openSearchPath),
                ],
              ),
              if (_listError case final error?) Text(_errorText(l, error)),
              const SizedBox.shrink(),
            ],
          ),
        ),
        if (entries.isEmpty && _listError == null)
          SliverFillRemaining(
            hasScrollBody: false,
            child: Center(child: Text(query.isEmpty ? l.externalVaultEmpty : l.externalVaultNoMatches)),
          )
        else
          SliverList.builder(
            itemCount: entries.length,
            itemBuilder: (context, index) {
              final key = entries[index];
              final folder = key.endsWith('/');
              final label = folder ? key.substring(0, key.length - 1) : key;
              final path = _underFolder(label);
              return ListTile(
                key: ValueKey('external-vault-item-$key'),
                dense: true,
                selected: !folder && _secretPath == path,
                leading: Icon(folder ? Icons.folder_outlined : Icons.key_outlined, size: 18),
                trailing: const Icon(Icons.chevron_right, size: 16),
                title: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis),
                onTap: _busy ? null : () => _run(() => folder ? _browse(path) : _open(path)),
              );
            },
          ),
      ],
    );
  }

  Widget _folderCrumb(String label, String path, {required bool current}) => GlassButton.plain(
    label: label,
    tooltip: path,
    onPressed: _busy || current ? null : () => _run(() => _browse(path)),
  );

  Widget _detail(AppLocalizations l) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    spacing: GlassSpacing.s8,
    children: [
      Text(_secretPath!, maxLines: 2, overflow: TextOverflow.ellipsis, style: Theme.of(context).textTheme.titleMedium),
      Wrap(
        spacing: GlassSpacing.toolbarItemGap,
        runSpacing: GlassSpacing.s8,
        children: [
          GlassButton.plain(
            label: l.commonBack,
            icon: Icons.arrow_back,
            onPressed: _busy
                ? null
                : () => setState(() {
                    _clearDetail();
                    _error = null;
                  }),
          ),
          GlassButton(label: l.commonEdit, onPressed: _busy ? null : () => setState(_edit)),
          GlassButton.plain(label: l.commonDelete, onPressed: _busy ? null : _delete),
        ],
      ),
      Expanded(
        child: ListView(
          children: [
            for (final pair in _data!.values.entries)
              ListTile(
                dense: true,
                title: Text(pair.key),
                subtitle: _revealed == pair.key
                    ? RevealedSecret(child: SelectableText(_value(pair.value)))
                    : const Text('••••••••'),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  spacing: GlassSpacing.toolbarItemGap,
                  children: [
                    GlassIconButton(
                      size: GlassControlSize.md.height,
                      tooltip: _revealed == pair.key ? l.externalVaultHide : l.externalVaultShow,
                      icon: _revealed == pair.key ? Icons.visibility_off_outlined : Icons.visibility_outlined,
                      onPressed: _busy
                          ? null
                          : () => setState(() {
                              _hide?.cancel();
                              _revealed = _revealed == pair.key ? null : pair.key;
                              _hide = Timer(const Duration(seconds: 20), () {
                                if (mounted) setState(() => _revealed = null);
                              });
                            }),
                    ),
                    GlassIconButton(
                      size: GlassControlSize.md.height,
                      tooltip: l.commonCopy,
                      icon: Icons.copy_outlined,
                      onPressed: _busy
                          ? null
                          : () => _run(() async {
                              final editing = _data;
                              await copySecretWithNotice(
                                context,
                                ref,
                                _value(pair.value),
                                isCurrent: () => _current && _data == editing,
                              );
                            }),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    ],
  );

  String _value(Object? value) => value is String ? value : jsonEncode(value);

  Widget _editor(AppLocalizations l) => SingleChildScrollView(
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      spacing: GlassSpacing.s8,
      children: [
        if (_secretPath == null) _field(_path, l.externalVaultPath) else Text(_secretPath!),
        RevealedSecretText(
          controller: _json,
          child: TextField(
            controller: _json,
            enabled: !_busy,
            maxLines: 10,
            autocorrect: false,
            enableSuggestions: false,
            enableIMEPersonalizedLearning: false,
            decoration: InputDecoration(labelText: l.externalVaultJson),
          ),
        ),
        Wrap(
          spacing: GlassSpacing.toolbarItemGap,
          runSpacing: GlassSpacing.s8,
          children: [
            GlassButton.prominent(label: l.commonSave, onPressed: _busy ? null : _write),
            GlassButton.plain(
              label: l.commonCancel,
              onPressed: _busy
                  ? null
                  : () => setState(() {
                      _editing = false;
                      _json.clear();
                      _error = null;
                      _path.clear();
                    }),
            ),
          ],
        ),
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final connections = ref.watch(externalVaultConnectionsProvider);
    final child = !_current
        ? const SizedBox.shrink()
        : _configuring
        ? _configForm(l)
        : _editing
        ? _editor(l)
        : _client != null
        ? LayoutBuilder(
            builder: (context, constraints) {
              if (_data == null) return _browser(l);
              if (constraints.maxWidth < 800) return _detail(l);
              return Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                spacing: GlassSpacing.s12,
                children: [
                  Expanded(child: _browser(l)),
                  const VerticalDivider(width: 1),
                  Expanded(child: _detail(l)),
                ],
              );
            },
          )
        : ListView(
            children: [
              for (final connection in connections)
                ListTile(
                  key: ValueKey('external-vault-${connection.id}'),
                  leading: const Icon(Icons.shield_outlined),
                  title: Text(connection.name),
                  subtitle: Text(connection.endpoint.host),
                  onTap: _busy ? null : () => setState(() => _configure(connection)),
                ),
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: GlassButton.prominent(
                  label: l.externalVaultAdd,
                  icon: Icons.add,
                  onPressed: _busy ? null : () => setState(() => _configure(null)),
                ),
              ),
            ],
          );
    final workspace = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      spacing: GlassSpacing.s8,
      children: [
        if (_busy) const LinearProgressIndicator(),
        if (_error case final error?)
          Text(_errorText(l, error), style: TextStyle(color: Theme.of(context).colorScheme.error)),
        Expanded(child: child),
      ],
    );
    if (widget.onClose case final onClose?) {
      final connection = _client?.connection;
      return Padding(
        key: const ValueKey('external-vault-workspace'),
        padding: const EdgeInsets.all(GlassSpacing.s16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: GlassSpacing.s12,
          children: [
            Wrap(
              spacing: GlassSpacing.toolbarItemGap,
              runSpacing: GlassSpacing.s8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                GlassButton.plain(label: l.navPasswords, icon: Icons.arrow_back, onPressed: onClose),
                Text(l.externalVaultTitle, style: Theme.of(context).textTheme.titleMedium),
                if (connection != null) Text(connection.name, maxLines: 1, overflow: TextOverflow.ellipsis),
                if (connection?.endpoint.scheme == 'http') const Text('HTTP'), // l10n-ignore: protocol name.
              ],
            ),
            Expanded(
              child: connection != null
                  ? workspace
                  : Align(
                      alignment: AlignmentDirectional.topStart,
                      child: ConstrainedBox(constraints: const BoxConstraints(maxWidth: 660), child: workspace),
                    ),
            ),
          ],
        ),
      );
    }
    return GlassDialog(
      title: l.externalVaultTitle,
      icon: Icons.shield_outlined,
      width: 660,
      content: SizedBox(height: min(MediaQuery.sizeOf(context).height * .60, 580), child: workspace),
      primaryAction: GlassButton(label: l.commonClose, onPressed: () => Navigator.of(context).pop()),
    );
  }
}
