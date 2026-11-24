import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

class DockerDiskUsage extends StatefulWidget {
  const DockerDiskUsage({required this.service, required this.sessionId, required this.query, super.key});
  final DockerObservationService service;
  final String sessionId;
  final String query;

  @override
  State<DockerDiskUsage> createState() => DockerDiskUsageState();
}

class DockerDiskUsageState extends State<DockerDiskUsage> {
  bool _loading = false;
  late Future<DockerJson> _data = _load();

  Future<DockerJson> _load() async {
    _loading = true;
    try {
      return await widget.service.diskUsage(widget.sessionId);
    } finally {
      _loading = false;
    }
  }

  void refresh() {
    if (!_loading) setState(() => _data = _load());
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, t = GlassTokens.of(context);
    final caption = t.typography.caption.copyWith(color: t.secondaryLabel);
    final query = widget.query.trim().toLowerCase();
    final categories = [
      ('images', l.kubeImages, Icons.layers_outlined),
      ('containers', l.dockerStandalone, Icons.view_in_ar_outlined),
      ('volumes', l.dockerVolumes, Icons.storage_outlined),
      ('cache', l.dockerBuildCache, Icons.build_outlined),
    ].where((entry) => entry.$2.toLowerCase().contains(query)).toList();
    return FutureBuilder<DockerJson>(
      future: _data,
      builder: (context, snapshot) {
        if (snapshot.hasError) return Center(child: Text(dockerErrorText(l, snapshot.error!)));
        final data = snapshot.data?.cast<String, DockerJson>();
        if (data == null) return const Center(child: CircularProgressIndicator());
        if (categories.isEmpty) return Center(child: Text(l.dockerNoMatches));
        return LayoutBuilder(
          builder: (context, constraints) {
            const gap = 8.0;
            final columns = ((constraints.maxWidth + gap) / (MediaQuery.textScalerOf(context).scale(205) + gap))
                .floor()
                .clamp(1, 4);
            final width = (constraints.maxWidth - (columns - 1) * gap) / columns;
            return SingleChildScrollView(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: Wrap(
                spacing: gap,
                runSpacing: gap,
                children: [
                  for (final (key, name, icon) in categories)
                    SizedBox(
                      width: width,
                      child: ContentSurface(
                        key: ValueKey('docker-usage-$key'),
                        kind: ContentSurfaceKind.inset,
                        padding: const EdgeInsets.all(12),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Row(
                              children: [
                                Icon(icon, size: 15, color: t.secondaryLabel),
                                const SizedBox(width: 6),
                                Expanded(child: Text(name, style: t.typography.callout)),
                              ],
                            ),
                            const SizedBox(height: 6),
                            Text(switch (data[key]?['bytes']) {
                              final int bytes => formatBytes(l, bytes),
                              _ => '—',
                            }, style: t.typography.title2),
                            if (data[key]?['count'] case final int count)
                              Text(l.dockerObjectsCount(count), style: caption),
                            if (data[key]?['reclaimable'] case final int bytes)
                              Text(l.dockerReclaimable(formatBytes(l, bytes)), style: caption),
                          ],
                        ),
                      ),
                    ),
                ],
              ),
            );
          },
        );
      },
    );
  }
}

class DockerEventJournal extends StatefulWidget {
  const DockerEventJournal({required this.service, required this.sessionId, required this.query, super.key});
  final DockerObservationService service;
  final String sessionId;
  final String query;

  @override
  State<DockerEventJournal> createState() => DockerEventJournalState();
}

class DockerEventJournalState extends State<DockerEventJournal> {
  List<DockerJson>? _events;
  Object? _error;
  bool _loading = false;

  @override
  void initState() {
    super.initState();
    refresh();
  }

  Future<void> refresh() async {
    if (_loading) return;
    _loading = true;
    try {
      final events = await widget.service.recentEvents(widget.sessionId);
      if (mounted) {
        setState(() {
          _events = events;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      _loading = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, t = GlassTokens.of(context);
    final data = _events;
    final failure = _error;
    if (data == null) {
      return Center(child: failure == null ? const CircularProgressIndicator() : Text(dockerErrorText(l, failure)));
    }
    final caption = t.typography.caption.copyWith(color: t.secondaryLabel, fontSize: 11);
    final query = widget.query.trim().toLowerCase();
    final dateFormat = DateFormat.yMd(l.localeName).add_Hms();
    String label(String action) => l.dockerLifecycleAction(action.replaceFirst('health_status: ', ''));
    final rows = data
        .where(
          (e) =>
              query.isEmpty ||
              [
                e['name'],
                e['id'],
                e['action'],
                label(e['action'] as String),
              ].whereType<String>().any((s) => s.toLowerCase().contains(query)),
        )
        .toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 8),
          child: Text(l.dockerRecentLifecycle, style: caption),
        ),
        if (failure != null)
          Text(dockerErrorText(l, failure), style: t.typography.caption.copyWith(color: t.palette.danger)),
        Expanded(
          child: rows.isEmpty
              ? Center(child: Text(query.isEmpty ? l.dockerJournalEmpty : l.dockerNoMatches))
              : ListView.separated(
                  itemCount: rows.length,
                  separatorBuilder: (_, _) => const SizedBox(height: 4),
                  itemBuilder: (context, index) {
                    final e = rows[index], action = e['action'] as String;
                    final name = e['name'] as String?, id = e['id'] as String? ?? '';
                    final exitCode = e['exitCode'] as int?;
                    final time = DateTime.fromMillisecondsSinceEpoch(e['timeMs'] as int);
                    return ContentSurface(
                      kind: ContentSurfaceKind.inset,
                      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
                      child: Row(
                        children: [
                          Icon(
                            switch (action) {
                              'start' || 'unpause' => Icons.play_arrow_rounded,
                              'restart' => Icons.restart_alt,
                              'die' || 'stop' || 'kill' => Icons.stop_outlined,
                              'destroy' => Icons.delete_outline,
                              'oom' => Icons.warning_amber_rounded,
                              _ => Icons.history_rounded,
                            },
                            size: 15,
                            color: action == 'oom' || (exitCode != null && exitCode != 0)
                                ? t.palette.danger
                                : t.secondaryLabel,
                          ),
                          const SizedBox(width: 8),
                          Expanded(
                            child: Wrap(
                              spacing: 12,
                              runSpacing: 2,
                              crossAxisAlignment: WrapCrossAlignment.center,
                              children: [
                                Text(
                                  name?.isNotEmpty == true ? name! : id,
                                  style: t.typography.bodyEmph.copyWith(fontSize: 12),
                                ),
                                Text(label(action), style: t.typography.callout),
                                if (exitCode != null) Text(l.dockerKubeExitCode(exitCode), style: caption),
                                Text(dateFormat.format(time), style: caption),
                              ],
                            ),
                          ),
                        ],
                      ),
                    );
                  },
                ),
        ),
      ],
    );
  }
}
