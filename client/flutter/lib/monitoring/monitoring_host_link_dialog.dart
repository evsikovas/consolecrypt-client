import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:material_ui/material_ui.dart';

/// Links a saved host to a target explicitly selected from an existing metrics
/// source. This never installs software, infers ownership or accepts PromQL.
class MonitoringHostLinkDialog extends StatefulWidget {
  const MonitoringHostLinkDialog({
    required this.host,
    required this.service,
    required this.store,
    required this.isCurrent,
    required this.onLinked,
    required this.onConfigureSource,
    super.key,
  });
  final MonitoringHostRef host;
  final MonitoringService service;
  final MonitoringSourceStore store;
  final bool Function() isCurrent;
  final ValueChanged<MonitoringBinding> onLinked;
  final VoidCallback onConfigureSource;
  @override
  State<MonitoringHostLinkDialog> createState() => _MonitoringHostLinkDialogState();
}

class _MonitoringHostLinkDialogState extends State<MonitoringHostLinkDialog> {
  List<MonitoringSource> sources = [];
  List<Map<String, String>> targets = [];
  String? sourceId, selectedTarget;
  bool loading = true, busy = false, searched = false, partial = false;
  int generation = 0;
  Object? failure;
  MonitoringSource? get source => sources.where((s) => s.id == sourceId).firstOrNull;
  bool current(int token) => mounted && token == generation && widget.isCurrent();
  @override
  void initState() {
    super.initState();
    unawaited(load());
  }

  Future<void> load() async {
    final token = ++generation;
    try {
      final loaded = await widget.store.load();
      if (!current(token)) return;
      setState(() {
        sources = loaded.where((s) => !s.readOnly && s.allowsBackgroundObservation).toList();
        sourceId = sources.firstOrNull?.id;
        loading = false;
      });
    } catch (error) {
      if (current(token)) {
        setState(() {
          failure = error;
          loading = false;
        });
      }
    }
  }

  Future<void> findTargets() async {
    final original = source;
    if (busy || original == null || !widget.isCurrent()) return;
    final token = ++generation;
    setState(() {
      busy = true;
      failure = null;
      targets = [];
      selectedTarget = null;
      searched = false;
      partial = false;
    });
    try {
      final result = await widget.service.query(original, const MonitoringQuery(expression: 'up'));
      if (!current(token)) return;
      if (result.error != null) throw result.error!;
      if (result.status != 'success' || result.data?.kind != 'vector') {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
      }
      final unique = <String, Map<String, String>>{};
      for (final series in result.data!.series.take(1000)) {
        final selector = Map<String, String>.fromEntries(series.labels.entries.where((e) => e.key != '__name__'));
        if ((selector['instance']?.isEmpty ?? true) || series.samples.isEmpty) continue;
        final keys = selector.keys.toList()..sort();
        final stable = {for (final key in keys) key: selector[key]!};
        // Validate the exact labels before offering a target to the user.
        MonitoringBinding(
          id: '',
          name: widget.host.name,
          hostId: widget.host.id,
          sourceId: original.id,
          revision: 0,
          sourceRevision: original.revision,
          selector: stable,
          state: MonitoringBindingState.current,
        ).validate(create: true);
        unique[jsonEncode(stable)] = stable;
      }
      setState(() {
        targets = unique.values.toList();
        searched = true;
        partial = result.partial;
      });
    } catch (error) {
      if (current(token)) setState(() => failure = error);
    } finally {
      if (current(token)) setState(() => busy = false);
    }
  }

  Future<void> save() async {
    final original = source, bindings = widget.store;
    final labels = targets.where((t) => jsonEncode(t) == selectedTarget).firstOrNull;
    if (busy ||
        original == null ||
        labels == null ||
        bindings is! MonitoringBindingStore ||
        bindings.readOnly ||
        !widget.isCurrent()) {
      return;
    }
    final token = ++generation;
    setState(() {
      busy = true;
      failure = null;
    });
    try {
      final catalog = await bindings.load();
      if (!current(token)) return;
      if (!catalog.any((s) => s.id == original.id && s.fingerprint == original.fingerprint && !s.readOnly)) {
        throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
      }
      final existing = (bindings as MonitoringBindingStore).bindings
          .where(
            (b) =>
                b.hostId == widget.host.id &&
                b.sourceId == original.id &&
                b.sourceRevision == original.revision &&
                b.state == MonitoringBindingState.current &&
                jsonEncode(Map.fromEntries((b.selector.entries.toList()..sort((a, b) => a.key.compareTo(b.key))))) ==
                    jsonEncode(labels),
          )
          .firstOrNull;
      if (existing != null) {
        widget.onLinked(existing);
        return;
      }
      final saved = await (bindings as MonitoringBindingStore).saveBinding(
        MonitoringBinding(
          id: '',
          name: widget.host.name,
          hostId: widget.host.id,
          sourceId: original.id,
          revision: 0,
          sourceRevision: original.revision,
          selector: labels,
          state: MonitoringBindingState.current,
        ),
        create: true,
      );
      if (current(token)) widget.onLinked(saved);
    } catch (error) {
      if (current(token)) setState(() => failure = error);
    } finally {
      if (current(token)) setState(() => busy = false);
    }
  }

  @override
  void dispose() {
    generation++;
    widget.service.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return AlertDialog(
      key: const ValueKey('monitoring-host-link-dialog'),
      title: Text('${l.monitoringChooseTarget} · ${widget.host.name}'),
      content: SizedBox(
        width: 580,
        height: (MediaQuery.sizeOf(context).height - 240).clamp(160.0, 240.0),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.monitoringTargetHint),
              const SizedBox(height: 12),
              if (loading || busy) const LinearProgressIndicator(),
              if (failure != null) Text(errorMessage(l, failure!), key: const ValueKey('monitoring-host-link-error')),
              if (!loading && sources.isEmpty)
                TextButton(onPressed: widget.onConfigureSource, child: Text(l.monitoringConnectSource)),
              if (sources.isNotEmpty) ...[
                DropdownButtonFormField<String>(
                  key: const ValueKey('monitoring-host-link-source'),
                  initialValue: sourceId,
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.monitoringSources),
                  items: [for (final value in sources) DropdownMenuItem(value: value.id, child: Text(value.name))],
                  onChanged: busy
                      ? null
                      : (value) => setState(() {
                          generation++;
                          sourceId = value;
                          targets = [];
                          selectedTarget = null;
                          searched = false;
                          partial = false;
                          failure = null;
                        }),
                ),
                const SizedBox(height: 8),
                OutlinedButton.icon(
                  key: const ValueKey('monitoring-host-link-find'),
                  onPressed: busy ? null : findTargets,
                  icon: const Icon(Icons.search),
                  label: Text(l.monitoringFindTargets),
                ),
              ],
              if (searched && targets.isEmpty) Text(l.monitoringNoTargets),
              if (partial) Text(l.monitoringPartialResult),
              if (targets.isNotEmpty) ...[
                const SizedBox(height: 12),
                DropdownButtonFormField<String>(
                  key: const ValueKey('monitoring-host-link-target'),
                  initialValue: selectedTarget,
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.monitoringTargetLabel),
                  items: [
                    for (final target in targets)
                      DropdownMenuItem(
                        value: jsonEncode(target),
                        child: Text(
                          target.entries.map((e) => '${e.key}: ${e.value}').join(' · '),
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                  ],
                  onChanged: busy ? null : (value) => setState(() => selectedTarget = value),
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l.commonClose)),
        FilledButton(
          key: const ValueKey('monitoring-host-link-save'),
          onPressed: busy || selectedTarget == null || widget.store.readOnly || widget.store is! MonitoringBindingStore
              ? null
              : save,
          child: Text(l.monitoringLinkAndOpen),
        ),
      ],
    );
  }
}
