import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/color_label_picker.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/db/db_errors.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DbConnectionDialog extends ConsumerStatefulWidget {
  const DbConnectionDialog({this.connection, super.key});
  final DbConnection? connection;
  @override
  ConsumerState<DbConnectionDialog> createState() => _DbConnectionDialogState();
}

class _DbConnectionDialogState extends ConsumerState<DbConnectionDialog> with WidgetsBindingObserver {
  final _form = GlobalKey<FormState>();
  final _fields = <String, TextEditingController>{};
  late final DbScope _scope;
  late final DbService? _service;
  late DbEngine _engine;
  late DbTlsMode _tls;
  bool _tlsChosen = false, _tlsAuto = false;
  Object? _error;
  late DbAuthMechanism _mechanism;
  late DbMongoReadPreference _readPreference;
  late bool _production, _resp3, _srv;
  bool _savePassword = true, _clearPassword = false, _busy = false, _failed = false;
  String? _group;
  DbRoute _route = const DbRoute();
  DbConnectionChoices? _choices;
  DbConnectionCheck? _check;
  DbConnectionDraft? _pendingDraft;

  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  TextEditingController _field(String key) => _fields[key]!;
  String _value(String key) => _field(key).text.trim();

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _scope = ref.read(dbScopeProvider);
    _service = ref.read(dbServiceProvider);
    final c = widget.connection;
    _engine = c?.engine ?? DbEngine.postgresql;
    _tls = c?.tls.mode ?? DbTlsMode.verify;
    _mechanism = c?.auth.mechanism ?? DbAuthMechanism.defaultAuth;
    _production = c?.production ?? true;
    _resp3 = c?.options.resp3 ?? false;
    _srv = c?.options.srv ?? false;
    _readPreference = c?.options.readPreference ?? DbMongoReadPreference.secondaryPreferred;
    _route = c?.route ?? const DbRoute();
    _tlsChosen = c != null;
    _group = c?.groupId;
    final values = {
      'name': c?.name ?? '',
      'host': c?.host ?? '',
      'port': '${c?.port ?? _engine.port}',
      'database': c?.database ?? '',
      'username': c?.username ?? '',
      'password': '',
      'key': '',
      'ca': c?.tls.certificateAuthorityPem ?? '',
      'serverName': c?.tls.serverName ?? '',
      'certificate': c?.auth.certificatePem ?? '',
      'tags': c?.tags.join(', ') ?? '',
      'color': c?.color ?? '',
      'rows': '${c?.limits.maxRows ?? 1000}',
      'statement': '${c?.limits.statementTimeoutSeconds ?? 60}',
      'connect': '${c?.limits.connectTimeoutSeconds ?? 10}',
      'memory': '${(c?.limits.resultMemoryBytes ?? 67108864) ~/ 1048576}',
      'application': c?.options.applicationName ?? 'ConsoleCrypt',
      'searchPath': c?.options.searchPath.join(', ') ?? '',
      'charset': c?.options.charset ?? 'utf8mb4',
      'authSource': c?.options.authSource ?? '',
      'replicaSet': c?.options.replicaSet ?? '',
      for (final key in const ['max_threads', 'max_memory_usage', 'max_bytes_to_read', 'max_rows_to_read'])
        key: c?.options.settings[key]?.toString() ?? '',
    };
    for (final entry in values.entries) {
      _fields[entry.key] = TextEditingController(text: entry.value);
    }
    // A port-forward or local database usually has no TLS: unless the user
    // picked a mode, loopback hosts start with "prefer".
    _field('host').addListener(() {
      if (_tlsChosen || !mounted) return;
      final local = dbLoopbackHost(_value('host'));
      final wanted = local ? DbTlsMode.prefer : DbTlsMode.verify;
      if (wanted != _tls || local != _tlsAuto) {
        setState(() {
          _tls = wanted;
          _tlsAuto = local;
        });
      }
    });
    unawaited(_loadChoices());
  }

  Future<void> _loadChoices() async {
    try {
      final choices = await _service?.connectionChoices();
      if (_current) setState(() => _choices = choices);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  void _wipeFields() {
    _field('password').clear();
    _field('key').clear();
    _pendingDraft?.wipe();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.hidden || state == AppLifecycleState.paused) _wipeFields();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _wipeFields();
    for (final controller in _fields.values) {
      controller.clear();
      controller.dispose();
    }
    super.dispose();
  }

  DbConnectionDraft _draft() {
    final old = widget.connection;
    String? nullable(String key) => _value(key).isEmpty ? null : _value(key);
    int number(String key) => int.tryParse(_value(key)) ?? 0;
    final password = _field('password').text;
    final key = _field('key').text;
    return DbConnectionDraft(
      metadata: DbConnection(
        id: old?.id ?? '',
        name: _value('name'),
        engine: _engine,
        host: _value('host'),
        port: number('port'),
        database: _value('database'),
        username: _value('username'),
        production: _production,
        auth: DbAuth(
          passwordSecretId: _clearPassword ? null : old?.auth.passwordSecretId,
          certificatePem: nullable('certificate'),
          keySecretId: old?.auth.keySecretId,
          mechanism: _mechanism,
        ),
        tls: DbTls(mode: _tls, certificateAuthorityPem: nullable('ca'), serverName: nullable('serverName')),
        route: _route,
        groupId: _group,
        tags: _value('tags').split(',').map((s) => s.trim()).where((s) => s.isNotEmpty).toSet().toList(),
        color: nullable('color'),
        limits: DbLimits(
          maxRows: number('rows'),
          statementTimeoutSeconds: number('statement'),
          connectTimeoutSeconds: number('connect'),
          resultMemoryBytes: number('memory') * 1048576,
        ),
        options: DbDriverOptions(
          engine: _engine,
          applicationName: _value('application'),
          charset: _value('charset'),
          searchPath: _value('searchPath').split(',').map((s) => s.trim()).where((s) => s.isNotEmpty).toList(),
          settings: {
            for (final key in const ['max_threads', 'max_memory_usage', 'max_bytes_to_read', 'max_rows_to_read'])
              if (_value(key).isNotEmpty) key: number(key),
          },
          resp3: _resp3,
          authSource: nullable('authSource'),
          replicaSet: nullable('replicaSet'),
          srv: _srv,
          readPreference: _readPreference,
        ),
        createdAt: old?.createdAt,
        updatedAt: old?.updatedAt,
      ),
      password: password.isEmpty ? null : SecretText(password),
      privateKey: key.isEmpty ? null : SecretText(key),
      savePassword: _savePassword,
      clearPassword: _clearPassword,
      clearCertificate: old?.auth.certificatePem != null && nullable('certificate') == null,
    );
  }

  Future<void> _submit({bool test = false}) async {
    if (!_current || _busy || _service == null || !_form.currentState!.validate()) return;
    final draft = _draft();
    _pendingDraft = draft;
    setState(() {
      _busy = true;
      _failed = false;
      _check = null;
    });
    try {
      if (test) {
        final result = await _service.testConnection(draft);
        if (_current) setState(() => _check = result);
      } else {
        final saved = await _service.saveConnection(draft);
        if (mounted && _current) {
          _wipeFields();
          ref.invalidate(dbConnectionsProvider(_scope));
          closeDialog(context, saved);
        }
      }
    } catch (error) {
      if (_current) {
        setState(() {
          _failed = true;
          _error = error;
        });
      }
    } finally {
      draft.wipe();
      if (identical(_pendingDraft, draft)) _pendingDraft = null;
      if (_current) setState(() => _busy = false);
    }
  }

  Widget _input(
    String key,
    String label, {
    bool required = false,
    int? min,
    int? max,
    int lines = 1,
    bool secret = false,
  }) {
    String? validate(String? raw) {
      final value = raw?.trim() ?? '', l = context.l10n;
      if (required && value.isEmpty) return l.dbRequired;
      if (min != null && (int.tryParse(value) == null || int.parse(value) < min || int.parse(value) > max!)) {
        return l.dbInvalid;
      }
      if (value.isNotEmpty &&
          key == 'host' &&
          (value.contains(RegExp(r'[\s/@?#\[\]\\]')) ||
              (value.contains(':') && InternetAddress.tryParse(value) == null))) {
        return l.dbInvalid;
      }
      if (value.isNotEmpty && key == 'color' && !RegExp(r'^#[0-9a-fA-F]{6}$').hasMatch(value)) return l.dbInvalid;
      if (_engine == DbEngine.redis && key == 'database' && int.tryParse(value) == null) return l.dbInvalid;
      if (key == 'certificate' &&
          value.isNotEmpty &&
          _value('key').isEmpty &&
          widget.connection?.auth.keySecretId == null) {
        return l.dbPrivateKey;
      }
      return null;
    }

    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: secret && lines == 1 && key != 'key'
          ? SecretField(
              key: ValueKey('db-field-$key'),
              controller: _field(key),
              label: label,
              enabled: !_busy,
              validator: validate,
            )
          : TextFormField(
              key: ValueKey('db-field-$key'),
              controller: _field(key),
              enabled: !_busy,
              maxLines: lines,
              autocorrect: false,
              enableSuggestions: false,
              enableIMEPersonalizedLearning: false,
              obscureText: secret,
              keyboardType: min == null
                  ? (lines > 1 ? TextInputType.multiline : TextInputType.text)
                  : TextInputType.number,
              decoration: InputDecoration(labelText: label),
              validator: validate,
            ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        _wipeFields();
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    final choices = _choices;
    return GlassDialog(
      title: widget.connection == null ? l.dbNewConnection : l.commonEdit,
      content: SizedBox(
        width: 680,
        child: Form(
          key: _form,
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                if (_busy) const LinearProgressIndicator(),
                if (_service == null) Text(l.dbUnavailable),
                if (_failed)
                  Text(
                    _error == null ? l.dbFailed : dbFailureText(_error!, l),
                    style: TextStyle(color: Theme.of(context).colorScheme.error),
                  ),
                if (_check != null) ...[Text(l.dbTestSuccess), Text(_check!.serverVersion)],
                _input('name', l.dbName, required: true),
                DropdownButtonFormField<DbEngine>(
                  key: const ValueKey('db-engine'),
                  initialValue: _engine,
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.dbEngine),
                  items: [
                    for (final engine in DbEngine.values) DropdownMenuItem(value: engine, child: Text(engine.label)),
                  ],
                  onChanged: _busy || widget.connection != null
                      ? null
                      : (value) {
                          if (value == null) return;
                          setState(() {
                            _engine = value;
                            _field('port').text = '${value.port}';
                            _field('database').text = value == DbEngine.redis ? '0' : '';
                            _mechanism = DbAuthMechanism.defaultAuth;
                          });
                        },
                ),
                const SizedBox(height: 12),
                _input('host', l.dbHost, required: true),
                _input('port', l.dbPort, min: 1, max: 65535),
                _input('database', l.dbDatabase, required: _engine == DbEngine.redis),
                _input('username', l.dbUsername),
                _input('password', l.dbPassword, secret: true),
                if (widget.connection?.auth.passwordSecretId != null) ...[
                  Text(l.dbSavedPassword),
                  CheckboxListTile(
                    value: _clearPassword,
                    title: Text(l.dbClearPassword),
                    onChanged: _busy ? null : (v) => setState(() => _clearPassword = v!),
                  ),
                ],
                CheckboxListTile(
                  value: _savePassword,
                  title: Text(l.dbSavePassword),
                  onChanged: _busy ? null : (v) => setState(() => _savePassword = v!),
                ),
                CheckboxListTile(
                  key: const ValueKey('db-production'),
                  value: _production,
                  title: Text(l.dbProduction),
                  subtitle: Text(l.dbProductionHelp),
                  onChanged: _busy ? null : (v) => setState(() => _production = v!),
                ),
                ExpansionTile(
                  title: Text(l.dbTls),
                  initiallyExpanded: false,
                  children: [
                    DropdownButtonFormField<DbTlsMode>(
                      key: const ValueKey('db-tls'),
                      initialValue: _tls,
                      isExpanded: true,
                      items: [
                        for (final mode in DbTlsMode.values)
                          DropdownMenuItem(
                            value: mode,
                            child: Text(switch (mode) {
                              DbTlsMode.verify => l.dbTlsVerify,
                              DbTlsMode.require => l.dbTlsRequire,
                              DbTlsMode.prefer => l.dbTlsPrefer,
                              DbTlsMode.disable => l.dbTlsDisable,
                            }),
                          ),
                      ],
                      onChanged: _busy
                          ? null
                          : (v) => setState(() {
                              _tls = v!;
                              _tlsChosen = true;
                              _tlsAuto = false;
                            }),
                    ),
                    if (_tlsAuto) Text(l.dbTlsLocalHint),
                    if (_tls != DbTlsMode.verify && !_tlsAuto) Text(l.dbTlsWarning),
                    _input('serverName', l.dbTlsServerName),
                    _input('ca', l.dbTlsCa, lines: 3),
                    _input('certificate', l.dbClientCertificate, lines: 3),
                    _input('key', l.dbPrivateKey, secret: true),
                    if (_engine == DbEngine.mongodb)
                      DropdownButtonFormField<DbAuthMechanism>(
                        initialValue: _mechanism,
                        decoration: InputDecoration(labelText: l.dbAuthMechanism),
                        isExpanded: true,
                        items: [
                          for (final mechanism in DbAuthMechanism.values)
                            DropdownMenuItem(
                              value: mechanism,
                              child: Text(
                                mechanism == DbAuthMechanism.defaultAuth ? l.dbAuthDefault : mechanism.wireName,
                              ),
                            ),
                        ],
                        onChanged: _busy ? null : (v) => setState(() => _mechanism = v!),
                      ),
                  ],
                ),
                ExpansionTile(
                  title: Text(l.dbRoute),
                  children: [
                    if (choices == null)
                      const LinearProgressIndicator()
                    else
                      DropdownButtonFormField<int>(
                        key: const ValueKey('db-route'),
                        isExpanded: true,
                        initialValue: _route.kind == DbRouteKind.direct
                            ? -1
                            : choices.routes.indexWhere(
                                (r) => r.route.kind == _route.kind && r.route.reference == _route.reference,
                              ),
                        decoration: InputDecoration(labelText: l.dbRoute),
                        items: [
                          DropdownMenuItem(value: -1, child: Text(l.dbDirect)),
                          for (var i = 0; i < choices.routes.length; i++)
                            DropdownMenuItem(value: i, child: Text(choices.routes[i].label)),
                        ],
                        validator: (_) =>
                            _route.kind != DbRouteKind.direct &&
                                !choices.routes.any(
                                  (r) => r.route.kind == _route.kind && r.route.reference == _route.reference,
                                )
                            ? l.dbInvalid
                            : null,
                        onChanged: _busy
                            ? null
                            : (i) => setState(() => _route = i == -1 ? const DbRoute() : choices.routes[i!].route),
                      ),
                    Text(l.dbRouteHelp),
                  ],
                ),
                ExpansionTile(
                  title: Text(l.dbLimits),
                  children: [
                    _input('rows', l.dbMaxRows, min: 1, max: 1000000),
                    _input('statement', l.dbStatementTimeout, min: 1, max: _production ? 300 : 86400),
                    _input('connect', l.dbConnectTimeout, min: 1, max: 120),
                    _input('memory', l.dbMemoryLimit, min: 1, max: 256),
                  ],
                ),
                ExpansionTile(
                  title: Text(l.dbOptions),
                  children: [
                    if (_engine == DbEngine.postgresql) ...[
                      _input('application', l.dbApplicationName, required: true),
                      _input('searchPath', l.dbSearchPath),
                    ],
                    if (_engine == DbEngine.mysql || _engine == DbEngine.mariadb)
                      _input('charset', l.dbCharset, required: true),
                    if (_engine == DbEngine.clickhouse)
                      for (final key in const [
                        'max_threads',
                        'max_memory_usage',
                        'max_bytes_to_read',
                        'max_rows_to_read',
                      ])
                        _input(key, key),
                    if (_engine == DbEngine.redis)
                      CheckboxListTile(
                        value: _resp3,
                        title: const Text('RESP3'), // l10n-ignore: Redis protocol version.
                        onChanged: _busy ? null : (v) => setState(() => _resp3 = v!),
                      ),
                    if (_engine == DbEngine.mongodb) ...[
                      _input('authSource', l.dbMongoAuthSource),
                      _input('replicaSet', l.dbMongoReplicaSet),
                      CheckboxListTile(
                        value: _srv,
                        title: Text(l.dbMongoSrv),
                        onChanged: _busy ? null : (v) => setState(() => _srv = v!),
                      ),
                      DropdownButtonFormField<DbMongoReadPreference>(
                        initialValue: _readPreference,
                        isExpanded: true,
                        decoration: InputDecoration(labelText: l.dbMongoReadPreference),
                        items: [
                          for (final preference in DbMongoReadPreference.values)
                            DropdownMenuItem(value: preference, child: Text(preference.wireName)),
                        ],
                        onChanged: _busy ? null : (v) => setState(() => _readPreference = v!),
                      ),
                      Text(l.dbMongoReadHelp),
                    ],
                  ],
                ),
                ExpansionTile(
                  title: Text(l.dbGroup),
                  children: [
                    DropdownButtonFormField<String>(
                      initialValue: choices?.groups.any((g) => g.id == _group) == true ? _group : '',
                      isExpanded: true,
                      decoration: InputDecoration(labelText: l.dbGroup),
                      items: [
                        DropdownMenuItem(value: '', child: Text(l.dbUngrouped)),
                        for (final group in choices?.groups ?? const <DbGroupChoice>[])
                          DropdownMenuItem(value: group.id, child: Text(group.label)),
                      ],
                      onChanged: _busy ? null : (value) => setState(() => _group = value == '' ? null : value),
                    ),
                    _input('tags', l.dbTags),
                    Padding(
                      padding: const EdgeInsets.only(top: 4, bottom: 12),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(l.dbColor, style: Theme.of(context).textTheme.labelLarge),
                          const SizedBox(height: 8),
                          ColorLabelPicker(
                            value: _value('color').isEmpty ? null : _value('color'),
                            enabled: !_busy,
                            onChanged: (color) => setState(() => _field('color').text = color ?? ''),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context)),
        GlassButton(
          key: const ValueKey('db-test'),
          label: l.dbTestConnection,
          onPressed: _busy || _service == null ? null : () => _submit(test: true),
        ),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('db-save'),
        label: l.commonSave,
        onPressed: _busy || _service == null ? null : _submit,
      ),
    );
  }
}
