import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_ansible_config.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_static_inventory_preview.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter/foundation.dart' show compute;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class AutomationInventoryDialog extends ConsumerStatefulWidget {
  const AutomationInventoryDialog({
    super.key,
    required this.scope,
    required this.projectId,
    required this.files,
    required this.service,
    this.originalCurrent,
  });
  final AutomationScope scope;
  final String projectId;
  final AutomationService files;
  final AutomationSourceInventoryService service;
  final bool Function()? originalCurrent;
  @override
  ConsumerState<AutomationInventoryDialog> createState() => _AutomationInventoryDialogState();
}

class _AutomationInventoryDialogState extends ConsumerState<AutomationInventoryDialog> {
  final search = TextEditingController();
  final savedSearch = TextEditingController();
  final savedHosts = <String>{};
  String sourceKind = 'files';
  bool get unified => originalService is AutomationInventorySelectionService;
  final selected = <String>{};
  final bindings = <String, AutomationJson>{};
  List<String> paths = [];
  final directories = <String>{};
  List<AutomationJson> resolvedSources = [];
  List<AutomationJson> frozenSources = [], rows = [];
  List<AutomationJson> conflicts = [];
  Map<String, List<String>>? yamlPreview;
  int conflictCount = 0;
  String? runner, previewId;
  String mode = 'external_only';
  int? revision;
  int offset = 0, cursor = 0, total = 0, pageEpoch = 0;
  bool loading = true, busy = false, ready = false, fetching = false, pendingPage = false;
  bool scopeValid = true, committed = false, configured = false;
  Object? error;
  Timer? timer, debounce;
  late final AutomationSourceInventoryService originalService;
  late final AutomationService originalFiles;
  late final AutomationScope originalScope;
  late final String originalProject;
  late final bool Function()? parentCurrent;
  AutomationRunnerService? originalRunners;
  late final bool Function() ownerMatches;
  late final VoidCallback removeRouteObserver;
  late final List<ProviderSubscription<dynamic>> ownerSubscriptions;
  StreamSubscription<Object?>? ownerNotices;
  bool retired = false;
  Route<dynamic>? decisionRoute;
  NavigatorState? decisionNavigator;
  Widget ownDecision(BuildContext context, Widget child) {
    decisionRoute = ModalRoute.of(context);
    decisionNavigator = Navigator.of(context);
    return child;
  }
  bool get current {
    if (!mounted || retired) return false;
    if (ownerMatches()) return true;
    retire();
    return false;
  }

  void retire() {
    if (retired || !mounted) return;
    retired = true;
    timer?.cancel(); debounce?.cancel(); pageEpoch++;
    pendingPage = false;
    final id = previewId;
    previewId = null;
    if (id != null && !committed) discard(id);
    // Remove only this dialog, never a newer unrelated top modal.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final child = decisionRoute, navigator = decisionNavigator;
      if (child?.isActive == true && navigator?.mounted == true) navigator!.removeRoute(child!);
      final route = ModalRoute.of(context);
      if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
    });
    WidgetsBinding.instance.scheduleFrame();
  }

  @override
  void didUpdateWidget(AutomationInventoryDialog oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(widget.scope, originalScope) || !identical(widget.service, originalService) ||
        !identical(widget.files, originalFiles) || widget.projectId != originalProject ||
        !identical(widget.originalCurrent, parentCurrent)) { retire(); }
  }

  @override
  void initState() {
    super.initState();
    originalScope = widget.scope; originalService = widget.service;
    originalFiles = widget.files; originalProject = widget.projectId; parentCurrent = widget.originalCurrent;
    final app = ref.read(appServicesProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final epoch = runtime?.currentScope.epoch, router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    final runners = ref.read(automationRunnerServiceProvider);
    originalRunners = runners;
    ownerMatches = () => parentCurrent?.call() != false && uri.path == AppRoutes.automation && identical(ref.read(automationScopeProvider), originalScope) && originalScope.unlocked &&
      identical(ref.read(appServicesProvider), app) && identical(ref.read(automationServiceProvider), originalFiles) &&
      identical(ref.read(automationRunnerServiceProvider), runners) &&
      identical(ref.read(criticalRuntimeEventsProvider), runtime) && runtime?.currentScope.epoch == epoch &&
      runtime?.currentScope.streamUnavailable != true && identical(ref.read(routerProvider), router) &&
      router.routeInformationProvider.value.uri == uri;
    void observe() { if (!ownerMatches()) retire(); }
    ownerSubscriptions = [
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(automationServiceProvider, (_, _) => observe()),
      ref.listenManual(automationRunnerServiceProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ];
    ownerNotices = runtime?.watchRuntimeNotices().listen((_) => observe(),
      onError: (Object _, StackTrace _) => retire(), onDone: retire);
    router.routeInformationProvider.addListener(observe);
    removeRouteObserver = () => router.routeInformationProvider.removeListener(observe);
    ref.listenManual(
      automationScopeProvider,
      (_, next) => scopeValid = identical(next, originalScope),
      fireImmediately: true,
    );
    if (!current) { scheduleMicrotask(retire); return; }
    load();
  }

  void discard(String id) {
    unawaited(originalService.discardInventory(id).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
  }

  @override
  void dispose() {
    retired = true;
    for (final subscription in ownerSubscriptions) { subscription.close(); }
    unawaited(ownerNotices?.cancel());
    removeRouteObserver();
    timer?.cancel();
    debounce?.cancel();
    if (!committed && scopeValid && previewId != null) discard(previewId!);
    search.dispose();
    savedSearch.dispose();
    super.dispose();
  }

  List<AutomationJson> sources() => [
    for (final path in selected.toList()..sort())
      {
        'path': path,
        'format': directories.contains(path)
            ? 'directory'
            : RegExp(r'\.(yaml|yml|json)$', caseSensitive: false).hasMatch(path)
            ? 'yaml'
            : 'ini',
      },
  ];
  bool sameSources(List<AutomationJson> a, List<AutomationJson> b) =>
      a.length == b.length && a.every((s) => b.any((t) => s['path'] == t['path'] && s['format'] == t['format']));
  AutomationJson endpoint(AutomationJson row) => {
    'address': row['address'],
    'port': row['port'],
    'username': row['username'],
  };
  bool stale(AutomationJson binding, AutomationJson row) {
    final source = binding['source'] as AutomationJson;
    return source['address'] != row['address'] ||
        source['port'] != row['port'] ||
        source['username'] != row['username'];
  }

  Future<void> load() async {
    try {
      final values = await Future.wait<Object>([
        originalFiles.files(originalProject),
        originalService.inventoryConfiguration(originalProject),
      ]);
      if (!mounted || !current) return;
      final tree = values[0] as AutomationProjectRevision, value = values[1] as AutomationJson;
      if (value['version'] != 1 || value['project_id'] != originalProject) {
        throw FormatException(context.l10n.automationInventoryChanged);
      }
      final configuration = value['configuration'] as AutomationJson?;
      if (configuration != null && configuration['version'] != 1) {
        throw FormatException(context.l10n.automationInventoryChanged);
      }
      final configPaths = <String>{};
      if (configuration == null && tree.entries.any((entry) => !entry.directory && entry.path == 'ansible.cfg')) {
        try {
          final config = await originalFiles.readFile(originalProject, 'ansible.cfg');
          final document = AutomationAnsibleConfigDocument.parse(config.text);
          final inventory = document.value(AutomationAnsibleSetting.inventory);
          if (document.canEdit && inventory != null) {
            for (final source in inventory.split(',').take(8)) {
              configPaths.add(source.trim().replaceFirst(RegExp(r'^\./'), '').replaceFirst(RegExp(r'/+$'), ''));
            }
          }
        } catch (_) {
          // The file picker stays usable when an optional config is unreadable.
        }
      }
      if (!current) return;
      setState(() {
        revision = value['project_revision'] as int;
        paths = [
          for (final entry in tree.entries)
            if (!entry.path.split('/').any((part) => part.startsWith('.') || part.endsWith('~')) &&
                (entry.directory ||
                    (!entry.path.split('/').any((part) => part == 'group_vars' || part == 'host_vars') &&
                        (!entry.path.split('/').last.contains('.') ||
                            RegExp(r'\.(ini|yml|yaml|json)$', caseSensitive: false).hasMatch(entry.path)))))
              entry.path,
        ]..sort();
        directories.addAll(tree.entries.where((e) => e.directory).map((e) => e.path));
        configured = configuration != null;
        if (unified) {
          final inventory = value['saved_inventory'] as AutomationJson?;
          if (inventory != null && ![1, 2].contains(inventory['version'])) {
            throw FormatException(context.l10n.automationInventoryChanged);
          }
          savedHosts.addAll((inventory?['selected_hosts'] as List? ?? []).cast<String>());
          sourceKind = configuration == null
              ? 'saved'
              : configuration['mode'] == 'mixed'
              ? 'mixed'
              : 'files';
        }
        if (configuration != null) {
          mode = configuration['mode'] as String;
          frozenSources = (configuration['sources'] as List).cast<AutomationJson>();
          selected.addAll(frozenSources.map((source) => source['path'] as String));
          bindings.addAll(
            (configuration['bindings'] as AutomationJson).map(
              (key, value) => MapEntry(key, Map<String, dynamic>.from(value as Map)),
            ),
          );
        } else {
          selected.addAll(configPaths.where(paths.contains));
          if (selected.isNotEmpty) sourceKind = savedHosts.isEmpty ? 'files' : 'mixed';
        }
      });
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => loading = false);
    }
  }

  bool get canPreviewYaml => selected.isNotEmpty && selected.every(
    (path) => !directories.contains(path) && RegExp(r'\.(yaml|yml|json)$', caseSensitive: false).hasMatch(path),
  );

  Future<void> previewYaml() async {
    if (!current || busy || !canPreviewYaml) return;
    setState(() {
      busy = true;
      error = null;
      yamlPreview = null;
    });
    try {
      final documents = <String>[];
      for (final path in selected) {
        final file = await originalFiles.readFile(originalProject, path);
        if (!current) return;
        documents.add(file.text);
      }
      final groups = await compute(previewStaticYamlInventory, documents);
      if (current) setState(() => yamlPreview = groups);
    } catch (_) {
      if (current) setState(() => error = FormatException(context.l10n.automationInventoryYamlPreviewFailed));
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> start() async {
    if (!current || busy ||
        loading ||
        previewId != null ||
        runner == null ||
        selected.isEmpty ||
        selected.length > 8 ||
        selected.any((p) => !paths.contains(p))) {
      return;
    }
    final chosen = sources();
    setState(() => busy = true);
    try {
      await originalRunners!.connectRunner(runner!);
      if (!mounted || !current) return;
      final value = await originalService.startInventory(originalProject, runner!, chosen);
      final id = value['preview_id'] as String;
      if (!mounted || !current) {
        if (scopeValid) discard(id);
        return;
      }
      setState(() {
        if (!sameSources(chosen, frozenSources)) bindings.clear();
        frozenSources = chosen;
        previewId = id;
        conflicts = [];
        conflictCount = 0;
        error = null;
      });
      if (value['version'] != 1 || value['project_id'] != originalProject || value['project_revision'] != revision) {
        throw FormatException(context.l10n.automationInventoryChanged);
      }
      timer = Timer.periodic(const Duration(seconds: 1), (_) => page());
      await page();
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> page() async {
    if (!current || previewId == null) return;
    if (fetching) {
      pendingPage = true;
      return;
    }
    setState(() => fetching = true);
    final id = previewId!, epoch = pageEpoch, requestedOffset = offset;
    try {
      final value = await originalService.inventoryPage(id, query: search.text.trim(), offset: requestedOffset);
      if (!mounted || !current || previewId != id || epoch != pageEpoch) return;
      if (value['version'] != 1 ||
          value['preview_id'] != id ||
          value['project_id'] != originalProject ||
          value['project_revision'] != revision) {
        throw FormatException(context.l10n.automationInventoryChanged);
      }
      switch (value['state']) {
        case 'preparing':
          break;
        case 'ready':
          timer?.cancel();
          setState(() {
            ready = true;
            rows = (value['hosts'] as List).cast<AutomationJson>();
            cursor = value['cursor'] as int;
            total = value['total'] as int;
            resolvedSources = (value['resolved_sources'] as List? ?? []).cast<AutomationJson>();
            error = null;
          });
        default:
          timer?.cancel();
          throw FormatException(
            '${context.l10n.automationInventoryFailed} (${value['error'] ?? 'inventory_preview_failed'})',
          );
      }
    } catch (e) {
      if (current && previewId == id && epoch == pageEpoch) {
        timer?.cancel();
        setState(() => error = e);
      }
    } finally {
      fetching = false;
      if (current) setState(() {});
      if (pendingPage && current && previewId != null) {
        pendingPage = false;
        unawaited(page());
      }
    }
  }

  void queryChanged(String _) {
    if (!current) return;
    offset = 0;
    pageEpoch++;
    debounce?.cancel();
    debounce = Timer(const Duration(milliseconds: 300), () => page());
  }

  void go(int next) {
    if (!current) return;
    setState(() {
      offset = next;
      pageEpoch++;
    });
    page();
  }

  void back() {
    if (!current || busy) return;
    timer?.cancel();
    debounce?.cancel();
    final id = previewId;
    if (id != null) discard(id);
    setState(() {
      previewId = null;
      conflicts = [];
      conflictCount = 0;
      ready = false;
      rows = [];
      offset = 0;
      cursor = 0;
      total = 0;
      pageEpoch++;
      pendingPage = false;
      error = null;
      search.clear();
    });
  }

  Future<void> save() async {
    if (!current || !ready || busy || fetching || previewId == null) return;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final configuration = <String, dynamic>{
        'version': 1,
        'sources': frozenSources,
        'mode': unified ? (sourceKind == 'mixed' ? 'mixed' : 'external_only') : mode,
        'bindings': bindings,
      };
      final check = await originalService.checkInventory(previewId!, configuration);
      if (!mounted || !current) return;
      if (check['version'] != 1 || check['preview_id'] != previewId || check['project_id'] != originalProject) {
        throw FormatException(context.l10n.automationInventoryChanged);
      }
      setState(() {
        conflicts = (check['conflicts'] as List).cast<AutomationJson>();
        conflictCount = check['conflict_count'] as int;
      });
      if (check['valid'] != true) return;
      final updated = unified
          ? await (originalService as AutomationInventorySelectionService).selectInventory({
              'version': 1,
              'project_id': originalProject,
              'expected_revision': revision,
              'saved_hosts': savedHosts.toList()..sort(),
              'configuration': configuration,
              'preview_id': previewId,
            })
          : await originalService.commitInventory(previewId!, configuration);
      committed = true;
      if (mounted && current) Navigator.pop(context, updated);
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> saveSaved() async {
    if (!current || !unified || busy || loading || revision == null) return;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final result = await (originalService as AutomationInventorySelectionService).selectInventory({
        'version': 1,
        'project_id': originalProject,
        'expected_revision': revision,
        'saved_hosts': savedHosts.toList()..sort(),
        'configuration': null,
        'preview_id': null,
      });
      if (mounted && current) Navigator.pop(context, result);
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Widget savedPicker(List<Host> hosts, AsyncValue<List<Group>> folders) {
    final l = context.l10n;
    final query = savedSearch.text.trim().toLowerCase();
    final matching = hosts
        .where((host) => host.name.toLowerCase().contains(query) || host.address.toLowerCase().contains(query))
        .toList();
    final missing = savedHosts.where((id) => !hosts.any((host) => host.id.value == id)).toList();
    return ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.automationInventorySavedHint),
          const SizedBox(height: 8),
          GlassSelect<String?>(
            key: const ValueKey('automation-inventory-folder'),
            value: null,
            expand: true,
            semanticLabel: l.automationInventoryFolder,
            items: [
              GlassSelectItem(value: null, label: l.automationInventoryFolder),
              for (final folder in folders.value ?? <Group>[])
                GlassSelectItem(value: folder.id.value, label: folder.name),
            ],
            onChanged: busy || loading
                ? null
                : (id) {
                    if (id == null) return;
                    final children = <String, List<String>>{};
                    for (final group in folders.value ?? <Group>[]) {
                      if (group.parentId != null) (children[group.parentId!.value] ??= []).add(group.id.value);
                    }
                    final descendants = <String>{id}, queue = <String>[id];
                    for (var index = 0; index < queue.length; index++) {
                      for (final child in children[queue[index]] ?? <String>[]) {
                        if (descendants.add(child)) queue.add(child);
                      }
                    }
                    setState(
                      () => savedHosts.addAll(
                        hosts.where((host) => descendants.contains(host.groupId?.value)).map((host) => host.id.value),
                      ),
                    );
                  },
          ),
          if (folders.hasError) Text(errorMessage(l, folders.error!)),
          const SizedBox(height: 12),
          TextField(
            key: const ValueKey('automation-inventory-saved-search'),
            controller: savedSearch,
            decoration: InputDecoration(labelText: l.automationInventorySavedSearch),
            onChanged: (_) => setState(() {}),
          ),
          const SizedBox(height: 10),
          Text('${l.automationTargets}: ${savedHosts.length}'),
          const SizedBox(height: 8),
          SizedBox(
            height: 200,
            child: ClipRect(
              child: ListView.builder(
                key: const ValueKey('automation-inventory-saved-hosts'),
                primary: false,
                itemCount: matching.length,
                itemBuilder: (context, index) {
                  final host = matching[index];
                  return CheckboxListTile(
                    key: ValueKey('automation-inventory-saved-${host.id.value}'),
                    dense: true,
                    value: savedHosts.contains(host.id.value),
                    title: Text(host.name),
                    subtitle: Text(host.address),
                    onChanged: busy || loading
                        ? null
                        : (value) => setState(
                            () => value == true ? savedHosts.add(host.id.value) : savedHosts.remove(host.id.value),
                          ),
                  );
                },
              ),
            ),
          ),
          for (final id in missing)
            ListTile(
              title: Text(l.automationInventoryMissingHost),
              subtitle: Text(id),
              trailing: IconButton(
                icon: const Icon(Icons.close),
                onPressed: busy ? null : () => setState(() => savedHosts.remove(id)),
              ),
            ),
        ],
      ),
    );
  }

  Future<void> clear() async {
    if (!current || busy || revision == null) return;
    final yes = await showGlassDialog<bool>(
      context,
      builder: (decisionContext) => ownDecision(decisionContext, AutomationScopeGuard(
        scope: originalScope,
        builder: (context) => AlertDialog(
          title: Text(context.l10n.automationInventoryClear),
          content: Text(context.l10n.automationInventoryClearHint),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context, false), child: Text(context.l10n.commonCancel)),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: Text(context.l10n.automationInventoryClear),
            ),
          ],
        ),
      )),
    );
    if (yes != true || !mounted || !current) return;
    setState(() => busy = true);
    try {
      final updated = await originalService.clearInventory(originalProject, revision!);
      if (mounted && current) Navigator.pop(context, updated);
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Widget hostRow(AutomationJson row, List<Host> hosts) {
    final l = context.l10n, alias = row['alias'] as String;
    final binding = bindings[alias], hostId = binding?['host_id'] as String?;
    final missing = hostId != null && !hosts.any((host) => host.id.value == hostId);
    final changed = binding != null && stale(binding, row);
    void bind(String? id) => setState(() {
      if (id == null) {
        bindings.remove(alias);
      } else {
        bindings[alias] = {'host_id': id, 'resolution': 'match', 'source': endpoint(row)};
      }
    });
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(alias, style: Theme.of(context).textTheme.titleSmall),
          SelectableText /* l10n-ignore: SSH endpoint data */ (
            '${row['username'] ?? '—'}@${row['address']}:${row['port']}',
          ), // l10n-ignore: SSH endpoint identity data
          Text((row['sources'] as List).join(', '), style: Theme.of(context).textTheme.bodySmall),
          TextButton.icon(
            key: ValueKey('automation-inventory-binding-$alias'),
            icon: const Icon(Icons.link),
            label: Text(
              missing
                  ? l.automationInventoryMissingHost
                  : hosts.where((h) => h.id.value == hostId).map((h) => '${h.name} · ${h.address}').firstOrNull ??
                        l.automationInventoryBind,
            ),
            onPressed: busy
                ? null
                : () async {
                    final choice = await showGlassDialog<_HostChoice>(
                      context,
                      builder: (decisionContext) => ownDecision(decisionContext, AutomationScopeGuard(
                        scope: originalScope,
                        builder: (_) => _HostPicker(
                          hosts: hosts
                              .where(
                                (host) =>
                                    host.id.value == hostId ||
                                    !bindings.values.any((b) => b['host_id'] == host.id.value),
                              )
                              .toList(),
                          hasBinding: hostId != null,
                        ),
                      )),
                    );
                    if (current && choice != null) bind(choice.id);
                  },
          ),
          if (changed)
            TextButton(
              key: ValueKey('automation-inventory-rebind-$alias'),
              onPressed: busy ? null : () => bind(hostId),
              child: Text(l.automationInventoryRebind),
            ),
          if (hostId != null)
            CheckboxListTile(
              dense: true,
              contentPadding: EdgeInsets.zero,
              value: binding?['resolution'] == 'use_saved_host',
              title: Text(l.automationInventoryUseSaved),
              onChanged: busy || changed || missing
                  ? null
                  : (value) => setState(
                      () => bindings[alias] = {...binding!, 'resolution': value == true ? 'use_saved_host' : 'match'},
                    ),
            ),
          const Divider(),
        ],
      ),
    );
  }

  Widget conflictRow(AutomationJson value) {
    final l = context.l10n, alias = value['alias'] as String, kind = value['kind'] as String;
    final label = switch (kind) {
      'source_missing' => l.automationInventorySourceMissing,
      'source_changed' => l.automationInventoryRebind,
      'saved_host_missing' => l.automationInventoryMissingHost,
      'saved_route_invalid' => l.automationInventoryRouteInvalid,
      'endpoint_mismatch' => l.automationInventoryEndpointMismatch,
      _ => l.automationInventoryFailed,
    };
    final saved = value['saved'] as AutomationJson?;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('$alias: $label'),
        if (saved != null)
          SelectableText /* l10n-ignore: localized route label and endpoint data */ (
            // l10n-ignore: localized saved route label plus SSH endpoint data
            '${l.automationInventorySavedRoute}: ${saved['username']}@${saved['address']}:${saved['port']}',
          ),
        Wrap(
          children: [
            if (kind != 'source_missing')
              TextButton(
                key: ValueKey('automation-inventory-conflict-open-$alias'),
                onPressed: busy
                    ? null
                    : () {
                        search.text = alias;
                        queryChanged(alias);
                      },
                child: Text(l.automationInventoryShowEntry),
              ),
            TextButton(
              key: ValueKey('automation-inventory-conflict-remove-$alias'),
              onPressed: busy
                  ? null
                  : () => setState(() {
                      bindings.remove(alias);
                      conflicts.removeWhere((c) => c['alias'] == alias);
                      conflictCount--;
                    }),
              child: Text(l.automationInventoryUnbound),
            ),
          ],
        ),
      ],
    );
  }

  Widget resultPane(List<Host> available, {required bool hostsLoading, required Object? hostError}) {
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (busy || fetching || hostsLoading) const LinearProgressIndicator(),
        if (error != null)
          Text(
            error is FormatException ? (error as FormatException).message : errorMessage(l, error!),
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        if (hostError != null) Text(errorMessage(l, hostError)),
        if (!unified)
          GlassSelect<String>(
            key: const ValueKey('automation-inventory-mode'),
            value: mode,
            expand: true,
            semanticLabel: l.automationInventoryMode,
            onChanged: busy ? null : (value) => setState(() => mode = value),
            items: [
              GlassSelectItem(value: 'external_only', label: l.automationInventoryExternalOnly),
              GlassSelectItem(value: 'mixed', label: l.automationInventoryMixed),
            ],
          ),
        if (unified)
          Text(
            sourceKind == 'mixed'
                ? '${l.automationInventoryMixed} · ${l.automationTargets}: ${savedHosts.length}'
                : l.automationInventoryExternalOnly,
          ),
        Text('${l.automationInventoryBound}: ${bindings.length}'),
        if (resolvedSources.isNotEmpty)
          TextButton(
            key: const ValueKey('automation-inventory-resolved-files'),
            onPressed: () => showGlassDialog<void>(
              context,
              builder: (_) => AutomationScopeGuard(
                scope: originalScope,
                builder: (context) => AlertDialog(
                  title: Text(l.automationInventoryResolved),
                  content: SizedBox(
                    width: 600,
                    height: MediaQuery.sizeOf(context).height * .5,
                    child: ListView.builder(
                      itemCount: resolvedSources.length,
                      itemBuilder: (context, index) => ListTile(
                        title: Text(resolvedSources[index]['path'] as String),
                        subtitle: Text(resolvedSources[index]['format'] as String),
                      ),
                    ),
                  ),
                  actions: [TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonClose))],
                ),
              ),
            ),
            child: Text('${l.automationInventoryResolved}: ${resolvedSources.length}'),
          ),
        TextField(
          key: const ValueKey('automation-inventory-search'),
          controller: search,
          onChanged: queryChanged,
          decoration: InputDecoration(labelText: l.automationInventorySearch),
          autocorrect: false,
        ),
        Wrap(
          spacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            Text('$offset–$cursor / $total'),
            TextButton(
              key: const ValueKey('automation-inventory-previous'),
              onPressed: busy || fetching || offset == 0 ? null : () => go((offset - 50).clamp(0, 10000)),
              child: Text(l.automationInventoryPrevious),
            ),
            TextButton(
              key: const ValueKey('automation-inventory-next'),
              onPressed: busy || fetching || cursor >= total ? null : () => go(cursor),
              child: Text(l.automationInventoryNext),
            ),
          ],
        ),
        if (conflictCount > 0) ...[
          Text('${l.automationInventoryConflicts}: $conflictCount'),
          Flexible(
            child: ListView(
              children: [
                Text(l.automationInventoryConflictsHint),
                for (final conflict in conflicts) conflictRow(conflict),
              ],
            ),
          ),
        ],
        Expanded(
          flex: 3,
          child: ListView.builder(
            key: const ValueKey('automation-inventory-rows'),
            itemCount: rows.length,
            itemBuilder: (context, index) => hostRow(rows[index], available),
          ),
        ),
      ],
    );
  }

  Widget filePicker(AsyncValue<AutomationRunnerCatalog?> runners, List<String> missing) {
    final l = context.l10n;
    final groups = yamlPreview?.entries.toList(growable: false);
    return ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (groups != null) ...[
            Text(l.automationInventoryYamlPreviewHelp),
            if (groups.isEmpty) Text(l.automationInventoryYamlPreviewEmpty),
            if (groups.isNotEmpty)
              ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 200),
                child: ListView.builder(
                  shrinkWrap: true,
                  primary: false,
                  itemCount: groups.length,
                  itemBuilder: (context, index) {
                    final group = groups[index];
                    return ExpansionTile(
                      dense: true,
                      initiallyExpanded: groups.length == 1,
                      title: Text(group.key),
                      subtitle: Text(group.value.length.toString()),
                      children: [for (final host in group.value) ListTile(title: Text(host), dense: true)],
                    );
                  },
                ),
              ),
            const SizedBox(height: 12),
          ],
          GlassSelect<String?>(
            key: const ValueKey('automation-inventory-runner'),
            value: runner,
            expand: true,
            semanticLabel: l.automationChooseRunner,
            onChanged: busy || loading ? null : (value) => setState(() => runner = value),
            items: [
              GlassSelectItem(value: null, label: l.automationChooseRunner),
              for (final item in runners.value?.runners ?? <AutomationRunnerProfile>[])
                if (!item.readOnly) GlassSelectItem(value: item.id, label: item.name ?? item.id),
            ],
          ),
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text(l.automationInventoryRunnerHelp),
          ),
          if (runners.hasError) Text(errorMessage(l, runners.error!)),
          const SizedBox(height: 12),
          Text(l.automationInventoryFiles),
          const SizedBox(height: 8),
          SizedBox(
            height: 180,
            child: ClipRect(
              child: ListView.builder(
                key: const ValueKey('automation-inventory-files'),
                primary: false,
                itemCount: paths.length,
                itemBuilder: (context, index) {
                  final path = paths[index];
                  return CheckboxListTile(
                    key: ValueKey('automation-inventory-file-$path'),
                    dense: true,
                    value: selected.contains(path),
                    title: Text(directories.contains(path) ? '$path/' : path),
                    onChanged: loading || busy || (!selected.contains(path) && selected.length >= 8)
                        ? null
                        : (value) => setState(() {
                            value == true ? selected.add(path) : selected.remove(path);
                            yamlPreview = null;
                            error = null;
                          }),
                  );
                },
              ),
            ),
          ),
          for (final path in missing)
            ListTile(
              title: Text(path),
              subtitle: Text(l.automationInventoryMissingFile),
              trailing: IconButton(
                icon: const Icon(Icons.close),
                onPressed: busy ? null : () => setState(() {
                  selected.remove(path);
                  yamlPreview = null;
                }),
              ),
            ),
          const SizedBox(height: 12),
          Text(l.automationInventoryDraftHint),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (retired) return const SizedBox.shrink();
    final l = context.l10n, runners = ref.watch(automationRunnersProvider), hosts = ref.watch(hostsProvider);
    final available = [
      for (final host in hosts.value ?? <Host>[])
        if (host.protocol == HostProtocol.ssh) host,
    ];
    final missing = selected.where((path) => !paths.contains(path)).toList();
    final folders = unified ? ref.watch(groupsProvider) : const AsyncData<List<Group>>([]);
    return AlertDialog(
      title: Text(l.automationInventorySources),
      content: SizedBox(
        width: 960,
        height: MediaQuery.sizeOf(context).height * .65,
        child: ready
            ? resultPane(available, hostsLoading: hosts.isLoading, hostError: hosts.error)
            : ListView(
                key: const ValueKey('automation-inventory-source-scroll'),
                children: [
                  Text(unified ? l.automationInventorySourcesHint : l.automationInventoryHint),
                  const SizedBox(height: 12),
                  if (loading || busy || (previewId != null && !ready && error == null))
                    const LinearProgressIndicator(),
                  if (error != null)
                    Text(
                      error is FormatException ? (error as FormatException).message : errorMessage(l, error!),
                      style: TextStyle(color: Theme.of(context).colorScheme.error),
                    ),
                  if (conflictCount > 0) ...[
                    Text('${l.automationInventoryConflicts}: $conflictCount'),
                    Text(l.automationInventoryConflictsHint),
                    for (final conflict in conflicts) conflictRow(conflict),
                  ],
                  if (previewId == null) ...[
                    if (unified)
                      GlassSelect<String>(
                        key: const ValueKey('automation-inventory-source-kind'),
                        value: sourceKind,
                        expand: true,
                        items: [
                          GlassSelectItem(value: 'saved', label: l.automationInventorySavedHosts),
                          GlassSelectItem(value: 'files', label: l.automationInventoryFilesOnly),
                          GlassSelectItem(value: 'mixed', label: l.automationInventoryMixed),
                        ],
                        onChanged: busy || loading ? null : (value) => setState(() => sourceKind = value),
                      ),
                    const SizedBox(height: 16),
                    LayoutBuilder(
                      builder: (context, constraints) {
                        final saved = unified && sourceKind != 'files';
                        final files = !unified || sourceKind != 'saved';
                        final hostPicker = KeyedSubtree(
                          key: const ValueKey('automation-inventory-host-panel'),
                          child: savedPicker(available, folders),
                        );
                        final filesPicker = KeyedSubtree(
                          key: const ValueKey('automation-inventory-file-panel'),
                          child: filePicker(runners, missing),
                        );
                        if (saved && files && constraints.maxWidth >= 760) {
                          return Row(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Expanded(child: hostPicker),
                              const SizedBox(width: 16),
                              Expanded(child: filesPicker),
                            ],
                          );
                        }
                        return Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            if (saved) hostPicker,
                            if (saved && files) const SizedBox(height: 16),
                            if (files) filesPicker,
                          ],
                        );
                      },
                    ),
                  ],
                ],
              ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonCancel)),
        if (configured && previewId == null)
          TextButton(
            key: const ValueKey('automation-inventory-clear'),
            onPressed: busy || loading ? null : clear,
            child: Text(l.automationInventoryClear),
          ),
        if (previewId != null)
          TextButton(
            key: const ValueKey('automation-inventory-back'),
            onPressed: busy ? null : back,
            child: Text(l.automationInventoryBack),
          ),
        if (unified && sourceKind == 'saved')
          FilledButton(
            key: const ValueKey('automation-inventory-save-saved'),
            onPressed: busy || loading || hosts.isLoading || hosts.hasError ? null : saveSaved,
            child: Text(l.commonSave),
          ),
        if (previewId == null && (!unified || sourceKind != 'saved'))
          FilledButton(
            key: const ValueKey('automation-inventory-start'),
              onPressed: busy || loading || selected.isEmpty || missing.isNotEmpty
                  ? null
                  : runner != null
                  ? start
                  : canPreviewYaml
                  ? previewYaml
                  : null,
            child: Text(l.automationInventoryPreview),
          ),
        if (ready)
          FilledButton(
            key: const ValueKey('automation-inventory-save'),
            onPressed: busy || fetching || hosts.isLoading || hosts.hasError ? null : save,
            child: Text(l.commonSave),
          ),
      ],
    );
  }
}

class _HostChoice {
  const _HostChoice(this.id);
  final String? id;
}

class _HostPicker extends StatefulWidget {
  const _HostPicker({required this.hosts, required this.hasBinding});
  final List<Host> hosts;
  final bool hasBinding;
  @override
  State<_HostPicker> createState() => _HostPickerState();
}

class _HostPickerState extends State<_HostPicker> {
  String query = '';
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final hosts = widget.hosts
        .where((h) => h.name.toLowerCase().contains(query) || h.address.toLowerCase().contains(query))
        .toList();
    return AlertDialog(
      title: Text(l.automationInventoryBind),
      content: SizedBox(
        width: 560,
        height: MediaQuery.sizeOf(context).height * .55,
        child: Column(
          children: [
            TextField(
              key: const ValueKey('automation-inventory-host-search'),
              autofocus: true,
              decoration: InputDecoration(labelText: l.automationInventorySavedSearch),
              onChanged: (value) => setState(() => query = value.trim().toLowerCase()),
            ),
            Expanded(
              child: ListView.builder(
                itemCount: hosts.length,
                itemBuilder: (context, index) {
                  final host = hosts[index];
                  return ListTile(
                    key: ValueKey('automation-inventory-choose-${host.id}'),
                    title: Text(host.name),
                    subtitle: Text(host.address),
                    onTap: () => Navigator.pop(context, _HostChoice(host.id.value)),
                  );
                },
              ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonCancel)),
        if (widget.hasBinding)
          TextButton(
            key: const ValueKey('automation-inventory-unbind'),
            onPressed: () => Navigator.pop(context, const _HostChoice(null)),
            child: Text(l.automationInventoryUnbound),
          ),
      ],
    );
  }
}
