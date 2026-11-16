import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_exporter_review.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:material_ui/material_ui.dart';

/// Explicit post-job observation. No installer, poller, guessed host or query.
class MonitoringExporterDialog extends StatefulWidget {
  const MonitoringExporterDialog({
    super.key,
    required this.profileId,
    required this.source,
    required this.reader,
    required this.store,
    required this.jobs,
    required this.hosts,
    required this.isCurrent,
    required this.onSaved,
  });
  final String profileId;
  final MonitoringSource source;
  final MonitoringExporterReader reader;
  final MonitoringExporterBindingStore store;
  final AutomationJobService jobs;
  final List<MonitoringHostRef> hosts;
  final bool Function() isCurrent;
  final ValueChanged<MonitoringBinding> onSaved;
  @override
  State<MonitoringExporterDialog> createState() => _MonitoringExporterDialogState();
}

class _MonitoringExporterDialogState extends State<MonitoringExporterDialog> {
  final name = TextEditingController(), jobLabel = TextEditingController(), instance = TextEditingController();
  final clock = Stopwatch()..start();
  List<({String id, int revision})> jobs = const [];
  String? hostId, jobId;
  MonitoringExporterKind kind = MonitoringExporterKind.nodeExporter;
  MonitoringExporterContext? original;
  MonitoringExporterObservation? observation;
  int generation = 0, acceptedAt = 0;
  bool busy = false, failed = false;
  bool get current => mounted && widget.isCurrent();
  bool live(int token) => current && token == generation;
  Map<String, String> get selector => {'job': jobLabel.text, 'instance': instance.text};
  bool get fresh {
    final value = observation;
    if (value == null || !value.observed) return false;
    final elapsed = (clock.elapsedMicroseconds - acceptedAt) / 1000000;
    return elapsed >= 0 && elapsed <= 30 && value.maxSampleAgeSeconds! + elapsed <= 120;
  }

  @override
  void initState() {
    super.initState();
    for (final field in [name, jobLabel, instance]) {
      field.addListener(edited);
    }
  }

  @override
  void didUpdateWidget(MonitoringExporterDialog old) {
    super.didUpdateWidget(old);
    if (old.profileId != widget.profileId ||
        old.source.fingerprint != widget.source.fingerprint ||
        !identical(old.reader, widget.reader) ||
        !identical(old.store, widget.store) ||
        !identical(old.jobs, widget.jobs)) {
      old.reader.cancelExporter();
      generation++;
      original = null;
      observation = null;
      busy = false;
      failed = false;
    }
  }

  void invalidate() {
    generation++;
    widget.reader.cancelExporter();
    original = null;
    observation = null;
    busy = false;
    failed = false;
  }

  void edited() {
    if (!mounted) return;
    setState(invalidate);
  }

  @override
  void dispose() {
    generation++;
    widget.reader.cancelExporter();
    for (final field in [name, jobLabel, instance]) {
      field.dispose();
    }
    clock.stop();
    super.dispose();
  }

  Future<void> loadJobs() async {
    if (!current || busy || hostId == null) return;
    final token = ++generation;
    setState(() {
      busy = true;
      failed = false;
      jobs = const [];
      jobId = null;
      original = null;
      observation = null;
    });
    try {
      final catalog = await widget.jobs.jobs();
      if (!live(token)) return;
      final raw = catalog['jobs'];
      if (raw is! List || raw.length > 2048) throw const FormatException('jobs');
      final seen = <String>{};
      final selected = <({String id, int revision})>[];
      final uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
      for (final value in raw) {
        if (value is! Map) throw const FormatException('jobs');
        if (value['state'] != 'succeeded') continue;
        final id = value['job_id'], revision = value['notification_revision'], hosts = value['host_ids'];
        if (id is! String ||
            !uuid.hasMatch(id) ||
            !seen.add(id) ||
            revision is! int ||
            revision < 1 ||
            revision > 9007199254740991 ||
            hosts is! List ||
            hosts.length > 256 ||
            !hosts.every((h) => h is String && uuid.hasMatch(h))) {
          throw const FormatException('jobs');
        }
        if (hosts.contains(hostId)) selected.add((id: id, revision: revision));
      }
      if (live(token)) setState(() => jobs = List.unmodifiable(selected));
    } catch (_) {
      if (live(token)) setState(() => failed = true);
    } finally {
      if (live(token)) setState(() => busy = false);
    }
  }

  Future<void> verify({bool save = false}) async {
    if (!current || busy || hostId == null || jobId == null) return;
    final selected = jobs.where((j) => j.id == jobId).firstOrNull;
    if (selected == null) return;
    final token = ++generation;
    setState(() {
      busy = true;
      failed = false;
      observation = null;
    });
    try {
      validateExporterSelector(selector, kind);
      final captured =
          original ??
          await widget.reader.exporterContext(
            widget.source,
            profileId: widget.profileId,
            hostId: hostId!,
            jobId: selected.id,
            jobRevision: selected.revision,
          );
      if (!live(token)) return;
      original = captured;
      final observed = await widget.reader.verifyExporter(widget.source, captured, kind, selector);
      if (!live(token)) return;
      setState(() {
        observation = observed;
        acceptedAt = clock.elapsedMicroseconds;
      });
      if (!save || !observed.observed) return;
      final binding = MonitoringBinding(
        id: '',
        name: name.text.trim(),
        hostId: captured.hostId,
        sourceId: captured.sourceId,
        revision: 0,
        sourceRevision: captured.sourceRevision,
        selector: selector,
        state: MonitoringBindingState.current,
      );
      binding.validate(create: true);
      // Native repeats exact original profile/nonce/host/job/source checks in
      // the EXISTING metadata writer, with the actual catalog/source CAS.
      final saved = await widget.store.saveExporterBinding(binding, captured);
      if (!live(token)) return;
      widget.onSaved(saved);
    } catch (_) {
      if (live(token)) {
        setState(() {
          failed = true;
          observation = null;
        });
      }
    } finally {
      if (live(token)) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return GlassDialog(
      title: l.monitoringExporterTitle,
      width: 640,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('${widget.source.name} · ${widget.source.revision}'),
            Text(l.monitoringExporterHint),
            const SizedBox(height: 12),
            DropdownButtonFormField<String>(
              key: const ValueKey('exporter-host'),
              initialValue: hostId,
              decoration: InputDecoration(labelText: l.monitoringBindingHost),
              items: [for (final host in widget.hosts) DropdownMenuItem(value: host.id, child: Text(host.name))],
              onChanged: busy
                  ? null
                  : (value) => setState(() {
                      invalidate();
                      hostId = value;
                      jobId = null;
                      jobs = const [];
                    }),
            ),
            TextButton(
              key: const ValueKey('exporter-load-jobs'),
              onPressed: busy || hostId == null ? null : loadJobs,
              child: Text(l.monitoringExporterLoadJobs),
            ),
            DropdownButtonFormField<String>(
              key: ValueKey('exporter-job-${hostId ?? ''}-${jobs.length}'),
              initialValue: jobId,
              decoration: InputDecoration(labelText: l.monitoringExporterJob),
              items: [for (final job in jobs) DropdownMenuItem(value: job.id, child: Text(job.id))],
              onChanged: busy
                  ? null
                  : (value) => setState(() {
                      invalidate();
                      jobId = value;
                    }),
            ),
            DropdownButtonFormField<MonitoringExporterKind>(
              initialValue: kind,
              decoration: InputDecoration(labelText: l.monitoringExporterKind),
              items: [
                for (final value in MonitoringExporterKind.values)
                  DropdownMenuItem(
                    value: value,
                    child: Text(value == MonitoringExporterKind.nodeExporter ? 'node_exporter' : 'cAdvisor'),
                  ),
              ],
              onChanged: busy
                  ? null
                  : (value) => setState(() {
                      invalidate();
                      kind = value!;
                    }),
            ),
            TextField(
              key: const ValueKey('exporter-name'),
              controller: name,
              enabled: !busy,
              decoration: InputDecoration(labelText: l.monitoringBindingName),
            ),
            TextField(
              key: const ValueKey('exporter-job-label'),
              controller: jobLabel,
              enabled: !busy,
              decoration: InputDecoration(labelText: l.monitoringExporterJobLabel),
            ),
            TextField(
              key: const ValueKey('exporter-instance'),
              controller: instance,
              enabled: !busy,
              decoration: InputDecoration(labelText: l.monitoringExporterInstance),
            ),
            if (failed)
              InfoBanner(
                key: const ValueKey('exporter-error'),
                message: l.monitoringExporterChanged,
                tone: BannerTone.danger,
              ),
            if (observation != null)
              Text(observation!.observed ? l.monitoringExporterObserved : l.monitoringExporterUnavailable),
          ],
        ),
      ),
      secondaryActions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l.commonCancel)),
        if (busy)
          TextButton(
            key: const ValueKey('exporter-cancel-read'),
            onPressed: () => setState(invalidate),
            child: Text(l.commonCancel),
          ),
        GlassButton(
          key: const ValueKey('exporter-verify'),
          label: l.monitoringExporterVerify,
          onPressed: busy || jobId == null ? null : verify,
        ),
        GlassButton(
          key: const ValueKey('exporter-save'),
          label: l.monitoringBindingSave,
          onPressed: busy || !fresh || name.text.trim().isEmpty ? null : () => verify(save: true),
        ),
      ],
    );
  }
}
