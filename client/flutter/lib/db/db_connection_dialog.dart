import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/color_label_picker.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/db/db_errors.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/db/db_transport_badge.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DbConnectionDialog extends ConsumerStatefulWidget {
  const DbConnectionDialog({this.connection, this.template, super.key});

  /// The saved connection being edited; null creates a new one.
  final DbConnection? connection;

  /// Prefilled values for a new connection (e.g. from a Kubernetes pod).
  final DbConnection? template;
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
  bool _tlsChosen = false, _tlsAuto = false, _allowUnbound = false;
  Object? _error;
  late DbAuthMechanism _mechanism;
  late DbMongoReadPreference _readPreference;
  late DbCqlConsistency _readConsistency, _writeConsistency;
  late bool _production, _resp3, _srv;
  bool _savePassword = true, _clearPassword = false, _busy = false, _failed = false, _disposing = false;
  String? _group;
  DbRoute _route = const DbRoute();
  DbKubeTargetKind _kubeKind = DbKubeTargetKind.pod;
  DbConnectionChoices? _choices;
  DbConnectionCheck? _check;
  DbConnectionDraft? _pendingDraft;

  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  TextEditingController _field(String key) => _fields[key]!;
  String _value(String key) => _field(key).text.trim();
  bool get _kube => _route.kind == DbRouteKind.kubernetes;
  bool get _docker => _route.kind == DbRouteKind.docker;
  bool get _tunnel => _route.kind == DbRouteKind.tunnel;

  /// PostgreSQL "require" binds the password to TLS unless the user opts out.
  bool get _unboundChoice => _engine == DbEngine.postgresql && _tls == DbTlsMode.require;

  /// Kubernetes and Docker forwards listen on loopback; the port is the
  /// container port.
  bool get _forwarded => _kube || _docker;

  /// The selected route; Kubernetes and Docker routes take their workload or
  /// container from the form.
  DbRoute get _routeValue => switch (_route.kind) {
    DbRouteKind.kubernetes => DbRoute(
      kind: DbRouteKind.kubernetes,
      reference: _route.reference,
      namespace: _value('kubeNamespace'),
      targetKind: _kubeKind,
      targetName: _value('kubeTarget'),
    ),
    DbRouteKind.docker => DbRoute(
      kind: DbRouteKind.docker,
      reference: _route.reference,
      container: _value('dockerContainer'),
    ),
    _ => _route,
  };

  /// A saved tunnel listening on the direct loopback address in the form.
  DbRouteChoice? get _matchingTunnel {
    if (_route.kind != DbRouteKind.direct || !dbLoopbackHost(_value('host'))) return null;
    final port = int.tryParse(_value('port'));
    return _choices?.routes.where((r) => r.route.kind == DbRouteKind.tunnel && r.localPort == port).firstOrNull;
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _scope = ref.read(dbScopeProvider);
    _service = ref.read(dbServiceProvider);
    final c = widget.connection ?? widget.template;
    _engine = c?.engine ?? DbEngine.postgresql;
    _tls = c?.tls.mode ?? DbTlsMode.verify;
    _allowUnbound = c?.tls.allowUnboundAuth ?? false;
    _mechanism = c?.auth.mechanism ?? DbAuthMechanism.defaultAuth;
    _production = c?.production ?? true;
    _resp3 = c?.options.resp3 ?? false;
    _srv = c?.options.srv ?? false;
    _readPreference = c?.options.readPreference ?? DbMongoReadPreference.secondaryPreferred;
    _readConsistency = c?.options.readConsistency ?? DbCqlConsistency.localOne;
    _writeConsistency = c?.options.writeConsistency ?? DbCqlConsistency.localQuorum;
    _route = c?.route ?? const DbRoute();
    _kubeKind = _route.targetKind;
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
      'contactPoints': c?.options.contactPoints.join(', ') ?? '',
      'localDc': c?.options.localDatacenter ?? '',
      'pageSize': '${c?.options.pageSize ?? 500}',
      'kubeNamespace': _route.namespace,
      'kubeTarget': _route.targetName,
      'dockerContainer': _route.container,
      for (final key in const ['max_threads', 'max_memory_usage', 'max_bytes_to_read', 'max_rows_to_read'])
        key: c?.options.settings[key]?.toString() ?? '',
    };
    for (final entry in values.entries) {
      _fields[entry.key] = TextEditingController(text: entry.value);
    }
    // A port-forward or local database usually has no TLS: unless the user
    // picked a mode, loopback hosts start with "prefer".
    _field('host').addListener(() {
      // Kubernetes and Docker forwards always listen on loopback.
      if (_tlsChosen || !mounted || _disposing || _forwarded) return;
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
    _disposing = true;
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
        // A Kubernetes forward always listens on loopback.
        host: _forwarded ? '127.0.0.1' : _value('host'),
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
        tls: DbTls(
          mode: _tls,
          certificateAuthorityPem: nullable('ca'),
          serverName: nullable('serverName'),
          allowUnboundAuth: _unboundChoice && _allowUnbound,
        ),
        route: _routeValue,
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
          // Forwards reach one endpoint; extra contact points need a socket route.
          contactPoints: _forwarded
              ? const []
              : _value('contactPoints').split(',').map((s) => s.trim()).where((s) => s.isNotEmpty).toList(),
          localDatacenter: nullable('localDc'),
          readConsistency: _readConsistency,
          writeConsistency: _writeConsistency,
          pageSize: _engine == DbEngine.cassandra ? number('pageSize') : 500,
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
      bool badHost(String host) =>
          host.contains(RegExp(r'[\s/@?#\[\]\\]')) || (host.contains(':') && InternetAddress.tryParse(host) == null);
      if (value.isNotEmpty && key == 'host' && badHost(value)) return l.dbInvalid;
      if (key == 'contactPoints') {
        final hosts = value.split(',').map((s) => s.trim()).where((s) => s.isNotEmpty).toList();
        if (hosts.length > 15 || hosts.any(badHost)) return l.dbInvalid;
      }
      if (_engine == DbEngine.cassandra &&
          key == 'database' &&
          value.isNotEmpty &&
          (value.length > 48 || !RegExp(r'^[A-Za-z0-9_]+$').hasMatch(value))) {
        return l.dbInvalid;
      }
      if (key == 'kubeNamespace' && !_kubeName(value, 63, dots: false)) return l.dbInvalid;
      if (key == 'kubeTarget' && !_kubeName(value, 253, dots: true)) return l.dbInvalid;
      if (key == 'dockerContainer' &&
          (value.length > 255 || !RegExp(r'^[a-zA-Z0-9][a-zA-Z0-9_.-]*$').hasMatch(value))) {
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
          : RevealedSecretText(
              controller: _field(key),
              // A pasted private key is shown in plain text.
              enabled: secret && key == 'key',
              child: TextFormField(
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
            ),
    );
  }

  /// RFC 1123 label (namespace) or subdomain (object name).
  static bool _kubeName(String value, int max, {required bool dots}) {
    const label = '[a-z0-9]([-a-z0-9]*[a-z0-9])?';
    final pattern = dots ? RegExp('^$label(\\.$label)*\$') : RegExp('^$label\$');
    return value.length <= max && pattern.hasMatch(value);
  }

  void _selectRoute(DbRouteChoice? choice) => setState(() {
    _route = choice?.route ?? const DbRoute();
    // A tunnel's target is the database address (and its TLS identity).
    if (choice != null && _tunnel) {
      _field('host').text = choice.host ?? _value('host');
      _field('port').text = '${choice.port ?? _value('port')}';
    }
    // Port-forwards listen on loopback, which usually has no TLS.
    if (_forwarded && !_tlsChosen) {
      _tls = DbTlsMode.prefer;
      _tlsAuto = true;
    }
  });

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
                if (_check case final check?) ...[
                  Text(l.dbTestSuccess),
                  Text(check.serverVersion),
                  if (check.transportSecurity case final security?)
                    Align(
                      alignment: AlignmentDirectional.centerStart,
                      child: DbTransportBadge(security, key: const ValueKey('db-test-transport')),
                    ),
                ],
                _input('name', l.dbName, required: true),
                GlassDropdownField<DbEngine>(
                  key: const ValueKey('db-engine'),
                  value: _engine,
                  label: l.dbEngine,
                  items: [for (final engine in DbEngine.values) GlassSelectItem(value: engine, label: engine.label)],
                  onChanged: _busy || widget.connection != null
                      ? null
                      : (value) {
                          setState(() {
                            _engine = value;
                            _field('port').text = '${value.port}';
                            _field('database').text = value == DbEngine.redis ? '0' : '';
                            _mechanism = DbAuthMechanism.defaultAuth;
                          });
                        },
                ),
                if (_engine.localOnly)
                  Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: InfoBanner(
                      key: const ValueKey('db-local-only'),
                      title: l.dbLocalOnly,
                      message: l.dbLocalOnlyHelp,
                    ),
                  ),
                const SizedBox(height: 12),
                if (!_forwarded && !_tunnel) _input('host', l.dbHost, required: true),
                if (!_tunnel) _input('port', _forwarded ? l.dbContainerPort : l.dbPort, min: 1, max: 65535),
                ListenableBuilder(
                  listenable: Listenable.merge([_field('host'), _field('port')]),
                  builder: (context, _) => switch (_matchingTunnel) {
                    final tunnel? => Align(
                      alignment: AlignmentDirectional.centerStart,
                      child: TextButton.icon(
                        key: const ValueKey('db-use-tunnel'),
                        icon: const Icon(Icons.swap_horiz_rounded),
                        label: Text(l.dbUseTunnel(tunnel.label)),
                        onPressed: _busy ? null : () => _selectRoute(tunnel),
                      ),
                    ),
                    null => const SizedBox.shrink(),
                  },
                ),
                _input(
                  'database',
                  _engine == DbEngine.cassandra ? l.dbKeyspace : l.dbDatabase,
                  required: _engine == DbEngine.redis,
                ),
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
                    GlassDropdownField<DbTlsMode>(
                      key: const ValueKey('db-tls'),
                      value: _tls,
                      items: [
                        for (final mode in DbTlsMode.values)
                          GlassSelectItem(
                            value: mode,
                            label: switch (mode) {
                              DbTlsMode.verify => l.dbTlsVerify,
                              DbTlsMode.require => l.dbTlsRequire,
                              DbTlsMode.prefer => l.dbTlsPrefer,
                              DbTlsMode.disable => l.dbTlsDisable,
                            },
                          ),
                      ],
                      onChanged: _busy
                          ? null
                          : (v) => setState(() {
                              _tls = v;
                              _tlsChosen = true;
                              _tlsAuto = false;
                            }),
                    ),
                    if (_tls == DbTlsMode.prefer) Text(l.dbTlsPreferHelp, key: const ValueKey('db-tls-help')),
                    if (_tls == DbTlsMode.require) Text(l.dbTlsRequireHelp, key: const ValueKey('db-tls-help')),
                    if (_tlsAuto) Text(l.dbTlsLocalHint),
                    if (_tls != DbTlsMode.verify && !_tlsAuto) Text(l.dbTlsWarning),
                    if (_unboundChoice)
                      CheckboxListTile(
                        key: const ValueKey('db-tls-unbound'),
                        value: _allowUnbound,
                        title: Text(l.dbTlsAllowUnbound),
                        subtitle: Text(l.dbTlsAllowUnboundHelp),
                        onChanged: _busy ? null : (v) => setState(() => _allowUnbound = v!),
                      ),
                    _input('serverName', l.dbTlsServerName),
                    _input('ca', l.dbTlsCa, lines: 3),
                    _input('certificate', l.dbClientCertificate, lines: 3),
                    _input('key', l.dbPrivateKey, secret: true),
                    if (_engine == DbEngine.mongodb)
                      GlassDropdownField<DbAuthMechanism>(
                        value: _mechanism,
                        label: l.dbAuthMechanism,
                        items: [
                          for (final mechanism in DbAuthMechanism.values)
                            GlassSelectItem(
                              value: mechanism,
                              label: mechanism == DbAuthMechanism.defaultAuth ? l.dbAuthDefault : mechanism.wireName,
                            ),
                        ],
                        onChanged: _busy ? null : (v) => setState(() => _mechanism = v),
                      ),
                  ],
                ),
                ExpansionTile(
                  title: Text(l.dbRoute),
                  initiallyExpanded: _route.restorable,
                  children: [
                    if (choices == null)
                      const LinearProgressIndicator()
                    else
                      GlassDropdownField<int>(
                        key: const ValueKey('db-route'),
                        // -1: direct, -2: a Docker container (engine chosen below).
                        value: switch (_route.kind) {
                          DbRouteKind.direct => -1,
                          DbRouteKind.docker => -2,
                          _ => choices.routes.indexWhere(
                            (r) => r.route.kind == _route.kind && r.route.reference == _route.reference,
                          ),
                        },
                        label: l.dbRoute,
                        items: [
                          GlassSelectItem(value: -1, label: l.dbDirect),
                          for (var i = 0; i < choices.routes.length; i++)
                            GlassSelectItem(
                              value: i,
                              label: switch (choices.routes[i].route.kind) {
                                DbRouteKind.kubernetes => l.dbRouteKubernetes(choices.routes[i].label),
                                DbRouteKind.tunnel => l.dbRouteTunnel(choices.routes[i].label),
                                _ => choices.routes[i].label,
                              },
                            ),
                          GlassSelectItem(value: -2, label: l.dbRouteDocker),
                        ],
                        validator: (_) =>
                            !const {DbRouteKind.direct, DbRouteKind.docker}.contains(_route.kind) &&
                                !choices.routes.any(
                                  (r) => r.route.kind == _route.kind && r.route.reference == _route.reference,
                                )
                            ? l.dbInvalid
                            : null,
                        onChanged: _busy
                            ? null
                            : (i) => _selectRoute(switch (i) {
                                -1 => null,
                                -2 => DbRouteChoice(
                                  route: DbRoute(
                                    kind: DbRouteKind.docker,
                                    reference: _docker ? _route.reference : null,
                                  ),
                                  label: '',
                                ),
                                _ => choices.routes[i],
                              }),
                      ),
                    if (_kube) ...[
                      const SizedBox(height: 12),
                      _input('kubeNamespace', l.kubeNamespace, required: true),
                      GlassDropdownField<DbKubeTargetKind>(
                        key: const ValueKey('db-kube-kind'),
                        value: _kubeKind,
                        label: l.dbKubeTarget,
                        items: [
                          for (final kind in DbKubeTargetKind.values) GlassSelectItem(value: kind, label: kind.label),
                        ],
                        onChanged: _busy ? null : (v) => setState(() => _kubeKind = v),
                      ),
                      const SizedBox(height: 12),
                      _input('kubeTarget', l.dbKubeTargetName, required: true),
                      Text(l.dbKubeRouteHelp),
                    ] else if (_docker && choices != null) ...[
                      const SizedBox(height: 12),
                      GlassDropdownField<String>(
                        key: const ValueKey('db-docker-engine'),
                        value: _route.reference ?? '',
                        label: l.dbDockerEngine,
                        items: [
                          GlassSelectItem(value: '', label: l.dockerLocalHost),
                          for (final r in choices.routes)
                            if (r.route.kind == DbRouteKind.ssh)
                              GlassSelectItem(value: r.route.reference!, label: r.label),
                        ],
                        validator: (_) =>
                            _route.reference != null &&
                                !choices.routes.any(
                                  (r) => r.route.kind == DbRouteKind.ssh && r.route.reference == _route.reference,
                                )
                            ? l.dbInvalid
                            : null,
                        onChanged: _busy
                            ? null
                            : (v) => setState(
                                () => _route = DbRoute(kind: DbRouteKind.docker, reference: v.isEmpty ? null : v),
                              ),
                      ),
                      const SizedBox(height: 12),
                      _input('dockerContainer', l.dbDockerContainer, required: true),
                      Text(l.dbDockerRouteHelp),
                    ] else if (_tunnel)
                      Text(l.dbTunnelRouteHelp)
                    else
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
                    if (_engine == DbEngine.cassandra) ...[
                      if (!_forwarded) _input('contactPoints', l.dbCassandraContactPoints),
                      _input('localDc', l.dbCassandraLocalDc),
                      GlassDropdownField<DbCqlConsistency>(
                        key: const ValueKey('db-cql-read'),
                        value: _readConsistency,
                        label: l.dbCassandraReadConsistency,
                        items: [
                          for (final level in DbCqlConsistency.values)
                            if (level.readable) GlassSelectItem(value: level, label: level.label),
                        ],
                        onChanged: _busy ? null : (v) => setState(() => _readConsistency = v),
                      ),
                      const SizedBox(height: 12),
                      GlassDropdownField<DbCqlConsistency>(
                        key: const ValueKey('db-cql-write'),
                        value: _writeConsistency,
                        label: l.dbCassandraWriteConsistency,
                        items: [
                          for (final level in DbCqlConsistency.values)
                            GlassSelectItem(value: level, label: level.label),
                        ],
                        onChanged: _busy ? null : (v) => setState(() => _writeConsistency = v),
                      ),
                      const SizedBox(height: 12),
                      _input('pageSize', l.dbCassandraPageSize, min: 1, max: 10000),
                      Text(l.dbCassandraRouteHelp),
                    ],
                    if (_engine == DbEngine.mongodb) ...[
                      _input('authSource', l.dbMongoAuthSource),
                      _input('replicaSet', l.dbMongoReplicaSet),
                      CheckboxListTile(
                        value: _srv,
                        title: Text(l.dbMongoSrv),
                        onChanged: _busy ? null : (v) => setState(() => _srv = v!),
                      ),
                      GlassDropdownField<DbMongoReadPreference>(
                        value: _readPreference,
                        label: l.dbMongoReadPreference,
                        items: [
                          for (final preference in DbMongoReadPreference.values)
                            GlassSelectItem(value: preference, label: preference.wireName),
                        ],
                        onChanged: _busy ? null : (v) => setState(() => _readPreference = v),
                      ),
                      Text(l.dbMongoReadHelp),
                    ],
                  ],
                ),
                ExpansionTile(
                  title: Text(l.dbGroup),
                  children: [
                    GlassDropdownField<String>(
                      value: choices?.groups.any((g) => g.id == _group) == true ? _group : '',
                      label: l.dbGroup,
                      items: [
                        GlassSelectItem(value: '', label: l.dbUngrouped),
                        for (final group in choices?.groups ?? const <DbGroupChoice>[])
                          GlassSelectItem(value: group.id, label: group.label),
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
