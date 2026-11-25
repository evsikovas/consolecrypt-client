import 'dart:convert';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/src/rust/api/ssh.dart' as ssh;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

typedef SystemdRequest = Future<Json> Function(Host host, Json operation);

final systemdRequestProvider = Provider<SystemdRequest>(
  (ref) =>
      (host, operation) async => decodeObject(
        await guard(
          () => ssh.hostSystemd(
            hostId: host.id.value,
            hostUpdatedAtMs: host.updatedAt.millisecondsSinceEpoch,
            requestJson: jsonEncode(operation),
          ),
        ),
      ),
);

typedef _Unit = ({String name, String state, String description});
final _unitLine = RegExp(r'^(\S+\.service)\s+\S+\s+(\S+)\s+(\S+)(?:\s+(.*))?$');

List<_Unit> _units(String output) => [
  for (final line in const LineSplitter().convert(output))
    if (_unitLine.firstMatch(line.trim()) case final m?)
      (name: m[1]!, state: [m[2]!, m[3]!].join(' · '), description: m[4] ?? ''),
];

String? _failure(AppLocalizations l, Json reply, {bool listing = false}) {
  if (reply['timeout'] == true) return l.systemdTimedOut;
  if (reply['status'] == 0) return null;
  if (listing && reply['status'] == 69) return l.systemdUnavailable;
  final diagnostic = (reply['diagnostic'] as String? ?? '').trim();
  return diagnostic.isEmpty ? l.systemdFailure : [l.systemdFailure, diagnostic].join('\n');
}

class SystemServicesPanel extends ConsumerStatefulWidget {
  const SystemServicesPanel({required this.host, super.key});
  final Host host;

  @override
  ConsumerState<SystemServicesPanel> createState() => _SystemServicesState();
}

class _SystemServicesState extends ConsumerState<SystemServicesPanel> {
  final search = TextEditingController();
  late final profile = ref.read(activeProfileProvider)?.id;
  late Future<Json> services;
  Future<Json>? journal;
  _Unit? selected;
  String? notice;
  bool restarting = false;

  @override
  void initState() {
    super.initState();
    services = request({'action': 'list'});
  }

  @override
  void dispose() {
    search.dispose();
    super.dispose();
  }

  Future<Json> request(Json operation) => ref.read(systemdRequestProvider)(widget.host, operation);

  void refresh() {
    setState(() {
      services = request({'action': 'list'});
      if (selected case final unit?) {
        journal = services.then((_) {
          if (!mounted || !inProfile || selected != unit) return <String, Object?>{};
          return request({'action': 'journal', 'unit': unit.name});
        });
      }
    });
  }

  void open(_Unit unit) {
    setState(() {
      selected = unit;
      notice = null;
      journal = request({'action': 'journal', 'unit': unit.name});
    });
  }

  Future<void> restart(_Unit unit) async {
    if (restarting) return;
    setState(() => restarting = true);
    final l = context.l10n;
    var sudo = false;
    try {
      final ok = await showConfirmDialog(
        context,
        title: l.dockerActionRestart,
        message: l.systemdConfirm(unit.name, {widget.host.name, widget.host.address}.join(' · ')),
        confirmLabel: l.dockerActionRestart,
        secure: true,
        destructive: true,
        extra: StatefulBuilder(
          builder: (_, setDialogState) => CheckboxListTile(
            contentPadding: EdgeInsets.zero,
            title: Text(l.systemdSudo),
            value: sudo,
            onChanged: (value) => setDialogState(() => sudo = value == true),
          ),
        ),
      );
      if (!ok || !mounted || !inProfile || selected != unit) return;
      final reply = await request({'action': 'restart', 'unit': unit.name, 'sudo': sudo});
      if (!mounted || !inProfile) return;
      notice = _failure(l, reply) ?? l.systemdRestarted;
      refresh();
    } catch (error) {
      if (mounted) setState(() => notice = errorMessage(l, error));
    } finally {
      if (mounted) setState(() => restarting = false);
    }
  }

  bool get inProfile =>
      profile != null &&
      profile == ref.read(activeProfileProvider)?.id &&
      ref.read(appStageProvider) == AppStage.unlocked;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final t = GlassTokens.of(context);
    if (ref.watch(activeProfileProvider)?.id != profile || ref.watch(appStageProvider) != AppStage.unlocked) {
      return const SizedBox.shrink();
    }
    final unit = selected;
    return Padding(
      padding: const EdgeInsets.all(GlassSpacing.s12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(widget.host.name, style: t.typography.bodyEmph),
          const SizedBox(height: GlassSpacing.s8),
          Row(
            children: [
              if (unit != null)
                GlassIconButton(
                  tooltip: l.commonBack,
                  icon: Icons.arrow_back_rounded,
                  onPressed: restarting
                      ? null
                      : () => setState(() {
                          selected = null;
                          journal = null;
                          notice = null;
                        }),
                ),
              Expanded(
                child: unit == null
                    ? GlassField(
                        controller: search,
                        search: true,
                        placeholder: l.systemdSearch,
                        onChanged: (_) => setState(() {}),
                      )
                    : SelectableText(unit.name, style: t.typography.bodyEmph),
              ),
              GlassIconButton(
                tooltip: l.devicesRefresh,
                icon: Icons.refresh_rounded,
                onPressed: restarting ? null : refresh,
              ),
            ],
          ),
          const SizedBox(height: 8),
          Expanded(
            child: FutureBuilder<Json>(
              future: services,
              builder: (context, snapshot) {
                final failure = snapshot.hasError
                    ? errorMessage(l, snapshot.error!)
                    : snapshot.hasData
                    ? _failure(l, snapshot.requireData, listing: true)
                    : null;
                final units = snapshot.hasData
                    ? _units(snapshot.requireData['output'] as String? ?? '')
                    : const <_Unit>[];
                if (unit != null) {
                  final latest = units.where((u) => u.name == unit.name).firstOrNull;
                  return ListView(
                    primary: false,
                    children: [
                      if (latest != null) ...[
                        if (latest.description.isNotEmpty) Text(latest.description),
                        Text(latest.state, style: t.typography.caption),
                      ],
                      if (failure != null) SelectableText(failure),
                      const SizedBox(height: 12),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: GlassButton(
                          icon: Icons.restart_alt_rounded,
                          label: l.dockerActionRestart,
                          onPressed: restarting || latest == null ? null : () => restart(unit),
                        ),
                      ),
                      if (restarting) const LinearProgressIndicator(),
                      if (notice case final message?) SelectableText(message),
                      const Divider(height: 24),
                      Text(l.systemdJournal, style: t.typography.bodyEmph),
                      const SizedBox(height: 8),
                      FutureBuilder<Json>(
                        future: journal,
                        builder: (context, logs) {
                          if (logs.connectionState != ConnectionState.done) return const LinearProgressIndicator();
                          if (logs.hasError) return SelectableText(errorMessage(l, logs.error!));
                          final reply = logs.requireData;
                          final error = _failure(l, reply);
                          if (error != null) return SelectableText(error);
                          final output = (reply['output'] as String).trimRight();
                          final hint = (reply['diagnostic'] as String? ?? '').trim();
                          return SelectableText(
                            [
                              if (hint.isNotEmpty) hint,
                              output.isEmpty || output == '-- No entries --' ? l.systemdNoEntries : output,
                            ].join('\n'),
                            style: t.typography.callout.copyWith(fontFamily: 'monospace'),
                          );
                        },
                      ),
                    ],
                  );
                }
                if (snapshot.connectionState != ConnectionState.done) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (failure != null) return SingleChildScrollView(child: SelectableText(failure));
                final query = search.text.trim().toLowerCase();
                final filtered = units
                    .where((u) => [u.name, u.description].any((s) => s.toLowerCase().contains(query)))
                    .toList();
                if (filtered.isEmpty) return Center(child: Text(l.systemdEmpty));
                return Material(
                  type: MaterialType.transparency,
                  child: ListView.builder(
                    key: PageStorageKey(('systemd-list', widget.host.id)),
                    primary: false,
                    itemCount: filtered.length,
                    itemBuilder: (_, index) {
                      final u = filtered[index];
                      return ListTile(
                        title: Text(u.name),
                        subtitle: Text(u.state),
                        trailing: const Icon(Icons.chevron_right_rounded),
                        onTap: () => open(u),
                      );
                    },
                  ),
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}
