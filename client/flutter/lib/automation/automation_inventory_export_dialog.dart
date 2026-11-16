import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:consolecrypt/automation/automation_inventory_export_save.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class AutomationInventoryExportDialog extends ConsumerStatefulWidget {
  const AutomationInventoryExportDialog({
    super.key,
    required this.scope,
    required this.project,
    required this.service,
    required this.isProjectCurrent,
  });
  final AutomationScope scope;
  final AutomationProject project;
  final AutomationInventoryExportService service;

  /// Parent compares the selected project, revision and workspace generation.
  final bool Function() isProjectCurrent;
  @override
  ConsumerState<AutomationInventoryExportDialog> createState() => _InventoryExportDialogState();
}

class _InventoryExportDialogState extends ConsumerState<AutomationInventoryExportDialog> {
  final selectedHosts = <String>{}, selectedGroups = <String>{};
  final search = TextEditingController();
  InventoryExportContext? inventory;
  Object? failure;
  bool loading = true, busy = false;
  int epoch = 0;
  Timer? contextTimer;
  bool get current =>
      mounted &&
      widget.scope.unlocked &&
      identical(ref.read(automationScopeProvider), widget.scope) &&
      widget.isProjectCurrent();
  bool requestCurrent(int token) => current && epoch == token;

  @override
  void initState() {
    super.initState();
    contextTimer = Timer.periodic(const Duration(milliseconds: 250), (_) {
      if (current || !mounted) return;
      epoch++;
      final route = ModalRoute.of(context);
      if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
    });
    load();
  }

  @override
  void dispose() {
    epoch++;
    contextTimer?.cancel();
    search.dispose();
    selectedHosts.clear();
    selectedGroups.clear();
    inventory = null;
    super.dispose();
  }

  Future<void> load() async {
    if (!current || busy) return;
    final token = ++epoch;
    setState(() {
      loading = true;
      failure = null;
      inventory = null;
      selectedHosts.clear();
      selectedGroups.clear();
    });
    try {
      final context = await widget.service.inventoryExportContext(widget.project.id);
      if (!requestCurrent(token)) return;
      if (context.revision != widget.project.revision) {
        throw const InventoryExportFailure(InventoryExportReason.contextChanged);
      }
      setState(() => inventory = context);
    } catch (e) {
      if (requestCurrent(token)) setState(() => failure = e);
    } finally {
      if (requestCurrent(token)) setState(() => loading = false);
    }
  }

  List<Host> allowedHosts(List<Host> saved) => [
    for (final host in saved)
      if (host.protocol == HostProtocol.ssh && inventory?.permits(host.id.value) == true) host,
  ];

  Future<void> save() async {
    final context = inventory;
    final saved = ref.read(hostsProvider);
    if (!current ||
        busy ||
        loading ||
        context == null ||
        context.externalOnly ||
        saved.isLoading ||
        saved.hasError ||
        selectedHosts.isEmpty ||
        selectedHosts.length > 32) {
      return;
    }
    final available = allowedHosts(saved.value ?? []).map((host) => host.id.value).toSet();
    if (!available.containsAll(selectedHosts)) return;
    final request = InventoryExportRequest(
      projectId: widget.project.id,
      expectedRevision: context.revision,
      hostIds: selectedHosts.toList()..sort(),
      groupNames: selectedGroups.toList()..sort(),
    );
    if (!context.allows(request)) return;
    final token = epoch;
    final files = ref.read(fileDialogServiceProvider);
    final writer = ref.read(inventoryExportFileWriterProvider);
    setState(() {
      busy = true;
      failure = null;
    });
    String? destination;
    try {
      destination = await files.chooseSaveFile(
        suggestedName: 'inventory.consolecrypt.json',
        extensions: const ['json'],
      );
      if (destination == null || !requestCurrent(token)) return;
      // Issue after destination approval: never reuse a cached export receipt.
      final issued = await widget.service.issueInventoryExport(request);
      if (!requestCurrent(token)) return;
      await writer.write(destination, issued.bytes);
      if (!requestCurrent(token)) return;
      final saved = await files.finishSaveFile(destination);
      if (!requestCurrent(token) || !saved) return;
      if (mounted) {
        showSnack(this.context, this.context.l10n.automationInventoryExportSaved);
        Navigator.of(this.context).pop(true);
      }
    } catch (e) {
      if (requestCurrent(token)) setState(() => failure = e);
    } finally {
      if (destination != null && AppPlatform.isMobile) {
        await writer.discardMobileStaging(destination);
      }
      if (current) setState(() => busy = false);
    }
  }

  String message(AppLocalizations l, Object error) {
    if (error is! InventoryExportFailure) return errorMessage(l, error);
    return switch (error.reason) {
      InventoryExportReason.contextChanged => l.automationInventoryExportChanged,
      InventoryExportReason.hostNotSelected ||
      InventoryExportReason.invalidRequest => l.automationInventoryExportSelectionInvalid,
      InventoryExportReason.unknownGroup ||
      InventoryExportReason.invalidGroups ||
      InventoryExportReason.aliasConflict => l.automationInventoryExportGroupsInvalid,
      InventoryExportReason.unsupportedRoute => l.automationInventoryExportRouteUnsupported,
      InventoryExportReason.unsupportedEndpoint => l.automationInventoryExportEndpointUnsupported,
      InventoryExportReason.size => l.automationInventoryExportTooLarge,
      InventoryExportReason.invalidResponse || InventoryExportReason.failed => l.automationInventoryExportFailed,
    };
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final saved = ref.watch(hostsProvider);
    ref.listen(hostsProvider, (previous, next) {
      if (identical(previous?.value, next.value) || !current) return;
      setState(() {
        if (busy || !loading) epoch++;
        final allowed = allowedHosts(next.value ?? []).map((host) => host.id.value).toSet();
        selectedHosts.removeWhere((id) => !allowed.contains(id));
        if (busy) {
          failure = const InventoryExportFailure(InventoryExportReason.contextChanged);
        }
      });
    });
    final contextValue = inventory;
    final hosts = allowedHosts(saved.value ?? []);
    final listHeight = math.min(
      360.0,
      math.max(220.0, math.max(hosts.length * 58.0 + 128, (contextValue?.groups.length ?? 0) * 42.0 + 100)),
    );
    final query = search.text.trim().toLowerCase();
    final visible = hosts.where(
      (host) => host.name.toLowerCase().contains(query) || host.address.toLowerCase().contains(query),
    );
    final enabled =
        current &&
        !loading &&
        !busy &&
        contextValue != null &&
        !contextValue.externalOnly &&
        !saved.isLoading &&
        !saved.hasError;
    Widget hostList() => ContentSurface(
      key: const ValueKey('inventory-export-host-list'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.automationInventoryExportHosts, style: GlassTokens.of(context).typography.bodyEmph),
          Text(l.automationInventoryExportHostsHint),
          TextField(
            controller: search,
            enabled: !busy,
            decoration: InputDecoration(labelText: l.commonSearch),
            onChanged: (_) => setState(() {}),
          ),
          const SizedBox(height: 8),
          Expanded(
            child: ClipRect(
              child: ListView(
                children: [
                  if (hosts.isEmpty) Text(l.automationInventoryExportNoHosts),
                  for (final host in visible)
                    CheckboxListTile(
                      key: ValueKey('inventory-export-host-${host.id.value}'),
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      title: Text(host.name),
                      subtitle: Text(host.address),
                      value: selectedHosts.contains(host.id.value),
                      onChanged: !enabled || selectedHosts.length >= 32 && !selectedHosts.contains(host.id.value)
                          ? null
                          : (value) => setState(() {
                              epoch++;
                              failure = null;
                              value == true ? selectedHosts.add(host.id.value) : selectedHosts.remove(host.id.value);
                            }),
                    ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
    Widget groupList() => ContentSurface(
      key: const ValueKey('inventory-export-group-list'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.automationInventoryExportGroups, style: GlassTokens.of(context).typography.bodyEmph),
          Text(l.automationInventoryExportGroupsHint),
          const SizedBox(height: 8),
          Expanded(
            child: ClipRect(
              child: ListView(
                children: [
                  if (contextValue?.groups.isEmpty ?? true) Text(l.automationInventoryExportNoGroups),
                  for (final group in contextValue?.groups ?? <String>[])
                    CheckboxListTile(
                      key: ValueKey('inventory-export-group-$group'),
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      title: Text(group),
                      value: selectedGroups.contains(group),
                      onChanged: enabled
                          ? (value) => setState(() {
                              epoch++;
                              failure = null;
                              value == true ? selectedGroups.add(group) : selectedGroups.remove(group);
                            })
                          : null,
                    ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
    return AutomationScopeGuard(
      scope: widget.scope,
      builder: (context) => GlassDialog(
        key: const ValueKey('automation-inventory-export-dialog'),
        width: 920,
        title: l.automationInventoryExportTitle,
        icon: Icons.file_download_outlined,
        content: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: math.min(650, MediaQuery.sizeOf(context).height * .68)),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(widget.project.name ?? widget.project.id),
                Text(l.automationInventoryExportDisclosure),
                const SizedBox(height: 8),
                Text(l.automationInventoryExportStaticHint),
                const SizedBox(height: 8),
                Text(l.automationInventoryExportSelected(selectedHosts.length, selectedGroups.length)),
                if (loading || busy || saved.isLoading) const LinearProgressIndicator(),
                if (failure != null) Text(message(l, failure!), key: const ValueKey('inventory-export-error')),
                if (saved.hasError) Text(errorMessage(l, saved.error!)),
                if (contextValue?.externalOnly == true) Text(l.automationInventoryExportExternalOnly),
                const SizedBox(height: 12),
                LayoutBuilder(
                  builder: (_, size) => size.maxWidth >= 700
                      ? SizedBox(
                          height: listHeight,
                          child: Row(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              Expanded(child: hostList()),
                              const SizedBox(width: 12),
                              Expanded(child: groupList()),
                            ],
                          ),
                        )
                      : Column(
                          children: [
                            SizedBox(height: math.min(280, listHeight), child: hostList()),
                            const SizedBox(height: 12),
                            SizedBox(height: 200, child: groupList()),
                          ],
                        ),
                ),
              ],
            ),
          ),
        ),
        primaryAction: GlassButton.prominent(
          key: const ValueKey('inventory-export-save'),
          label: l.automationInventoryExportSave,
          onPressed: enabled && selectedHosts.isNotEmpty ? save : null,
        ),
        secondaryActions: [
          GlassButton(
            key: const ValueKey('inventory-export-reload'),
            label: l.commonRetry,
            onPressed: busy || loading ? null : load,
          ),
          GlassButton(label: l.commonCancel, onPressed: () => Navigator.of(context).pop()),
        ],
      ),
    );
  }
}
