import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/hosts/systemd_journal.dart';
import 'package:consolecrypt/src/rust/api/ssh.dart' as ssh;
import 'package:flutter/foundation.dart' show SynchronousFuture;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

part 'systemd_journal_view.dart';

typedef SystemdRequest = Future<Json> Function(Host host, Json operation);

final systemdRequestProvider = Provider<SystemdRequest>((ref) {
  return (host, operation) async {
    if (operation['action'] == 'journal' && host.backend == SshBackend.native) {
      final terminal = ref.read(activeSshTerminalProvider);
      final transport = ref.read(terminalServiceProvider);
      if (transport is ConnectedSshRuntime) {
        final parent = terminal?.tmuxParent ?? terminal;
        final id = parent == null
            ? null
            : (transport as ConnectedSshRuntime).connectedSshRuntimeId(parent.sessionId, host.id);
        if (id == null) throw const AppException(AppErrorCode.notFound, 'SSH session closed');
        operation = {...operation, 'terminal': id};
      }
    }
    return decodeObject(
      await guard(
        () => ssh.hostSystemd(
          hostId: host.id.value,
          hostUpdatedAtMs: host.updatedAt.millisecondsSinceEpoch,
          requestJson: jsonEncode(operation),
        ),
      ),
    );
  };
});

typedef _Unit = ({String name, String active, String sub, String description});
typedef _Journal = ({Json reply, List<ServiceLogEntry> entries});

enum _ServiceFilter { all, running, active, inactive, failed, changing }

enum _ServiceSort { name, reverseName, state }

final _unitLine = RegExp(r'^(\S+\.service)\s+\S+\s+(\S+)\s+(\S+)(?:\s+(.*))?$');

List<_Unit> _units(String output) => [
  for (final line in const LineSplitter().convert(output))
    if (_unitLine.firstMatch(line.trim()) case final m?)
      (name: m[1]!, active: m[2]!, sub: m[3]!, description: m[4] ?? ''),
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
  Future<_Journal>? journal;
  final detailScroll = ScrollController();
  Timer? followTimer;
  int followGeneration = 0;
  bool following = false;
  String? followError;
  Future<Json>? path, contents;
  _ServiceFilter filter = _ServiceFilter.all;
  _ServiceSort sort = _ServiceSort.name;
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
    followTimer?.cancel();
    detailScroll.dispose();
    search.dispose();
    super.dispose();
  }

  Future<Json> request(Json operation) => ref.read(systemdRequestProvider)(widget.host, operation);

  Future<_Journal> readJournal(_Unit unit, [_Journal? previous]) async {
    final cursor = previous?.entries.lastOrNull?.cursor;
    final reply = await request({'action': 'journal', 'unit': unit.name, 'after_cursor': ?cursor});
    final fresh = ServiceLogEntry.parseOutput(reply['output'] as String? ?? '');
    if (cursor == null || reply['status'] != 0) return (reply: reply, entries: fresh);
    if (fresh.isEmpty) return (reply: reply, entries: previous!.entries);
    final entries = [...previous!.entries, ...fresh];
    return (reply: reply, entries: entries.length > 200 ? entries.sublist(entries.length - 200) : entries);
  }

  void scheduleFollow() {
    followTimer?.cancel();
    final generation = ++followGeneration;
    final current = journal;
    if (!following || current == null) return;
    void schedule(_Journal? previous) {
      if (mounted && generation == followGeneration) {
        followTimer = Timer(const Duration(seconds: 3), () => pollJournal(generation, previous));
      }
    }

    // Even a slow initial read or manual refresh must finish before polling.
    current.then<void>(schedule, onError: (Object _, StackTrace _) => schedule(null));
  }

  Future<void> pollJournal(int generation, _Journal? previous) async {
    if (!mounted || generation != followGeneration || !inProfile) return;
    _Journal? update;
    String? error;
    try {
      // A rotated or vacuumed journal may no longer recognize its cursor.
      update = await readJournal(selected!, followError == null ? previous : null);
    } catch (e) {
      if (mounted) error = errorMessage(context.l10n, e);
    }
    if (!mounted || generation != followGeneration || !inProfile) return;
    if (update != null) error = _failure(context.l10n, update.reply);
    final atEnd = detailScroll.hasClients && detailScroll.position.extentAfter < 48;
    setState(() {
      followError = error;
      if (error == null && update != null) journal = SynchronousFuture(update);
    });
    if (atEnd && error == null) revealLatest();
    scheduleFollow();
  }

  void revealLatest([int passes = 3]) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted &&
          following &&
          detailScroll.hasClients &&
          !detailScroll.position.isScrollingNotifier.value &&
          detailScroll.position.extentAfter > 0) {
        detailScroll.jumpTo(detailScroll.position.maxScrollExtent);
        // Variable-height journal rows get measured as the sliver reaches them.
        if (passes > 1) revealLatest(passes - 1);
      }
    });
  }

  void stopFollowing() {
    following = false;
    followError = null;
    followTimer?.cancel();
    followGeneration++;
  }

  void setFollowing(bool value) {
    setState(() {
      following = value;
      followError = null;
    });
    scheduleFollow();
    if (value) revealLatest();
  }

  void refresh() {
    setState(() {
      services = request({'action': 'list'});
      if (selected case final unit?) {
        path = request({'action': 'unit_path', 'unit': unit.name});
        if (contents != null) contents = request({'action': 'unit_text', 'unit': unit.name});
        journal = readJournal(unit);
        followError = null;
        scheduleFollow();
      }
    });
  }

  void open(_Unit unit) {
    setState(() {
      selected = unit;
      stopFollowing();
      notice = null;
      contents = null;
      path = request({'action': 'unit_path', 'unit': unit.name});
      journal = readJournal(unit);
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
          Text(widget.host.name, style: t.typography.callout.copyWith(color: t.secondaryLabel)),
          const SizedBox(height: GlassSpacing.s12),
          Row(
            spacing: GlassSpacing.s12,
            children: [
              if (unit != null)
                GlassIconButton(
                  tooltip: l.commonBack,
                  icon: Icons.arrow_back_rounded,
                  onPressed: restarting
                      ? null
                      : () => setState(() {
                          selected = null;
                          stopFollowing();
                          journal = null;
                          path = null;
                          contents = null;
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
          const SizedBox(height: GlassSpacing.s16),
          if (unit == null) ...[
            Wrap(
              spacing: GlassSpacing.s6,
              runSpacing: GlassSpacing.s4,
              children: [
                GlassSelect<_ServiceFilter>(
                  key: const ValueKey('systemd-filter'),
                  semanticLabel: l.systemdFilter,
                  value: filter,
                  items: [
                    for (final f in _ServiceFilter.values)
                      GlassSelectItem(
                        value: f,
                        label: switch (f) {
                          _ServiceFilter.all => l.systemdAll,
                          _ServiceFilter.running => l.systemdRunning,
                          _ServiceFilter.active => l.systemdActive,
                          _ServiceFilter.inactive => l.systemdInactive,
                          _ServiceFilter.failed => l.systemdFailed,
                          _ServiceFilter.changing => l.systemdChanging,
                        },
                      ),
                  ],
                  onChanged: (v) => setState(() => filter = v),
                ),
                GlassSelect<_ServiceSort>(
                  key: const ValueKey('systemd-sort'),
                  value: sort,
                  semanticLabel: l.systemdSort,
                  items: [
                    GlassSelectItem(value: _ServiceSort.name, label: l.systemdSortName),
                    GlassSelectItem(value: _ServiceSort.reverseName, label: l.systemdSortReverse),
                    GlassSelectItem(value: _ServiceSort.state, label: l.systemdSortState),
                  ],
                  onChanged: (v) => setState(() => sort = v),
                ),
              ],
            ),
            const SizedBox(height: GlassSpacing.s6),
          ],
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
                  return CustomScrollView(
                    controller: detailScroll,
                    primary: false,
                    slivers: [
                      SliverToBoxAdapter(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            if (latest != null) ...[
                              if (latest.description.isNotEmpty) ...[
                                Text(latest.description),
                                const SizedBox(height: GlassSpacing.s4),
                              ],
                              Text(
                                '${latest.active} · ${latest.sub}',
                                style: t.typography.caption.copyWith(color: t.secondaryLabel),
                              ),
                            ],
                            if (failure != null) SelectableText(failure),
                            const SizedBox(height: GlassSpacing.s12),
                            Text(l.systemdUnitPath, style: t.typography.caption.copyWith(color: t.secondaryLabel)),
                            const SizedBox(height: GlassSpacing.s4),
                            _UnitOutput(future: path!, empty: l.systemdNoUnitPath),
                            const SizedBox(height: GlassSpacing.s12),
                            Wrap(
                              spacing: GlassSpacing.s12,
                              runSpacing: GlassSpacing.s8,
                              children: [
                                GlassButton(
                                  icon: Icons.description_outlined,
                                  label: contents == null ? l.systemdReadUnit : l.systemdHideUnit,
                                  onPressed: () => setState(() {
                                    contents = contents == null
                                        ? request({'action': 'unit_text', 'unit': unit.name})
                                        : null;
                                  }),
                                ),
                                GlassButton(
                                  icon: Icons.restart_alt_rounded,
                                  label: l.dockerActionRestart,
                                  onPressed: restarting || latest == null ? null : () => restart(unit),
                                ),
                              ],
                            ),
                            if (contents case final future?)
                              Padding(
                                padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s8),
                                child: _UnitOutput(future: future, empty: l.systemdNoUnitPath),
                              ),
                            if (restarting) const LinearProgressIndicator(),
                            if (notice case final message?) SelectableText(message),
                            const SizedBox(height: GlassSpacing.s16),
                          ],
                        ),
                      ),
                      PinnedHeaderSliver(
                        child: ColoredBox(
                          color: t.surfaces.contentSolid,
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              const Divider(height: 1),
                              Padding(
                                padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s12),
                                child: Wrap(
                                  alignment: WrapAlignment.spaceBetween,
                                  crossAxisAlignment: WrapCrossAlignment.center,
                                  spacing: GlassSpacing.s12,
                                  runSpacing: GlassSpacing.s8,
                                  children: [
                                    Text(l.systemdJournal, style: t.typography.bodyEmph),
                                    FilterChip(
                                      key: const ValueKey('systemd-journal-follow'),
                                      label: Text(l.systemdFollowJournal),
                                      tooltip: l.systemdFollowJournalHint(unit.name),
                                      selected: following,
                                      onSelected: setFollowing,
                                    ),
                                  ],
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                      if (followError != null) SliverToBoxAdapter(child: SelectableText(followError!)),
                      _JournalSection(key: ValueKey(unit.name), journal: journal!),
                    ],
                  );
                }
                if (snapshot.connectionState != ConnectionState.done) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (failure != null) return SingleChildScrollView(child: SelectableText(failure));
                final query = search.text.trim().toLowerCase();
                final filtered = units
                    .where(
                      (u) => switch (filter) {
                        _ServiceFilter.all => true,
                        _ServiceFilter.running => u.sub == 'running',
                        _ServiceFilter.active => u.active == 'active',
                        _ServiceFilter.inactive => u.active == 'inactive',
                        _ServiceFilter.failed => u.active == 'failed',
                        _ServiceFilter.changing => const {
                          'activating',
                          'deactivating',
                          'reloading',
                          'refreshing',
                        }.contains(u.active),
                      },
                    )
                    .where((u) => [u.name, u.description].any((s) => s.toLowerCase().contains(query)))
                    .toList();
                filtered.sort((a, b) {
                  if (sort == _ServiceSort.state) {
                    if ((a.active == 'failed') != (b.active == 'failed')) return a.active == 'failed' ? -1 : 1;
                    final group = a.active.compareTo(b.active);
                    if (group != 0) return group;
                  }
                  return sort == _ServiceSort.reverseName ? b.name.compareTo(a.name) : a.name.compareTo(b.name);
                });
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
                        subtitle: Text('${u.active} · ${u.sub}'),
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
