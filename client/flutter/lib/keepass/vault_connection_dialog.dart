import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
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

// The workspace owns the authenticated client. A palette opened over it may
// read the same connection, with its own mount and navigation position.
typedef _OpenVault = ({ExternalVaultSession client, List<ExternalVaultMount> mounts});

enum _VaultSignIn { manual, device, synced }

final _openVaultsProvider = Provider<Map<String, _OpenVault>>((ref) {
  ref.watch(passwordManagerProvider);
  return {};
});

class ExternalVaultWorkspace extends ConsumerStatefulWidget {
  const ExternalVaultWorkspace({super.key, this.id, this.onClose, this.onCopied});
  final String? id;
  final VoidCallback? onClose;
  final VoidCallback? onCopied;

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
  late final Map<String, _OpenVault> _openVaults;
  final _folders = <String, List<String>>{};
  bool _borrowed = false;
  bool get _picking => widget.onCopied != null;
  ExternalVaultMount? _pickerMount;
  ExternalVaultMount get _activeMount => _pickerMount ?? _client!.mount;
  List<ExternalVaultMount> _mounts = [];
  String _folder = '';
  String? _secretPath, _error, _listError, _revealed;
  Timer? _hide;
  bool _configuring = false;
  bool _editing = false;
  bool _versioned = true;
  bool _rememberCredential = false;
  bool _syncCredential = false;
  bool _verified = false;
  bool _rememberOnAccess = false;
  bool _busy = false;

  bool get _current => mounted && _owner.valid;

  @override
  void initState() {
    super.initState();
    _openVaults = ref.read(_openVaultsProvider);
    _owner.validity.addListener(_expire);
    if (widget.id != null) {
      final target = ref.read(externalVaultConnectionsProvider).where((item) => item.id == widget.id).firstOrNull;
      if (target != null) {
        _configure(target);
        final open = _openVaults[target.binding];
        if (_picking && open != null && !open.client.closed) {
          _client = open.client;
          _borrowed = true;
          _rememberOnAccess = true;
          _verified = target.auth == ExternalVaultAuth.userpass;
          _pickerMount = open.client.mount;
          _mounts = open.mounts;
          _configuring = false;
          unawaited(_run(() => _browse('')));
        }
      }
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
    final client = _client;
    if (client != null && !_borrowed) {
      final binding = client.connection.binding;
      if (identical(_openVaults[binding]?.client, client)) _openVaults.remove(binding);
      client.close();
    }
    _borrowed = false;
    _pickerMount = null;
    _client = null;
    _folders.clear();
    _mounts = [];
    _verified = false;
    _rememberOnAccess = false;
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
    _syncCredential = false;
    _configuring = true;
    _error = null;
  }

  ExternalVaultConnection _configuration(String id, String profile, String vault) => ExternalVaultConnection(
    id: id,
    profile: profile,
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

  ExternalVaultConnection? _matchingSavedConnection() {
    final selected = _selected;
    if (selected == null) return null;
    try {
      final form = _configuration(selected.id, selected.profile, selected.vault);
      if (form.binding != selected.binding) return null;
      return ref.read(externalVaultConnectionsProvider).where((item) => item.binding == form.binding).firstOrNull;
    } on ExternalVaultFailure {
      return null;
    }
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

  Future<void> _rememberConfirmedAccess(ExternalVaultSession client) async {
    if (!_rememberOnAccess || !_verified || !_current || _client != client) return;
    await _owner.rememberExternalVault(client.connection);
    _rememberOnAccess = false;
  }

  Future<void> _discardCredential(ExternalVaultConnection connection, String? wrapped) async {
    if (!_current || wrapped == null) return;
    try {
      await _store.forget(connection, wrapped);
    } catch (_) {
      // The settings reference has been removed. Its key cannot open another connection.
    }
  }

  Future<void> _connect([_VaultSignIn method = _VaultSignIn.manual]) => _run(() async {
    final vault = ref.read(vaultStatusProvider).value?.vaultId?.value;
    if (vault == null || !_current) throw const ExternalVaultFailure('closed');
    final random = Random.secure();
    final id = _selected?.id ?? base64UrlEncode(List.generate(18, (_) => random.nextInt(256)));
    final configuration = _configuration(id, _owner.profile, vault);
    FocusScope.of(context).unfocus();
    final previous = _selected;
    final saved = _matchingSavedConnection();
    final previousWrapper = saved?.wrapped;
    final SecretText secret;
    switch (method) {
      case _VaultSignIn.manual:
        secret = SecretText(_credential.text);
      case _VaultSignIn.device:
        if (saved == null || previousWrapper == null) throw const ExternalVaultFailure('configuration');
        secret = await _store.unlock(saved, () => _current);
      case _VaultSignIn.synced:
        if (saved == null || saved.credentialId == null) throw const ExternalVaultFailure('configuration');
        secret = await _owner.unlockExternalVault(saved);
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
      if (!_current || _client != client) return;
      _verified = configuration.auth == ExternalVaultAuth.userpass;
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
      final rememberOnDevice = _rememberCredential && method != _VaultSignIn.device;
      final saveForSync = _syncCredential && method != _VaultSignIn.synced;
      if ((rememberOnDevice || saveForSync) && !_verified && configuration.auth == ExternalVaultAuth.token) {
        await client.verifyToken();
        _verified = true;
      }
      String? newWrapper;
      if (rememberOnDevice) {
        if (!await _store.available(configuration)) throw const ExternalVaultFailure('biometric');
        newWrapper = await _store.remember(configuration, secret, () => _current);
      }
      try {
        if (!_current || _client != client) throw const ExternalVaultFailure('closed');
        await _owner.persistExternalVault(
          configuration,
          expected: previous,
          wrapped: newWrapper ?? previousWrapper,
          opened: _verified,
          syncCredential: saveForSync ? secret : null,
        );
      } catch (_) {
        await _discardCredential(configuration, newWrapper);
        rethrow;
      }
      if (previous != null && previous.wrapped != null && (previousWrapper == null || newWrapper != null)) {
        await _discardCredential(previous, previous.wrapped);
      }
      if (!_current) return;
      _configuring = false;
      // A scoped token can read a known path without permission to list the root.
      _rememberOnAccess = !_verified;
      connected = true;
      if (!_picking) _openVaults[configuration.binding] = (client: client, mounts: _mounts);
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
      keys = await client.list(folder, mount: _activeMount);
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
    _folders.removeWhere((parent, _) => parent != '' && parent != folder && !folder.startsWith('$parent/'));
    _folders[folder] = keys;
    _path.clear();
    _search.clear();
    await _rememberConfirmedAccess(client);
  }

  Future<void> _selectMount(ExternalVaultMount mount) => _run(() async {
    final client = _client!;
    final previous = _activeMount;
    if (_borrowed) {
      _pickerMount = mount;
    } else {
      client.selectMount(mount);
    }
    try {
      await _browse('');
    } catch (_) {
      if (_current && _client == client) {
        if (_borrowed) {
          _pickerMount = previous;
        } else {
          client.selectMount(previous);
        }
      }
      rethrow;
    }
  });

  String _underFolder(String name) => _folder.isEmpty ? name : '$_folder/$name';

  Future<void> _open(String path, {String? parent}) async {
    if (path.isEmpty) throw const ExternalVaultFailure('configuration');
    FocusScope.of(context).unfocus();
    final client = _client!;
    final data = await client.read(path, mount: _activeMount);
    if (!_current || client != _client) {
      data.values.clear();
      return;
    }
    _clearDetail();
    if (parent != null && parent != _folder) {
      _folder = parent;
      _folders.removeWhere((key, _) => key != '' && key != parent && !parent.startsWith('$key/'));
      _search.clear();
    }
    _secretPath = path;
    _data = data;
    _verified = true;
    await _rememberConfirmedAccess(client);
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
      _verified = true;
      await _rememberConfirmedAccess(client);
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

  Widget _field(TextEditingController controller, String label, {String? hint}) => TextField(
    controller: controller,
    enabled: !_busy,
    autocorrect: false,
    enableSuggestions: false,
    enableIMEPersonalizedLearning: false,
    onChanged: (_) => setState(() {}),
    decoration: InputDecoration(labelText: label, hintText: hint),
  );

  Widget _configForm(AppLocalizations l) => LayoutBuilder(
    builder: (context, constraints) {
      const gap = GlassSpacing.s16;
      final width = constraints.maxWidth;
      final cardWidth = width >= 760 ? (width - gap) / 2 : width;
      return SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: gap,
          children: [
            Wrap(
              spacing: gap,
              runSpacing: gap,
              children: [
                SizedBox(width: cardWidth, child: _serverSettings(l)),
                SizedBox(width: cardWidth, child: _signInSettings(l)),
              ],
            ),
            if (!_picking) _configurationActions(l),
          ],
        ),
      );
    },
  );

  Widget _serverSettings(AppLocalizations l) => SectionCard(
    key: const ValueKey('external-vault-server-settings'),
    title: l.settingsServerLabel,
    icon: Icons.dns_outlined,
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      spacing: GlassSpacing.s8,
      children: [
        _field(_name, l.keepassEntryTitle),
        _field(_endpoint, l.externalVaultEndpoint, hint: 'https://vault.example.com'),
        if (Uri.tryParse(_endpoint.text.trim())?.scheme == 'http')
          Text(l.externalVaultHttpNotice, style: Theme.of(context).textTheme.bodySmall),
        ExpansionTile(
          title: Text(l.externalVaultApiSettings),
          tilePadding: EdgeInsets.zero,
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
      ],
    ),
  );

  Widget _signInSettings(AppLocalizations l) {
    final saved = _matchingSavedConnection();
    return SectionCard(
      key: const ValueKey('external-vault-sign-in'),
      title: l.loginModeSignIn,
      icon: Icons.key_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: GlassSpacing.s8,
        children: [
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
          CheckboxListTile(
            key: const ValueKey('external-vault-remember-device'),
            value: _rememberCredential,
            contentPadding: EdgeInsets.zero,
            controlAffinity: ListTileControlAffinity.leading,
            title: Text(l.externalVaultRemember),
            onChanged: _busy ? null : (value) => setState(() => _rememberCredential = value!),
          ),
          CheckboxListTile(
            key: const ValueKey('external-vault-sync-login'),
            value: _syncCredential,
            contentPadding: EdgeInsets.zero,
            controlAffinity: ListTileControlAffinity.leading,
            title: Text(l.externalVaultSyncLogin),
            subtitle: Text(l.externalVaultSyncLoginHelp),
            onChanged: _busy ? null : (value) => setState(() => _syncCredential = value!),
          ),
          Wrap(
            spacing: GlassSpacing.toolbarItemGap,
            runSpacing: GlassSpacing.s8,
            children: [
              GlassButton.prominent(label: l.commonConnect, onPressed: _busy ? null : _connect),
              if (saved?.wrapped != null)
                GlassButton(
                  label: l.externalVaultUseSaved,
                  tooltip: l.externalVaultUseSaved,
                  icon: Icons.fingerprint,
                  onPressed: _busy ? null : () => _connect(_VaultSignIn.device),
                ),
              if (saved?.credentialId != null)
                GlassButton(
                  key: const ValueKey('external-vault-use-synced'),
                  label: l.externalVaultUseSynced,
                  icon: Icons.cloud_outlined,
                  onPressed: _busy ? null : () => _connect(_VaultSignIn.synced),
                ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _configurationActions(AppLocalizations l) => Wrap(
    spacing: GlassSpacing.toolbarItemGap,
    runSpacing: GlassSpacing.s8,
    children: [
      GlassButton.plain(
        label: l.commonBack,
        icon: Icons.arrow_back,
        onPressed: _busy
            ? null
            : () => setState(() {
                _configuring = false;
                _error = null;
                _credential.clear();
              }),
      ),
      if (_selected != null)
        GlassButton(
          style: GlassButtonStyle.destructiveQuiet,
          label: l.externalVaultRemoveConnection,
          tooltip: l.externalVaultRemoveConnection,
          onPressed: _busy
              ? null
              : () => _run(() async {
                  final chosen = _selected!;
                  if (!await showConfirmDialog(
                        context,
                        title: l.externalVaultRemoveConnection,
                        message: chosen.synced ? l.externalVaultRemoveSyncedConnection(chosen.name) : chosen.name,
                        confirmLabel: l.commonDelete,
                        destructive: true,
                        secure: true,
                      ) ||
                      !_current) {
                    return;
                  }
                  await _owner.persistExternalVault(chosen, remove: true);
                  await _discardCredential(chosen, chosen.wrapped);
                  _selected = null;
                  _configuring = false;
                  _credential.clear();
                }),
        ),
      if (_matchingSavedConnection() case final saved? when saved.credentialId != null)
        GlassButton(
          key: const ValueKey('external-vault-forget-synced'),
          style: GlassButtonStyle.destructiveQuiet,
          label: l.externalVaultForgetSynced,
          onPressed: _busy
              ? null
              : () => _run(() async {
                  if (!await showConfirmDialog(
                        context,
                        title: l.externalVaultForgetSynced,
                        message: l.externalVaultForgetSyncedHelp,
                        confirmLabel: l.commonDelete,
                        destructive: true,
                        secure: true,
                      ) ||
                      !_current) {
                    return;
                  }
                  final updated = await _owner.forgetExternalVaultLogin(saved);
                  if (!_current) return;
                  _syncCredential = false;
                  _selected = updated;
                }),
        ),
    ],
  );

  void _openSearchPath() {
    final query = _search.text.trim();
    if (query.isEmpty) return;
    final path = query.contains('/') ? query : _underFolder(query);
    unawaited(_run(() => path.endsWith('/') ? _browse(path.substring(0, path.length - 1)) : _open(path)));
  }

  Widget _browser(AppLocalizations l) {
    final path = vaultPath(_folder, optional: true);
    final query = _search.text.trim().toLowerCase();
    return LayoutBuilder(
      builder: (context, constraints) {
        final wide = constraints.maxWidth >= 680 && !_picking;
        if (!wide && _data != null) return _detail(l);
        final paths = <String>[
          if (wide && _folders.containsKey('')) '',
          if (wide)
            for (var i = 1; i < path.length; i++)
              if (_folders.containsKey(path.take(i).join('/'))) path.take(i).join('/'),
          if (_folder.isNotEmpty || !wide || !_folders.containsKey('')) _folder,
        ];
        // Keep the newest columns in view. The path bar can return to any ancestor.
        final capacity = max(1, min(4, (constraints.maxWidth / 225).floor()) - (_data == null ? 0 : 1));
        final visible = paths.skip(max(0, paths.length - capacity));
        final columns = [
          for (final folder in visible) _folderColumn(l, folder, filter: folder == _folder ? query : ''),
          if (_data != null) _detail(l),
        ];
        final actions = Wrap(
          spacing: GlassSpacing.toolbarItemGap,
          runSpacing: GlassSpacing.s8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            if (!_picking)
              GlassButton.plain(
                label: l.commonBack,
                icon: Icons.arrow_back,
                onPressed: _busy ? null : () => setState(_closeSession),
              ),
            if (_mounts.length > 1)
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 220),
                child: GlassSelect<ExternalVaultMount>(
                  value: _activeMount,
                  expand: true,
                  semanticLabel: l.externalVaultMount,
                  items: [for (final mount in _mounts) GlassSelectItem(value: mount, label: mount.path)],
                  onChanged: _busy ? null : _selectMount,
                ),
              ),
            if (!_picking)
              GlassButton(
                label: l.externalVaultNewSecret,
                icon: Icons.add,
                onPressed: _busy ? null : () => setState(_edit),
              ),
          ],
        );
        final breadcrumbs = SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            spacing: GlassSpacing.s4,
            children: [
              _folderCrumb(_activeMount.path, '', current: path.isEmpty),
              for (var i = 0; i < path.length; i++) ...[
                const Icon(Icons.chevron_right, size: 16),
                _folderCrumb(path[i], path.take(i + 1).join('/'), current: i == path.length - 1),
              ],
            ],
          ),
        );
        final search = Row(
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
                decoration: InputDecoration(labelText: l.externalVaultSearch, prefixIcon: const Icon(Icons.search)),
                onChanged: (_) => setState(() {}),
                onSubmitted: (_) => _openSearchPath(),
              ),
            ),
            GlassButton(label: l.commonOpen, onPressed: _busy || query.isEmpty ? null : _openSearchPath),
          ],
        );
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: GlassSpacing.s8,
          children: [
            ConstrainedBox(
              constraints: BoxConstraints(maxHeight: constraints.maxHeight * .55),
              child: ContentSurface(
                padding: const EdgeInsets.all(GlassSpacing.s8),
                child: SingleChildScrollView(
                  primary: false,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    spacing: GlassSpacing.s8,
                    children: [
                      if (wide)
                        Row(
                          spacing: GlassSpacing.s8,
                          children: [
                            Expanded(child: breadcrumbs),
                            Flexible(
                              child: Align(alignment: AlignmentDirectional.centerEnd, child: actions),
                            ),
                          ],
                        )
                      else ...[
                        if (!_picking || _mounts.length > 1) actions,
                        breadcrumbs,
                      ],
                      search,
                    ],
                  ),
                ),
              ),
            ),
            if (_listError case final error?) Text(_errorText(l, error)),
            Expanded(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  for (final (index, column) in columns.indexed) ...[
                    if (index != 0) const SizedBox(width: GlassSpacing.s8),
                    Expanded(
                      child: Material(type: MaterialType.transparency, clipBehavior: Clip.hardEdge, child: column),
                    ),
                  ],
                ],
              ),
            ),
          ],
        );
      },
    );
  }

  Widget _folderColumn(AppLocalizations l, String folder, {required String filter}) {
    final tokens = GlassTokens.of(context);
    final palette = tokens.palette;
    final entries = (_folders[folder] ?? const <String>[])
        .where((entry) => entry.toLowerCase().contains(filter))
        .toList();
    if (entries.isEmpty && _listError != null && folder == _folder) return const SizedBox.shrink();
    final name = folder.isEmpty ? _activeMount.path : folder.split('/').last;
    final tint = palette.accentFill.withValues(alpha: tokens.isDark ? .18 : .10);
    return Align(
      alignment: AlignmentDirectional.topStart,
      child: ContentSurface(
        padding: const EdgeInsets.all(GlassSpacing.s4),
        child: ListView.separated(
          key: PageStorageKey(('external-vault-column', _activeMount.path, folder)),
          primary: false,
          shrinkWrap: true,
          padding: EdgeInsets.zero,
          itemCount: 1 + max(entries.length, 1),
          separatorBuilder: (_, index) => Padding(
            padding: EdgeInsetsDirectional.only(
              start: index == 0 ? GlassSpacing.s8 : GlassSpacing.s48,
              end: GlassSpacing.s8,
            ),
            child: Divider(height: 1, thickness: 1, color: tokens.surfaces.separator),
          ),
          itemBuilder: (context, index) {
            if (index == 0) {
              return Padding(
                padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12, vertical: GlassSpacing.s12),
                child: Semantics(
                  header: true,
                  child: Text(
                    l.externalVaultFolderContents(name),
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: tokens.typography.title3.copyWith(color: palette.label),
                  ),
                ),
              );
            }
            if (entries.isEmpty) {
              return Padding(
                padding: const EdgeInsets.all(GlassSpacing.s12),
                child: Text(
                  filter.isEmpty ? l.externalVaultEmpty : l.externalVaultNoMatches,
                  style: tokens.typography.callout.copyWith(color: palette.secondary),
                ),
              );
            }
            final key = entries[index - 1];
            final directory = key.endsWith('/');
            final label = directory ? key.substring(0, key.length - 1) : key;
            final path = folder.isEmpty ? label : '$folder/$label';
            final selected = directory ? (_folder == path || _folder.startsWith('$path/')) : _secretPath == path;
            return ListTile(
              key: ValueKey('external-vault-item-$key'),
              dense: true,
              minTileHeight: GlassSpacing.s48,
              contentPadding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
              minLeadingWidth: GlassSpacing.s32,
              horizontalTitleGap: GlassSpacing.s8,
              selected: selected,
              selectedColor: palette.label,
              selectedTileColor: tint,
              shape: GlassRadii.shape(tokens.radii.row),
              leading: GlassFill(
                shape: GlassRadii.shape(tokens.radii.sm),
                color: directory ? tint : tokens.surfaces.inset,
                child: SizedBox.square(
                  dimension: GlassSpacing.s32,
                  child: Icon(
                    directory ? (selected ? Icons.folder_open_rounded : Icons.folder_rounded) : Icons.key_rounded,
                    size: GlassSizes.iconToolbar,
                    color: directory ? palette.accent : palette.secondary,
                    semanticLabel: directory ? l.sftpKindFolder : l.copyWhatSecret,
                  ),
                ),
              ),
              trailing: directory
                  ? Icon(Icons.chevron_right, size: GlassSizes.iconRow, color: palette.secondary)
                  : null,
              title: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: tokens.typography.body.copyWith(
                  color: palette.label,
                  fontWeight: selected ? FontWeight.w600 : null,
                ),
              ),
              onTap: _busy ? null : () => _run(() => directory ? _browse(path) : _open(path, parent: folder)),
            );
          },
        ),
      ),
    );
  }

  Widget _folderCrumb(String label, String path, {required bool current}) {
    final tokens = GlassTokens.of(context);
    final icon = path.isEmpty ? Icons.storage_rounded : null;
    if (!current) {
      return GlassButton.plain(
        label: label,
        icon: icon,
        tooltip: path.isEmpty ? label : path,
        onPressed: _busy ? null : () => _run(() => _browse(path)),
      );
    }
    return Semantics(
      key: ValueKey(('external-vault-current-crumb', path)),
      selected: true,
      child: GlassFill(
        shape: GlassRadii.shape(tokens.radii.sm),
        color: tokens.palette.accentFill.withValues(alpha: tokens.isDark ? .18 : .10),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8, vertical: GlassSpacing.s6),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            spacing: GlassSpacing.s6,
            children: [
              if (icon != null) Icon(icon, size: GlassSizes.iconMenu, color: tokens.palette.label),
              Text(
                label,
                style: tokens.typography.callout.copyWith(color: tokens.palette.label, fontWeight: FontWeight.w600),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Future<void> _copyField(String key) => _run(() async {
    final data = _data;
    final client = _client;
    if (data == null || client == null || client.closed || !data.values.containsKey(key)) return;
    await copySecretWithNotice(
      context,
      ref,
      _value(data.values[key]),
      onCopied: widget.onCopied,
      isCurrent: () => _current && !client.closed && identical(_data, data) && identical(_client, client),
    );
  });

  Widget _detail(AppLocalizations l) {
    final tokens = GlassTokens.of(context);
    final text = Theme.of(context).textTheme;
    final path = _secretPath!;
    final split = path.lastIndexOf('/') + 1;
    final fields = _data!.values;
    final keys = fields.keys.toList(growable: false);
    final header = Padding(
      padding: const EdgeInsets.all(GlassSpacing.s16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: GlassSpacing.s12,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            spacing: GlassSpacing.s8,
            children: [
              Icon(Icons.key_outlined, size: GlassSizes.iconToolbar, color: tokens.palette.accent),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  spacing: GlassSpacing.s4,
                  children: [
                    Semantics(header: true, child: Text(path.substring(split), style: text.titleMedium)),
                    Text(
                      split == 0 ? _activeMount.path : '${_activeMount.path}/${path.substring(0, split - 1)}',
                      style: text.bodySmall?.copyWith(color: tokens.secondaryLabel),
                    ),
                  ],
                ),
              ),
            ],
          ),
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
              if (!_picking) GlassButton(label: l.commonEdit, onPressed: _busy ? null : () => setState(_edit)),
              if (!_picking)
                GlassButton(
                  label: l.commonDelete,
                  style: GlassButtonStyle.destructiveQuiet,
                  onPressed: _busy ? null : _delete,
                ),
            ],
          ),
        ],
      ),
    );
    return ContentSurface(
      key: const ValueKey('external-vault-secret-details'),
      padding: EdgeInsets.zero,
      child: ListView.separated(
        key: ValueKey(path),
        primary: false,
        padding: EdgeInsets.zero,
        itemCount: keys.length + 1,
        separatorBuilder: (_, index) => Divider(
          color: tokens.surfaces.separator,
          height: 1,
          indent: index == 0 ? 0 : GlassSpacing.s16,
          endIndent: index == 0 ? 0 : GlassSpacing.s16,
        ),
        itemBuilder: (context, index) {
          if (index == 0) return header;
          final key = keys[index - 1];
          final revealed = !_picking && _revealed == key;
          return InkWell(
            key: ValueKey('external-vault-field-$key'),
            onTap: !_picking || _busy ? null : () => _copyField(key),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s16, vertical: GlassSpacing.s12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                spacing: GlassSpacing.s4,
                children: [
                  Row(
                    spacing: GlassSpacing.toolbarItemGap,
                    children: [
                      Expanded(child: Text(key, style: text.titleSmall)),
                      if (!_picking)
                        GlassIconButton(
                          size: GlassControlSize.md.height,
                          tooltip: revealed ? l.externalVaultHide : l.externalVaultShow,
                          icon: revealed ? Icons.visibility_off_outlined : Icons.visibility_outlined,
                          onPressed: _busy
                              ? null
                              : () => setState(() {
                                  _hide?.cancel();
                                  _revealed = revealed ? null : key;
                                  _hide = Timer(const Duration(seconds: 20), () {
                                    if (mounted) setState(() => _revealed = null);
                                  });
                                }),
                        ),
                      GlassIconButton(
                        size: GlassControlSize.md.height,
                        tooltip: l.commonCopy,
                        icon: Icons.copy_outlined,
                        onPressed: _busy ? null : () => _copyField(key),
                      ),
                    ],
                  ),
                  if (revealed)
                    RevealedSecret(child: SelectableText(_value(fields[key]), style: tokens.typography.mono))
                  else
                    Text('••••••••', style: tokens.typography.mono.copyWith(color: tokens.secondaryLabel)),
                ],
              ),
            ),
          );
        },
      ),
    );
  }

  String _value(Object? value) => value is String ? value : jsonEncode(value);

  Widget _editor(AppLocalizations l) => SingleChildScrollView(
    child: SectionCard(
      key: const ValueKey('external-vault-secret-editor'),
      title: _secretPath ?? l.externalVaultNewSecret,
      icon: Icons.edit_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: GlassSpacing.s8,
        children: [
          if (_secretPath == null) _field(_path, l.externalVaultPath),
          RevealedSecretText(
            controller: _json,
            child: TextField(
              controller: _json,
              enabled: !_busy,
              minLines: 5,
              maxLines: 12,
              autocorrect: false,
              enableSuggestions: false,
              enableIMEPersonalizedLearning: false,
              style: GlassTokens.of(context).typography.mono,
              decoration: InputDecoration(labelText: l.externalVaultJson, alignLabelWithHint: true),
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
    ),
  );

  Widget _connectionsList(AppLocalizations l, List<ExternalVaultConnection> connections) {
    final tokens = GlassTokens.of(context);
    final text = Theme.of(context).textTheme;
    return LayoutBuilder(
      builder: (context, bounds) {
        const gap = GlassSpacing.s12;
        final columns = min(max(1, connections.length), max(1, ((bounds.maxWidth + gap) / (340 + gap)).floor()));
        final rows = (connections.length / columns).ceil();
        return ListView.separated(
          key: const PageStorageKey('external-vault-connections'),
          padding: const EdgeInsets.all(GlassSpacing.s4),
          itemCount: 1 + max(1, rows),
          separatorBuilder: (_, i) => SizedBox(height: i == 0 ? GlassSpacing.s16 : gap),
          itemBuilder: (context, row) {
            if (row == 0) {
              return Wrap(
                alignment: WrapAlignment.spaceBetween,
                crossAxisAlignment: WrapCrossAlignment.center,
                spacing: GlassSpacing.s16,
                runSpacing: GlassSpacing.s8,
                children: [
                  Semantics(header: true, child: Text(l.externalVaultConnections, style: text.titleLarge)),
                  GlassButton.prominent(
                    label: l.externalVaultAdd,
                    icon: Icons.add,
                    onPressed: _busy ? null : () => setState(() => _configure(null)),
                  ),
                ],
              );
            }
            if (connections.isEmpty) {
              return Text(
                l.externalVaultNoConnections,
                style: text.bodyMedium?.copyWith(color: tokens.palette.secondary),
              );
            }
            final first = (row - 1) * columns;
            return Row(
              spacing: gap,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (var column = 0; column < columns; column++)
                  Expanded(
                    child: first + column < connections.length
                        ? _connectionCard(l, connections[first + column])
                        : const SizedBox.shrink(),
                  ),
              ],
            );
          },
        );
      },
    );
  }

  Widget _connectionCard(AppLocalizations l, ExternalVaultConnection connection) {
    final tokens = GlassTokens.of(context);
    final palette = tokens.palette;
    final text = Theme.of(context).textTheme;
    final origin = connection.endpoint.origin;
    final auth = connection.auth == ExternalVaultAuth.userpass ? l.externalVaultUserpass : l.externalVaultToken;
    final identity = Row(
      spacing: GlassSpacing.s12,
      children: [
        GlassFill(
          shape: GlassRadii.shape(tokens.radii.card),
          color: palette.accent.withValues(alpha: tokens.isDark ? .18 : .10),
          child: SizedBox.square(
            dimension: GlassSpacing.s40,
            child: Icon(Icons.shield_outlined, color: palette.accent),
          ),
        ),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            spacing: GlassSpacing.s4,
            children: [
              Tooltip(
                message: connection.name,
                child: Text(connection.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: text.titleMedium),
              ),
              Tooltip(
                message: origin,
                child: Text(
                  origin,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: text.bodySmall?.copyWith(color: palette.secondary),
                ),
              ),
            ],
          ),
        ),
      ],
    );
    final method = Tooltip(
      message: l.externalVaultAuth,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        spacing: GlassSpacing.s6,
        children: [
          Icon(Icons.key_outlined, size: GlassSizes.iconMenu, color: palette.secondary),
          Flexible(
            child: Text(
              auth,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: text.bodySmall?.copyWith(color: palette.secondary),
            ),
          ),
        ],
      ),
    );
    final open = Row(
      mainAxisSize: MainAxisSize.min,
      spacing: GlassSpacing.s4,
      children: [
        Text(l.commonOpen, style: text.labelLarge?.copyWith(color: palette.accent)),
        Icon(Icons.arrow_forward, size: GlassSizes.iconMenu, color: palette.accent),
      ],
    );
    return GlassInteractive(
      key: ValueKey('external-vault-${connection.id}'),
      onPressed: _busy ? null : () => setState(() => _configure(connection)),
      builder: (context, state) => GlassFocusRing(
        visible: state.focusVisible,
        shape: GlassRadii.shape(tokens.radii.card),
        child: ContentSurface(
          color: state.pressed
              ? tokens.surfaces.fillPressed
              : state.hovered
              ? tokens.surfaces.fillHover
              : null,
          child: LayoutBuilder(
            builder: (context, bounds) {
              if (bounds.maxWidth >= 600) {
                return Row(
                  spacing: GlassSpacing.s24,
                  children: [
                    Expanded(flex: 2, child: identity),
                    Expanded(
                      child: Align(alignment: AlignmentDirectional.centerStart, child: method),
                    ),
                    open,
                  ],
                );
              }
              return Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                spacing: GlassSpacing.s16,
                children: [
                  identity,
                  Wrap(
                    alignment: WrapAlignment.spaceBetween,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    spacing: GlassSpacing.s12,
                    runSpacing: GlassSpacing.s8,
                    children: [method, open],
                  ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }

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
        ? _browser(l)
        : _connectionsList(l, connections);
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
                GlassButton.plain(
                  label: _picking ? l.commonBack : l.navPasswords,
                  icon: Icons.arrow_back,
                  onPressed: onClose,
                ),
                Text(l.externalVaultTitle, style: Theme.of(context).textTheme.titleMedium),
                if (connection != null) Text(connection.name, maxLines: 1, overflow: TextOverflow.ellipsis),
                if (connection?.endpoint.scheme == 'http') const Text('HTTP'), // l10n-ignore: protocol name.
              ],
            ),
            Expanded(child: workspace),
          ],
        ),
      );
    }
    return GlassDialog(
      title: l.externalVaultTitle,
      icon: Icons.shield_outlined,
      width: 1040,
      content: SizedBox(height: min(MediaQuery.sizeOf(context).height * .60, 580), child: workspace),
      primaryAction: GlassButton(label: l.commonClose, onPressed: () => Navigator.of(context).pop()),
    );
  }
}
