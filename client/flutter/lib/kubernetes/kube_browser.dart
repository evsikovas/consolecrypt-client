import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_auth_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_bulk_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_create_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_flux_cli_panel.dart';
import 'package:consolecrypt/kubernetes/kube_flux_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_panel.dart';
import 'package:consolecrypt/kubernetes/kube_forward_restore.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_panel.dart';
import 'package:consolecrypt/kubernetes/kube_history_panel.dart';
import 'package:consolecrypt/kubernetes/kube_history_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_panel.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_navigation.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_organization_service.dart';
import 'package:consolecrypt/kubernetes/kube_permissions_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_resource_details.dart';
import 'package:consolecrypt/kubernetes/kube_resource_table_view.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_view_preferences_service.dart';
import 'package:consolecrypt/kubernetes/kube_workspace_content.dart';
import 'package:consolecrypt/kubernetes/kube_workspace_panel.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// A browser owns exactly one native connection. Leaving it, switching profile
/// or locking the vault revokes the handle; late asynchronous results are dropped.
class KubeBrowser extends ConsumerStatefulWidget {
  const KubeBrowser({
    required this.connection,
    required this.service,
    required this.onClose,
    this.connections = const [],
    this.onSelectContext,
    super.key,
  });
  final KubeConnection connection;
  final KubeBrowserService service;
  final VoidCallback onClose;
  final List<KubeConnection> connections;
  final ValueChanged<KubeConnection>? onSelectContext;
  @override
  ConsumerState<KubeBrowser> createState() => _KubeBrowserState();
}

class _KubeBrowserState extends ConsumerState<KubeBrowser> with WidgetsBindingObserver {
  static const _maxRows = 10000;
  final _selector = TextEditingController();
  final _selectedRows = <String>{};
  late final Set<String> _favoriteNamespaces, _favoriteKinds;
  bool _savingFavorites = false;
  String _resourceGroup = 'all';
  late final KubeScope _scope;
  KubeSession? _session;
  List<KubeResourceKind> _kinds = const [];
  List<KubeTableColumn> _columns = const [];
  List<String> _namespaces = const [];
  final _rows = <String, KubeResourceRow>{};
  final _objectTabs = <KubeResourceRef>[];
  KubeResourceKind? _kind;
  String? _namespace, _continueToken;
  String _query = '', _labelSelector = '', _resourceVersion = '', _view = 'resources';
  bool _connecting = true, _loading = false, _loadingMore = false;
  bool _navigationPending = false, _snapshotPending = false;
  late final KubeActiveNavigation _navigationSnapshot;
  late final KubeForwardRestore _restoreController;
  bool _forwardRestoreFailed = false;
  bool _connectionFailed = false, _requestFailed = false, _namespacesFailed = false;
  bool _authenticationRequired = false, _panelOpen = false, _aiPanel = false;
  KubeResourceRef? _aiSelected;
  String? _aiQuestion;
  final _extraTerminals = <TerminalSessionId>{};
  final _logPins = <KubeLogPin>[];
  final _outputs = <KubePanelOutput>[];
  String? _selectedLog, _selectedOutput;
  double _panelHeight = 300;
  bool _panelRight = false;
  final _workspaceKey = GlobalKey();
  String _workspaceTab = 'terminal';
  bool _iosSuspended = false;
  bool _live = false, _watchFailed = false, _foreground = true, _limited = false;
  int _generation = 0, _connectionGeneration = 0;
  // Cancelled by _stopWatch on filter, profile, lifecycle and disposal changes.
  // ignore: cancel_subscriptions
  StreamSubscription<KubeWatchEvent>? _watch;
  Timer? _enrichmentTimer, _metricsTimer;
  final _metrics = <String, KubeMetrics>{};
  bool _metricsLoading = false, _metricsUnavailable = false;
  bool get _hasMetrics =>
      widget.service is KubeMetricsService && (_kind?.isPod == true || (_kind?.group == '' && _kind?.kind == 'Node'));
  bool _enriching = false;
  int _enrichmentGeneration = 0;
  final _searchFocus = FocusNode();
  Set<String> _visibleRows = {};

  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _navigationSnapshot = ref.read(kubeActiveNavigationProvider.notifier);
    _restoreController = ref.read(kubeForwardRestoreProvider.notifier);
    _favoriteNamespaces = widget.connection.favouriteNamespaces.toSet();
    _favoriteKinds = widget.connection.favouriteKinds.toSet();
    _namespace = widget.connection.namespace.isEmpty ? null : widget.connection.namespace;
    WidgetsBinding.instance.addObserver(this);
    unawaited(_loadWorkspacePreferences());
    unawaited(_connect());
    _metricsTimer = Timer.periodic(const Duration(seconds: 10), (_) => unawaited(_loadMetrics()));
  }

  void _disconnect(KubeSession? session) {
    if (session != null) scheduleMicrotask(() => _navigationSnapshot.clear(session.id));
    if (session != null) unawaited(widget.service.disconnect(session.id).catchError((Object _) {}));
  }

  void _stopWatch() {
    _enrichmentTimer?.cancel();
    _enrichmentGeneration++;
    final watch = _watch;
    _watch = null;
    if (watch != null) unawaited(watch.cancel());
    _live = false;
  }

  void _revoke() {
    _connectionGeneration++;
    _generation++;
    _stopWatch();
    _disconnect(_session);
    _session = null;
    _rows.clear();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _metricsTimer?.cancel();
    _revoke();
    _selector.dispose();
    _searchFocus.dispose();
    _logPins.clear();
    _outputs.clear();
    _objectTabs.clear();
    scheduleMicrotask(() => _restoreController.clearContext(widget.connection.id));
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      if (!_foreground && _current) {
        _foreground = true;
        if (_iosSuspended) {
          _iosSuspended = false;
          unawaited(_connect());
        } else {
          unawaited(_refresh());
        }
      }
    } else if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.hidden ||
        state == AppLifecycleState.detached) {
      _foreground = false;
      if (AppPlatform.isIOS) {
        _iosSuspended = true;
        _revoke();
      } else {
        _generation++;
        _stopWatch();
      }
      if (mounted) setState(() {});
    }
  }

  Future<void> _connect() async {
    if (!_current) return;
    _revoke();
    final requestGeneration = _connectionGeneration;
    setState(() {
      _connecting = true;
      _connectionFailed = false;
      _authenticationRequired = false;
      _requestFailed = false;
      _kinds = const [];
      _namespaces = const [];
    });
    KubeSession? connected;
    try {
      connected = await widget.service.connect(widget.connection.id);
      if (!_current || requestGeneration != _connectionGeneration) {
        _disconnect(connected);
        return;
      }
      if (connected.contextId != widget.connection.id) {
        _disconnect(connected);
        connected = null;
        throw StateError('Mismatched Kubernetes session'); // l10n-ignore: diagnostic
      }
      _session = connected;
      final namespaces = _loadNamespaces(connected.id, requestGeneration);
      final kinds = await widget.service.discover(connected.id);
      await namespaces;
      if (!_current || requestGeneration != _connectionGeneration) return;
      final sorted = kinds.where((kind) => kind.canList && !kind.resource.contains('/')).toList()
        ..sort((a, b) => a.kind.compareTo(b.kind));
      setState(() {
        _kinds = sorted;
        _kind = sorted.where((kind) => kind.isPod).firstOrNull ?? sorted.firstOrNull;
        _connecting = false;
      });
      await _refresh();
      if (_current && requestGeneration == _connectionGeneration) await _restoreForwards(connected);
      _scheduleNavigation();
    } catch (error) {
      if (!_current || requestGeneration != _connectionGeneration) return;
      _disconnect(connected);
      _session = null;
      setState(() {
        _connecting = false;
        _connectionFailed = true;
        _authenticationRequired =
            error is AppException &&
            const {'exec_approval_required', 'oidc_login_required'}.contains(error.args['rule']);
      });
    }
  }

  Future<void> _restoreForwards(KubeSession session) async {
    final service = widget.service;
    if (!_current || service is! KubeForwardService || !session.canPortForward) return;
    final plans = ref
        .read(kubeForwardRestoreProvider)
        .where((plan) => plan.contextId == session.contextId && plan.sessionId != session.id)
        .toList();
    _forwardRestoreFailed = false;
    for (final plan in plans) {
      if (!_current || _session?.id != session.id || !_foreground) return;
      try {
        if (plan.approveNetwork &&
            (AppPlatform.isMobile || session.production || service is! KubeNetworkForwardService)) {
          _restoreController.forget(plan.forwardId);
          continue;
        }
        final forward = plan.approveNetwork
            ? await (service as KubeNetworkForwardService).startNetworkForward(
                session.id,
                namespace: plan.namespace,
                pod: plan.pod,
                remotePort: plan.remotePort,
                localPort: plan.localPort,
                localAddress: plan.localAddress,
                approveNetwork: true,
              )
            : await (service as KubeForwardService).startForward(
                session.id,
                namespace: plan.namespace,
                pod: plan.pod,
                remotePort: plan.remotePort,
                localPort: plan.localPort,
              );

        if (!_current || _session?.id != session.id || !_foreground) {
          await (service as KubeForwardService).stopForward(session.id, forward.id);
          return;
        }
        _restoreController.remember(
          KubeForwardRestorePlan(
            contextId: session.contextId,
            namespace: plan.namespace,
            pod: plan.pod,
            remotePort: plan.remotePort,
            localPort: plan.localPort,
            sessionId: session.id,
            forwardId: forward.id,
            localAddress: plan.localAddress,
            approveNetwork: plan.approveNetwork,
          ),
        );
      } catch (_) {
        if (_current) setState(() => _forwardRestoreFailed = true);
      }
    }
    if (_current) ref.invalidate(kubeForwardsProvider(_scope));
  }

  Future<void> _authenticate() async {
    if (!_current || !_authenticationRequired) return;
    final approved = await authenticateKubeConnection(context, widget.connection);
    if (_current && approved) await _connect();
  }

  Future<void> _loadNamespaces(String sessionId, int generation) async {
    try {
      final result = await widget.service.namespaces(sessionId);
      if (!_current || generation != _connectionGeneration) return;
      final names = result.toSet();
      if (_namespace != null) names.add(_namespace!);
      _namespaces = names.toList()..sort();
      _namespacesFailed = false;
    } catch (_) {
      if (!_current || generation != _connectionGeneration) return;
      _namespaces = _namespace == null ? [] : [_namespace!];
      _namespacesFailed = true;
    }
  }

  Future<void> _refresh() async {
    final session = _session;
    final kind = _kind;
    if (!_current || session == null || kind == null || !_foreground) return;
    final generation = ++_generation;
    _stopWatch();
    setState(() {
      _rows.clear();
      _selectedRows.clear();
      _metrics.clear();
      _metricsUnavailable = false;
      _columns = const [];
      _continueToken = null;
      _resourceVersion = '';
      _loading = true;
      _loadingMore = false;
      _requestFailed = false;
      _watchFailed = false;
      _limited = false;
    });
    try {
      final tableService = widget.service;
      final table = tableService is KubeTableService
          ? await (tableService as KubeTableService).table(
              session.id,
              kind,
              namespace: kind.namespaced ? _namespace : null,
              labelSelector: _labelSelector.isEmpty ? null : _labelSelector,
            )
          : null;
      final page =
          table?.page ??
          await widget.service.listResources(
            session.id,
            kind,
            namespace: kind.namespaced ? _namespace : null,
            labelSelector: _labelSelector.isEmpty ? null : _labelSelector,
          );
      if (!_current || generation != _generation) return;
      setState(() {
        _columns = table?.columns ?? const [];
        _acceptPage(page);
      });
      _startWatch(generation);
      unawaited(_loadMetrics());
    } catch (_) {
      if (_current && generation == _generation) setState(() => _requestFailed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _loading = false);
    }
  }

  void _acceptPage(KubeResourcePage page) {
    for (final row in page.items) {
      if (!_rows.containsKey(row.id) && _rows.length >= _maxRows) {
        _limited = true;
        break;
      }
      _rows[row.id] = row;
    }
    _resourceVersion = page.resourceVersion;
    _continueToken = page.continueToken?.isEmpty == true ? null : page.continueToken;
    if (_rows.length >= _maxRows && _continueToken != null) _limited = true;
  }

  Future<void> _loadMetrics() async {
    final session = _session, kind = _kind;
    if (!_current ||
        !_foreground ||
        _view != 'resources' ||
        !_hasMetrics ||
        _metricsLoading ||
        session == null ||
        kind == null) {
      return;
    }
    final generation = _generation;
    _metricsLoading = true;
    try {
      final values = await (widget.service as KubeMetricsService).metrics(
        session.id,
        nodes: kind.kind == 'Node',
        namespace: kind.namespaced ? _namespace : null,
      );
      if (!_current || !_foreground || generation != _generation || _session?.id != session.id) return;
      setState(() {
        _metrics.clear();
        for (final value in values.take(_maxRows)) {
          _metrics['${value.namespace ?? ''}/${value.name}'] = value;
        }
        _metricsUnavailable = values.isEmpty;
      });
    } catch (_) {
      if (_current && generation == _generation) setState(() => _metricsUnavailable = true);
    } finally {
      _metricsLoading = false;
    }
  }

  KubeResourceRow _withMetrics(KubeResourceRow row) {
    if (!_hasMetrics) return row;
    final metric = _metrics['${row.namespace ?? ''}/${row.name}'];
    return KubeResourceRow(
      name: row.name,
      uid: row.uid,
      resourceVersion: row.resourceVersion,
      namespace: row.namespace,
      createdAt: row.createdAt,
      status: row.status,
      cells: [
        ...List.generate(_columns.length, (i) => i < row.cells.length ? row.cells[i] : '—'),
        metric == null ? '—' : (metric.cpuCores * 1000).toStringAsFixed(1),
        metric == null ? '—' : (metric.memoryBytes / 1048576).toStringAsFixed(1),
      ],
    );
  }

  void _scheduleEnrichment() {
    if (_enrichmentTimer?.isActive == true || widget.service is! KubeTableService) return;
    _enrichmentTimer = Timer(const Duration(milliseconds: 800), () => unawaited(_enrichTable()));
  }

  Future<void> _enrichTable() async {
    final session = _session, kind = _kind;
    if (!_current ||
        !_foreground ||
        _view != 'resources' ||
        _enriching ||
        session == null ||
        kind == null ||
        widget.service is! KubeTableService) {
      return;
    }
    final generation = _generation, enrichment = _enrichmentGeneration;
    _enriching = true;
    try {
      String? token;
      final needed = Set<String>.of(_visibleRows);
      for (var page = 0; page < 5; page++) {
        final table = await (widget.service as KubeTableService).table(
          session.id,
          kind,
          namespace: kind.namespaced ? _namespace : null,
          labelSelector: _labelSelector.isEmpty ? null : _labelSelector,
          continueToken: token,
        );
        if (!_current ||
            !_foreground ||
            _view != 'resources' ||
            generation != _generation ||
            enrichment != _enrichmentGeneration ||
            _session?.id != session.id) {
          return;
        }
        setState(() {
          _columns = table.columns;
          for (final fresh in table.page.items) {
            final row = _rows[fresh.id];
            if (row != null && row.uid == fresh.uid && row.resourceVersion == fresh.resourceVersion) {
              // Only presentation cells from the matching immutable object
              // revision are accepted. A Table response cannot resurrect a
              // deleted object or replace newer watch state.
              needed.remove(row.id);
              _rows[row.id] = KubeResourceRow(
                name: row.name,
                uid: row.uid,
                resourceVersion: row.resourceVersion,
                namespace: row.namespace,
                createdAt: row.createdAt,
                status: row.status,
                cells: fresh.cells,
              );
            }
          }
        });
        token = table.page.continueToken;
        if (token == null || token.isEmpty || needed.isEmpty) break;
      }
    } catch (_) {
      // A permission change or unavailable Table endpoint must not terminate
      // an otherwise healthy resource watch.
    } finally {
      _enriching = false;
    }
  }

  Future<void> _more() async {
    final session = _session;
    final kind = _kind;
    final token = _continueToken;
    if (!_current || session == null || kind == null || token == null || _loadingMore || _limited) return;
    final generation = _generation;
    setState(() {
      _loadingMore = true;
      _requestFailed = false;
    });
    try {
      final tableService = widget.service;
      final table = tableService is KubeTableService
          ? await (tableService as KubeTableService).table(
              session.id,
              kind,
              namespace: kind.namespaced ? _namespace : null,
              labelSelector: _labelSelector.isEmpty ? null : _labelSelector,
              continueToken: token,
            )
          : null;
      final page =
          table?.page ??
          await widget.service.listResources(
            session.id,
            kind,
            namespace: kind.namespaced ? _namespace : null,
            labelSelector: _labelSelector.isEmpty ? null : _labelSelector,
            continueToken: token,
          );
      if (!_current || generation != _generation) return;
      setState(() => _acceptPage(page));
      _startWatch(generation);
    } catch (_) {
      if (_current && generation == _generation) setState(() => _requestFailed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _loadingMore = false);
    }
  }

  void _startWatch(int generation) {
    final service = widget.service;
    final session = _session;
    final kind = _kind;
    if (service is! KubeWatchService ||
        session == null ||
        kind == null ||
        !kind.canWatch ||
        _continueToken != null ||
        _limited ||
        !_foreground ||
        _resourceVersion.isEmpty) {
      return;
    }
    _stopWatch();
    _live = true;
    _watch = (service as KubeWatchService)
        .watchResources(
          session.id,
          kind,
          namespace: kind.namespaced ? _namespace : null,
          labelSelector: _labelSelector.isEmpty ? null : _labelSelector,
          resourceVersion: _resourceVersion,
        )
        .listen(
          (event) {
            if (!_current || generation != _generation || !_foreground) return;
            if (event.type == KubeWatchEventType.relist) {
              unawaited(_refresh());
              return;
            }
            final row = event.resource;
            setState(() {
              if (event.resourceVersion.isNotEmpty) _resourceVersion = event.resourceVersion;
              if (event.type == KubeWatchEventType.snapshotBegin) {
                _rows.clear();
                _live = false;
                _loading = true;
              }
              if (event.type == KubeWatchEventType.snapshotPage && event.page != null) {
                _acceptPage(event.page!);
                _loading = _continueToken != null;
                _live = !_loading;
                if (_limited) _stopWatch();
              }
              if (event.type == KubeWatchEventType.reconnecting) {
                _live = false;
                _watchFailed = true;
              }
              if (event.type == KubeWatchEventType.bookmark || row != null) {
                _live = true;
                _watchFailed = false;
              }
              if (row == null) return;
              switch (event.type) {
                case KubeWatchEventType.added:
                case KubeWatchEventType.modified:
                  if (_rows.containsKey(row.id) || _rows.length < _maxRows) {
                    _rows[row.id] = row;
                  } else {
                    _limited = true;
                    _stopWatch();
                  }
                case KubeWatchEventType.deleted:
                  _rows.remove(row.id);
                case KubeWatchEventType.bookmark:
                case KubeWatchEventType.snapshotBegin:
                case KubeWatchEventType.snapshotPage:
                case KubeWatchEventType.reconnecting:
                case KubeWatchEventType.relist:
                  break;
              }
            });
            if (row != null || event.type == KubeWatchEventType.snapshotPage) _scheduleEnrichment();
          },
          onError: (Object _) {
            if (_current && generation == _generation) {
              setState(() {
                _live = false;
                _watchFailed = true;
              });
            }
          },
          onDone: () {
            if (_current && generation == _generation) {
              setState(() {
                _live = false;
                _watchFailed = true;
              });
            }
          },
        );
  }

  Future<void> _details(KubeResourceRow row) async {
    final kind = _kind;
    if (kind == null) return;
    await _openObject(KubeResourceRef(api: kind, name: row.name, namespace: row.namespace));
  }

  Future<void> _openObject(KubeResourceRef resource) async {
    final session = _session;
    if (!_current || session == null || !resource.api.canRead) return;
    if (!_objectTabs.any((tab) => tab.id == resource.id)) {
      _objectTabs.add(resource);
      if (_objectTabs.length > 12) _objectTabs.removeAt(0);
    }
    final changed = await showAppDialog<bool>(
      context,
      secure: true,
      builder: (_) => KubeResourceDetails(
        session: session,
        resource: resource,
        service: widget.service,
        contextName: widget.connection.name,
        openedResources: List.of(_objectTabs),
        onSelectResource: (target) {
          if (_current) unawaited(_openObject(target));
        },
        onCloseResource: (target) => _objectTabs.removeWhere((tab) => tab.id == target.id),
        onTerminalOpened: _showTerminals,
        onExplain: widget.service is KubeAiService ? _showAi : null,
        onPinLogs: _pinLogs,
        onOutput: _recordOutput,
      ),
    );
    if (changed == true && _current) await _refresh();
  }

  Future<void> _createResource() async {
    final session = _session, kind = _kind, service = widget.service;
    if (!_current || session == null || kind == null || service is! KubeManifestService) return;
    final created = await showAppDialog<bool>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (_) => KubeCreateDialog(
        session: session,
        contextName: widget.connection.name,
        kind: kind,
        namespace: _namespace,
        service: service as KubeManifestService,
        onOutput: _recordOutput,
      ),
    );
    if (_current && created == true) await _refresh();
  }

  Future<TerminalTab?> _runtimeTerminal(String nativeId) async {
    final session = _session;
    if (!_current || session == null) return null;
    final controller = ref.read(terminalTabsProvider.notifier);
    try {
      final tab = await controller.openRuntime(nativeId);
      if (!_current || _session != session) {
        await controller.close(tab);
        return null;
      }
      _extraTerminals.add(tab.sessionId);
      _showTerminals();
      return tab;
    } catch (_) {
      if (mounted && _current) showSnack(context, context.l10n.kubeTerminalFailed, error: true);
      return null;
    }
  }

  Future<void> _reviewedCommand(String id, String command) async {
    final session = _session;
    final tab = await _runtimeTerminal(id);
    if (!_current || tab == null || _session != session || ref.read(terminalTabsProvider).active != tab) return;
    ref.read(terminalTabsProvider.notifier).insertIntoActive(command);
  }

  void _output(String title, FluxCliOutput output) {
    if (!_current) return;
    _recordOutput(title, '${output.stdout}\n${output.stderr}');
    _showPanel();
  }

  void _recordOutput(String title, String raw) {
    if (!_current) return;
    if (_outputs.length >= 8) _outputs.removeAt(0);
    final id = '${DateTime.now().microsecondsSinceEpoch}';
    _outputs.add(
      KubePanelOutput(
        id: id,
        title: title,
        text: raw.length > 200000 ? '${raw.substring(0, 200000)}\n${context.l10n.kubeDiffTruncated}' : raw,
      ),
    );
    _workspaceTab = 'output';
    _selectedOutput = id;
    _selectedLog = null;
    _aiPanel = false;
    setState(() {});
  }

  Future<void> _loadWorkspacePreferences() async {
    final service = widget.service;
    if (service is! KubeViewPreferencesService) return;
    try {
      final preferences = await (service as KubeViewPreferencesService).readKubeViewPreferences(widget.connection.id);
      if (!_current) return;
      final workspace = preferences['workspace'] as Map?;
      if (workspace == null) return;
      setState(() {
        _panelHeight = (workspace['height'] as num? ?? 300).toDouble().clamp(180, 800);
        _panelOpen = workspace['open'] == true;
        _workspaceTab = workspace['tab'] as String? ?? 'terminal';
        _aiPanel = _workspaceTab == 'ai';
      });
    } catch (_) {
      /* Optional local presentation preferences. */
    }
  }

  void _saveWorkspacePreferences([String? tab]) {
    if (tab != null) _workspaceTab = tab;
    final service = widget.service;
    if (!_current || service is! KubeViewPreferencesService) return;
    unawaited(
      (service as KubeViewPreferencesService)
          .updateKubeViewPreferences(
            widget.connection.id,
            patch: {'kind': 'workspace', 'height': _panelHeight, 'open': _panelOpen, 'tab': _workspaceTab},
          )
          .catchError((Object _) {}),
    );
  }

  void _hidePanel() {
    if (mounted) setState(() => _panelOpen = false);
    _saveWorkspacePreferences();
  }

  void _showTerminals() {
    _workspaceTab = 'terminal';
    _selectedLog = null;
    _selectedOutput = null;
    _aiPanel = false;
    _showPanel();
  }

  void _showAi([KubeResourceRef? selected, String? question]) {
    _selectedLog = null;
    _selectedOutput = null;
    _aiPanel = true;
    _workspaceTab = 'ai';
    _aiSelected = selected;
    _aiQuestion = question;
    _showPanel();
  }

  void _pinLogs(KubeResourceRef resource, List<String> containers) {
    if (!_current) return;
    _logPins.removeWhere((pin) => pin.id == resource.id);
    if (_logPins.length >= 8) _logPins.removeAt(0);
    _logPins.add(KubeLogPin(resource: resource, containers: containers));
    _workspaceTab = 'logs';
    _selectedLog = resource.id;
    _selectedOutput = null;
    _aiPanel = false;
    _showPanel();
  }

  void _closeLog(String id) {
    if (mounted) {
      setState(() {
        _logPins.removeWhere((pin) => pin.id == id);
        if (_selectedLog == id) _selectedLog = null;
      });
    }
  }

  void _showPanel() {
    final session = _session;
    if (!_current || session == null) return;
    if (AppPlatform.isMobile || MediaQuery.sizeOf(context).width < 700) {
      unawaited(
        showModalBottomSheet<void>(
          context: context,
          isScrollControlled: true,
          useSafeArea: true,
          builder: (context) => Padding(
            padding: EdgeInsets.only(bottom: MediaQuery.viewInsetsOf(context).bottom),
            child: SizedBox(
              height: (MediaQuery.sizeOf(context).height * .7).clamp(
                160,
                (MediaQuery.sizeOf(context).height - MediaQuery.viewInsetsOf(context).bottom - 40).clamp(
                  160,
                  double.infinity,
                ),
              ),
              child: KubeWorkspacePanel(
                session: session,
                contextName: widget.connection.name,
                service: widget.service,
                extraSessions: _extraTerminals,
                namespace: _namespace ?? 'default',
                logPins: _logPins,
                selectedLog: _selectedLog,
                onCloseLog: _closeLog,
                onReviewedCommand: _reviewedCommand,
                onTabChanged: _saveWorkspacePreferences,
                outputs: _outputs,
                selectedOutput: _selectedOutput,
                onRuntimeOpened: (id) => unawaited(_runtimeTerminal(id)),
                ai: _aiPanel,
                selected: _aiSelected,
                question: _aiQuestion,
                closeRouteOnLock: true,
                onHide: () => Navigator.of(context).pop(),
              ),
            ),
          ),
        ),
      );
    } else {
      setState(() => _panelOpen = true);
    }
    _saveWorkspacePreferences();
  }

  void _scheduleNavigation() {
    final request = ref.read(kubeNavigationProvider);
    if (!_current ||
        _session == null ||
        request == null ||
        _navigationPending ||
        !identical(request.scope, _scope) ||
        request.contextId != widget.connection.id) {
      return;
    }
    _navigationPending = true;
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      _navigationPending = false;
      if (!_current || _session == null || !identical(request, ref.read(kubeNavigationProvider))) return;
      ref.read(kubeNavigationProvider.notifier).consume(request);
      final target = request.resource;
      if (target != null) {
        final kind = _kinds.where((kind) => kind.id == target.api.id).firstOrNull;
        if (kind == null) return;
        setState(() {
          _kind = kind;
          _namespace = kind.namespaced ? target.namespace : null;
          _view = 'resources';
          _resourceGroup = 'all';
        });
        await _refresh();
        if (!_current) return;
        if (request.openAi) {
          _showAi(target);
          return;
        }
        final row = KubeResourceRow(
          name: target.name,
          namespace: target.namespace,
          uid: '',
          resourceVersion: '',
          status: '',
        );
        await _details(row);
      } else {
        if (request.namespace != null && _namespace != request.namespace && _namespaces.contains(request.namespace)) {
          setState(() => _namespace = request.namespace);
          await _refresh();
        }
        if (_current && request.openAi) _showAi();
      }
    });
  }

  void _publishNavigation() {
    if (_snapshotPending || _session == null || !_current) return;
    _snapshotPending = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _snapshotPending = false;
      final session = _session, kind = _kind;
      if (!_current || session == null) return;
      _navigationSnapshot.publish(
        KubeNavigationSnapshot(
          scope: _scope,
          owner: session.id,
          contextId: widget.connection.id,
          namespace: _namespace ?? '',
          namespaces: _namespaces,
          resources: kind == null
              ? []
              : _rows.values
                    .take(500)
                    .map((row) => KubeResourceRef(api: kind, name: row.name, namespace: row.namespace))
                    .toList(),
        ),
      );
    });
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _revoke();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) widget.onClose();
      });
    });
    ref.listen(kubeNavigationProvider, (_, request) => _scheduleNavigation());
    final l = context.l10n;
    if (!_current) return const SizedBox.shrink();
    _scheduleNavigation();
    _publishNavigation();
    final session = _session;
    final rightPanel = _panelRight && !AppPlatform.isMobile && MediaQuery.sizeOf(context).width >= 1100;
    final query = _query.trim().toLowerCase();
    final rows = _rows.values
        .where(
          (row) =>
              '${row.name} ${row.namespace ?? ''} ${row.status} ${row.cells.join(' ')}'.toLowerCase().contains(query),
        )
        .map(_withMetrics)
        .toList();
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.slash): () {
          final focused = FocusManager.instance.primaryFocus?.context;
          if (focused?.widget is! EditableText && focused?.findAncestorStateOfType<EditableTextState>() == null) {
            _searchFocus.requestFocus();
          }
        },
        const SingleActivator(LogicalKeyboardKey.keyJ, control: true): _toggleTerminals,
        const SingleActivator(LogicalKeyboardKey.keyJ, meta: true): _toggleTerminals,
      },
      child: PageScaffold(
        title: widget.connection.name,
        subtitle: session == null || session.serverVersion.isEmpty
            ? widget.connection.server
            : l.kubeClusterVersion(session.serverVersion),
        actions: [
          if (widget.connections.isNotEmpty && widget.onSelectContext != null)
            SizedBox(
              width: 240,
              child: DropdownButtonFormField<String>(
                key: const ValueKey('kube-context-switch'),
                initialValue: widget.connection.id,
                isExpanded: true,
                decoration: InputDecoration(labelText: l.kubeCommandContext),
                items: widget.connections
                    .map(
                      (connection) => DropdownMenuItem(
                        value: connection.id,
                        child: Text(
                          // l10n-ignore: Saved context or discovered Kubernetes identifiers with PROD/favorite markers.
                          /* l10n-ignore: Saved context or discovered Kubernetes identifiers with PROD/favorite markers. */ '${connection.production ? 'PROD · ' : ''}${connection.name}',
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    )
                    .toList(),
                onChanged: (id) {
                  if (id != null && id != widget.connection.id && _current) {
                    final target = widget.connections.where((connection) => connection.id == id).firstOrNull;
                    if (target != null) widget.onSelectContext?.call(target);
                  }
                },
              ),
            ),
          if (session != null &&
              _view == 'resources' &&
              _kind?.verbs.contains('create') == true &&
              widget.service is KubeManifestService)
            GlassIconButton(
              key: const ValueKey('kube-create-resource'),
              icon: Icons.add_circle_outline,
              tooltip: l.kubeCreateResource,
              onPressed: _createResource,
            ),
          if (session != null && widget.service is KubeAiService)
            GlassIconButton(
              key: const ValueKey('kube-open-ai'),
              icon: Icons.auto_awesome,
              tooltip: l.kubeAiAssistant,
              onPressed: () => _showAi(),
            ),
          if (session != null && widget.service is KubeOperationsService)
            GlassIconButton(
              key: const ValueKey('kube-permissions'),
              icon: Icons.policy_outlined,
              tooltip: l.kubePermissions,
              onPressed: () => showAppDialog<void>(
                context,
                secure: true,
                builder: (_) => KubePermissionsDialog(
                  sessionId: session.id,
                  namespace: _namespace ?? 'default',
                  service: widget.service as KubeOperationsService,
                ),
              ),
            ),
          if (session != null)
            GlassIconButton(
              key: const ValueKey('kube-toggle-terminal'),
              icon: Icons.terminal,
              tooltip: l.kubeShowPanel,
              onPressed: _toggleTerminals,
            ),
          GlassButton(
            key: const ValueKey('kube-disconnect'),
            label: session == null ? l.kubeBackToContexts : l.kubeDisconnect,
            icon: Icons.arrow_back,
            onPressed: widget.onClose,
          ),
          if (session != null && _view == 'resources')
            GlassIconButton(
              key: const ValueKey('kube-refresh-resources'),
              icon: Icons.refresh,
              tooltip: l.kubeRefreshResources,
              onPressed: _connecting ? null : _refresh,
            ),
        ],
        body: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (session != null)
                    KubeWriteBanner(session: session, contextName: widget.connection.name)
                  else if (widget.connection.production)
                    Tooltip(
                      message: l.kubeReadOnlyHelp,
                      child: Align(
                        alignment: Alignment.centerLeft,
                        child: Chip(avatar: const Icon(Icons.lock_outline, size: 18), label: Text(l.kubeReadOnly)),
                      ),
                    ),
                  if (_forwardRestoreFailed) Text(l.kubeForwardRestoreFailed),
                  if (_connecting) ...[
                    const LinearProgressIndicator(),
                    const SizedBox(height: 8),
                    Text(l.kubeConnecting),
                  ] else if (_connectionFailed) ...[
                    Text(
                      widget.connection.authentication == 'exec' && AppPlatform.isMobile
                          ? l.kubeExecMobile
                          : l.kubeConnectionFailed,
                    ),
                    const SizedBox(height: 8),
                    if (_authenticationRequired)
                      Align(
                        alignment: Alignment.centerLeft,
                        child: GlassButton(
                          key: const ValueKey('kube-authenticate'),
                          label: l.kubeAuthAction,
                          icon: Icons.login,
                          onPressed: _authenticate,
                        ),
                      ),
                    Align(
                      alignment: Alignment.centerLeft,
                      child: GlassButton(
                        key: const ValueKey('kube-reconnect'),
                        label: l.kubeRetry,
                        icon: Icons.refresh,
                        onPressed: _connect,
                      ),
                    ),
                  ] else if (_kinds.isEmpty)
                    Text(l.kubeNoResourceKinds)
                  else ...[
                    if (widget.service is KubeInsightsService)
                      SingleChildScrollView(
                        scrollDirection: Axis.horizontal,
                        child: Row(
                          children: [
                            for (final view in [
                              ('resources', l.kubeResources),
                              if (_kinds.any(
                                (kind) => kind.group.endsWith('.toolkit.fluxcd.io') || kind.group == 'argoproj.io',
                              ))
                                ('gitops', l.kubeGitOps),
                              ('problems', l.kubeProblems),
                              ('helm', l.kubeHelm),
                              if (widget.service is KubeHistoryService) ('history', l.kubeHistory),
                            ])
                              Padding(
                                padding: const EdgeInsets.only(right: 8, bottom: 8),
                                child: ChoiceChip(
                                  key: ValueKey('kube-view-${view.$1}'),
                                  label: Text(view.$2),
                                  selected: _view == view.$1,
                                  onSelected: (_) {
                                    if (_view == view.$1) return;
                                    _generation++;
                                    _stopWatch();
                                    setState(() => _view = view.$1);
                                    if (_view == 'resources') unawaited(_refresh());
                                  },
                                ),
                              ),
                          ],
                        ),
                      ),
                    if (_view != 'resources' && widget.service is KubeInsightsService && session != null) ...[
                      DropdownButtonFormField<String>(
                        key: const ValueKey('kube-insights-namespace'),
                        initialValue: _namespace ?? '',
                        isExpanded: true,
                        decoration: InputDecoration(labelText: l.kubeNamespace),
                        items: [
                          DropdownMenuItem(value: '', child: Text(l.kubeAllNamespaces)),
                          ..._namespaces.map(
                            (name) => DropdownMenuItem(
                              value: name,
                              child: Text(
                                // l10n-ignore: Saved context or discovered Kubernetes identifiers with PROD/favorite markers.
                                /* l10n-ignore: Saved context or discovered Kubernetes identifiers with PROD/favorite markers. */ '${_favoriteNamespaces.contains(name) ? '★ ' : ''}$name',
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                          ),
                        ],
                        onChanged: (value) => setState(() => _namespace = value == '' ? null : value),
                      ),
                      const SizedBox(height: 8),
                      Expanded(
                        child: _view == 'history' && widget.service is KubeHistoryService
                            ? KubeHistoryPanel(
                                key: ValueKey((session.id, _namespace)),
                                contextId: session.contextId,
                                contextName: widget.connection.name,
                                sessionId: session.id,
                                namespace: _namespace,
                                service: widget.service as KubeHistoryService,
                              )
                            : _view == 'helm'
                            ? KubeHelmPanel(
                                key: ValueKey((session.id, _namespace, _view)),
                                session: session,
                                contextName: widget.connection.name,
                                namespace: _namespace,
                                service: widget.service as KubeInsightsService,
                              )
                            : KubeInsightsPanel(
                                key: ValueKey((session.id, _namespace, _view)),
                                session: session,
                                contextName: widget.connection.name,
                                namespace: _namespace,
                                service: widget.service as KubeInsightsService,
                                browser: widget.service,
                                problems: _view == 'problems',
                                cliPanel: _view == 'gitops' && !AppPlatform.isMobile
                                    ? KubeFluxCliPanel(
                                        session: session,
                                        namespace: _namespace,
                                        onTerminalOpened: (id) => unawaited(_runtimeTerminal(id)),
                                        onOutput: _output,
                                      )
                                    : null,
                                onTerminalOpened: _showTerminals,
                                onExplain: widget.service is KubeAiService ? _showAi : null,
                              ),
                      ),
                    ] else ...[
                      _filters(context),
                      const SizedBox(height: 8),
                      Wrap(
                        spacing: 12,
                        runSpacing: 4,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          Text(l.kubeLoadedCount(_rows.length)),
                          if (_selectedRows.isNotEmpty)
                            GlassButton(
                              key: const ValueKey('kube-bulk-actions'),
                              label: '${l.kubeBulkActions} (${_selectedRows.length})',
                              onPressed: _bulk,
                            ),
                          Text(_live ? l.kubeLive : l.kubeSnapshot),
                        ],
                      ),
                      if (_loading || _loadingMore) const LinearProgressIndicator(),
                      if (_requestFailed)
                        Text(l.kubeRequestFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                      if (_watchFailed) Text(l.kubeWatchFailed),
                      if (_hasMetrics && _metricsUnavailable) Text(l.kubeMetricsUnavailable),
                      if (_limited) Text(l.kubeResourceLimit),
                      if (!_loading && !_requestFailed && rows.isEmpty) Text(l.kubeNoResources),
                      if (_kind != null)
                        Expanded(
                          child: KubeResourceTableView(
                            onVisibleRows: (ids) {
                              _visibleRows = ids;
                              if (ids.any((id) => _rows[id]?.cells.isEmpty == true) && _columns.isNotEmpty) {
                                _scheduleEnrichment();
                              }
                            },
                            contextId: widget.connection.id,
                            rows: rows,
                            kind: _kind!,
                            columns: [
                              ..._columns,
                              if (_hasMetrics) ...const [
                                KubeTableColumn(
                                  name: 'CPU (m)',
                                  type: 'number',
                                  format: '',
                                  description: '',
                                  priority: 0,
                                ),
                                KubeTableColumn(
                                  name: 'RAM (MiB)',
                                  type: 'number',
                                  format: '',
                                  description: '',
                                  priority: 0,
                                ),
                              ],
                            ],
                            selected: _selectedRows,
                            canSelect:
                                session?.production == false &&
                                widget.service is KubeMutationService &&
                                _kind!.verbs.contains('delete'),
                            onSelection: (row) => setState(() {
                              if (_selectedRows.contains(row.id)) {
                                _selectedRows.remove(row.id);
                              } else if (_selectedRows.length < 100) {
                                _selectedRows.add(row.id);
                              }
                            }),
                            onOpen: (row) => unawaited(_details(row)),
                            hasMore: _continueToken != null && !_limited,
                            loadingMore: _loadingMore,
                            onMore: () => unawaited(_more()),
                          ),
                        ),
                    ],
                  ],
                  if (session != null) KubeForwardsPanel(sessionId: session.id),
                  if (_panelOpen && session != null && !rightPanel)
                    Semantics(
                      label: l.kubeResizePanel,
                      child: MouseRegion(
                        cursor: SystemMouseCursors.resizeRow,
                        child: GestureDetector(
                          key: const ValueKey('kube-panel-resize'),
                          behavior: HitTestBehavior.opaque,
                          onVerticalDragEnd: (_) => _saveWorkspacePreferences(),
                          onVerticalDragUpdate: (details) => setState(
                            () => _panelHeight = (_panelHeight - details.delta.dy).clamp(
                              180,
                              MediaQuery.sizeOf(context).height * .6,
                            ),
                          ),
                          child: const SizedBox(height: 12, child: Center(child: Icon(Icons.drag_handle, size: 12))),
                        ),
                      ),
                    ),
                  if (_panelOpen && session != null && !rightPanel)
                    SizedBox(
                      height: _panelHeight.clamp(180, MediaQuery.sizeOf(context).height * .6),
                      child: _workspace(session),
                    ),
                  if (!_panelOpen &&
                      session != null &&
                      (_outputs.isNotEmpty || _logPins.isNotEmpty || _extraTerminals.isNotEmpty))
                    GlassButton(label: l.kubeShowPanel, icon: Icons.keyboard_arrow_up, onPressed: _showPanel),
                ],
              ),
            ),
            if (_panelOpen && session != null && rightPanel)
              SizedBox(width: MediaQuery.sizeOf(context).width * .38, child: _workspace(session)),
          ],
        ),
      ),
    );
  }

  Widget _workspace(KubeSession session) => KubeWorkspacePanel(
    key: _workspaceKey,
    session: session,
    contextName: widget.connection.name,
    service: widget.service,
    extraSessions: _extraTerminals,
    namespace: _namespace ?? 'default',
    logPins: _logPins,
    selectedLog: _selectedLog,
    onCloseLog: _closeLog,
    onReviewedCommand: _reviewedCommand,
    onTabChanged: _saveWorkspacePreferences,
    outputs: _outputs,
    selectedOutput: _selectedOutput,
    onRuntimeOpened: (id) => unawaited(_runtimeTerminal(id)),
    ai: _aiPanel,
    selected: _aiSelected,
    question: _aiQuestion,
    onHide: _hidePanel,
    dockedRight: _panelRight,
    onToggleDock: MediaQuery.sizeOf(context).width < 1100 ? null : () => setState(() => _panelRight = !_panelRight),
  );

  void _toggleTerminals() {
    if (_panelOpen) {
      _hidePanel();
    } else {
      _showTerminals();
    }
  }

  Future<void> _bulk() async {
    final session = _session, kind = _kind;
    if (!_current || session == null || session.production || kind == null || _selectedRows.isEmpty) return;
    final selected = _rows.values.where((row) => _selectedRows.contains(row.id)).toList();
    if (selected.isEmpty) return;
    final changed = await showAppDialog<bool>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (_) => KubeBulkDialog(
        session: session,
        contextName: widget.connection.name,
        kind: kind,
        rows: selected,
        service: widget.service,
      ),
    );
    if (_current && changed == true) await _refresh();
  }

  Future<void> _favorite({required bool namespace}) async {
    final service = widget.service;
    final value = namespace ? _namespace : _kind?.id;
    if (!_current || _savingFavorites || service is! KubeOrganizationService || value == null) return;
    final values = namespace ? _favoriteNamespaces : _favoriteKinds;
    final wasPresent = values.contains(value);
    setState(() {
      _savingFavorites = true;
      if (wasPresent) {
        values.remove(value);
        if (!namespace && _resourceGroup == 'favorites') _resourceGroup = 'all';
      } else {
        values.add(value);
      }
    });
    try {
      await (service as KubeOrganizationService).saveKubeFavourites(
        widget.connection.id,
        namespaces: _favoriteNamespaces.toList(),
        kinds: _favoriteKinds.toList(),
      );
      if (_current) ref.invalidate(kubeConnectionsProvider);
    } catch (_) {
      if (mounted && _current) {
        setState(() {
          if (wasPresent) {
            values.add(value);
          } else {
            values.remove(value);
          }
        });
        showSnack(context, context.l10n.kubeRequestFailed, error: true);
      }
    } finally {
      if (_current) setState(() => _savingFavorites = false);
    }
  }

  String _group(KubeResourceKind kind) {
    if (const [
      'Pod',
      'Deployment',
      'StatefulSet',
      'DaemonSet',
      'ReplicaSet',
      'Job',
      'CronJob',
      'ReplicationController',
    ].contains(kind.kind)) {
      return 'workloads';
    }
    if (const [
      'Service',
      'Endpoints',
      'EndpointSlice',
      'Ingress',
      'IngressClass',
      'NetworkPolicy',
    ].contains(kind.kind)) {
      return 'network';
    }
    if (const [
      'PersistentVolume',
      'PersistentVolumeClaim',
      'StorageClass',
      'VolumeAttachment',
      'CSIDriver',
      'CSINode',
    ].contains(kind.kind)) {
      return 'storage';
    }
    if (const [
      'ConfigMap',
      'Secret',
      'ResourceQuota',
      'LimitRange',
      'HorizontalPodAutoscaler',
      'PodDisruptionBudget',
    ].contains(kind.kind)) {
      return 'config';
    }
    if (kind.group == 'rbac.authorization.k8s.io' || kind.kind == 'ServiceAccount') return 'access';
    return 'cluster';
  }

  Widget _filters(BuildContext context) {
    final l = context.l10n;
    final sortedKinds =
        _kinds
            .where(
              (kind) =>
                  _resourceGroup == 'all' ||
                  (_resourceGroup == 'favorites' ? _favoriteKinds.contains(kind.id) : _group(kind) == _resourceGroup),
            )
            .toList()
          ..sort((a, b) {
            final favorite = (_favoriteKinds.contains(b.id) ? 1 : 0) - (_favoriteKinds.contains(a.id) ? 1 : 0);
            return favorite == 0 ? a.kind.compareTo(b.kind) : favorite;
          });
    final sortedNamespaces = [..._namespaces]
      ..sort((a, b) {
        final favorite = (_favoriteNamespaces.contains(b) ? 1 : 0) - (_favoriteNamespaces.contains(a) ? 1 : 0);
        return favorite == 0 ? a.compareTo(b) : favorite;
      });
    final fields = LayoutBuilder(
      builder: (context, constraints) {
        final wide = constraints.maxWidth >= 700;
        final fieldWidth = wide ? (constraints.maxWidth - 12) / 2 : constraints.maxWidth;
        return Wrap(
          spacing: 12,
          runSpacing: 10,
          children: [
            SizedBox(
              width: constraints.maxWidth,
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  children: [
                    for (final group in [
                      ('all', l.kubeAllKinds),
                      ('favorites', l.kubeFavorites),
                      ('workloads', l.kubeGroupWorkloads),
                      ('network', l.kubeGroupNetwork),
                      ('config', l.kubeGroupConfig),
                      ('storage', l.kubeGroupStorage),
                      ('access', l.kubeGroupAccess),
                      ('cluster', l.kubeGroupCluster),
                    ])
                      Padding(
                        padding: const EdgeInsets.only(right: 6),
                        child: ChoiceChip(
                          label: Text(group.$2),
                          selected: _resourceGroup == group.$1,
                          onSelected: (_) {
                            final options = _kinds
                                .where(
                                  (k) =>
                                      group.$1 == 'all' ||
                                      (group.$1 == 'favorites' ? _favoriteKinds.contains(k.id) : _group(k) == group.$1),
                                )
                                .toList();
                            if (options.isEmpty) return;
                            setState(() {
                              _resourceGroup = group.$1;
                              if (!options.contains(_kind)) _kind = options.first;
                            });
                            unawaited(_refresh());
                          },
                        ),
                      ),
                  ],
                ),
              ),
            ),
            SizedBox(
              width: fieldWidth,
              child: DropdownButtonFormField<String>(
                key: const ValueKey('kube-kind'),
                initialValue: _kind?.id,
                isExpanded: true,
                decoration: InputDecoration(
                  labelText: l.kubeResourceKind,
                  suffixIcon: widget.service is KubeOrganizationService
                      ? IconButton(
                          tooltip: l.kubeFavorite,
                          icon: Icon(_favoriteKinds.contains(_kind?.id) ? Icons.star : Icons.star_border),
                          onPressed: _savingFavorites ? null : () => _favorite(namespace: false),
                        )
                      : null,
                ),
                items: sortedKinds
                    .map(
                      (kind) => DropdownMenuItem(
                        value: kind.id,
                        child: Text(
                          // l10n-ignore: Saved context or discovered Kubernetes identifiers with PROD/favorite markers.
                          /* l10n-ignore: Saved context or discovered Kubernetes identifiers with PROD/favorite markers. */ '${_favoriteKinds.contains(kind.id) ? '★ ' : ''}${kind.kind} · ${kind.apiVersion}',
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    )
                    .toList(),
                onChanged: (id) {
                  if (id == null) return;
                  setState(() => _kind = _kinds.firstWhere((kind) => kind.id == id));
                  unawaited(_refresh());
                },
              ),
            ),
            SizedBox(
              width: fieldWidth,
              child: DropdownButtonFormField<String>(
                key: const ValueKey('kube-namespace'),
                initialValue: _namespace ?? '',
                isExpanded: true,
                decoration: InputDecoration(
                  labelText: l.kubeNamespace,
                  suffixIcon: widget.service is KubeOrganizationService
                      ? IconButton(
                          tooltip: l.kubeFavorite,
                          icon: Icon(_favoriteNamespaces.contains(_namespace) ? Icons.star : Icons.star_border),
                          onPressed: _savingFavorites || _namespace == null ? null : () => _favorite(namespace: true),
                        )
                      : null,
                ),
                items: [
                  DropdownMenuItem(value: '', child: Text(l.kubeAllNamespaces)),
                  ...sortedNamespaces.map(
                    (name) => DropdownMenuItem(
                      value: name,
                      child: Text(
                        // l10n-ignore: Saved context or discovered Kubernetes identifiers with PROD/favorite markers.
                        /* l10n-ignore: Saved context or discovered Kubernetes identifiers with PROD/favorite markers. */ '${_favoriteNamespaces.contains(name) ? '★ ' : ''}$name',
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ),
                ],
                onChanged: _kind?.namespaced != true
                    ? null
                    : (value) {
                        setState(() => _namespace = value == '' ? null : value);
                        unawaited(_refresh());
                      },
              ),
            ),
            SizedBox(
              width: fieldWidth,
              child: TextField(
                key: const ValueKey('kube-resource-search'),
                focusNode: _searchFocus,
                decoration: InputDecoration(labelText: l.commonSearch, prefixIcon: const Icon(Icons.search)),
                onChanged: (value) => setState(() => _query = value),
              ),
            ),
            SizedBox(
              width: fieldWidth,
              child: TextField(
                key: const ValueKey('kube-label-selector'),
                controller: _selector,
                autocorrect: false,
                enableSuggestions: false,
                decoration: InputDecoration(
                  labelText: l.kubeLabelSelector,
                  suffixIcon: IconButton(
                    tooltip: l.kubeApplyFilter,
                    icon: const Icon(Icons.filter_alt_outlined),
                    onPressed: () {
                      _labelSelector = _selector.text.trim();
                      unawaited(_refresh());
                    },
                  ),
                ),
                onSubmitted: (value) {
                  _labelSelector = value.trim();
                  unawaited(_refresh());
                },
              ),
            ),
          ],
        );
      },
    );
    if (MediaQuery.sizeOf(context).width < 600) {
      return ContentSurface(
        child: ExpansionTile(
          key: const ValueKey('kube-filters-expand'),
          title: Text(
            '${_kind?.kind ?? l.kubeResources} · ${_namespace ?? l.kubeAllNamespaces}',
            overflow: TextOverflow.ellipsis,
          ),
          children: [
            Padding(padding: const EdgeInsets.all(8), child: fields),
            if (_namespacesFailed) Text(l.kubeNamespacesFailed),
          ],
        ),
      );
    }
    return Column(children: [fields, if (_namespacesFailed) Text(l.kubeNamespacesFailed)]);
  }
}
