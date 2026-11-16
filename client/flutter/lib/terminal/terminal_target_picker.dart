import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/hosts/hosts_screen.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_route_guard.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final localShellProfilesProvider = FutureProvider.autoDispose<LocalShellProfiles>((ref) async {
  ref.watch(activeProfileProvider.select((p) => p?.id));
  if (AppPlatform.isMobile) return const LocalShellProfiles(unavailableReason: 'unsupported_platform');
  return ref.watch(terminalServiceProvider).localProfiles();
});

Future<TerminalTarget?> showTerminalTargetPicker(BuildContext context, {bool localOnly = false, JobContextFocus? contextFocus}) =>
    showAppDialog<TerminalTarget>(context, builder: (_) {
      final picker = TerminalTargetPicker(localOnly: localOnly, contextFocus: contextFocus);
      return contextFocus == null ? picker : JobContextRouteGuard(focus: contextFocus, child: picker);
    });

/// One chooser for tab-strip +, the empty state and Cmd/Ctrl+T. Native
/// discovery owns executable paths and WSL enumeration; this view uses IDs.
class TerminalTargetPicker extends ConsumerStatefulWidget {
  const TerminalTargetPicker({super.key, this.localOnly = false, this.contextFocus});
  final bool localOnly;
  final JobContextFocus? contextFocus;
  @override
  ConsumerState<TerminalTargetPicker> createState() => _TerminalTargetPickerState();
}

class _TerminalTargetPickerState extends ConsumerState<TerminalTargetPicker> {
  String _query = '';
  String? _selectedHostId;
  ProfileId? _profile;

  @override
  void initState() {
    super.initState();
    _profile = ref.read(activeProfileProvider)?.id;
    _selectedHostId = widget.contextFocus?.selection.hostId;
  }

  @override
  Widget build(BuildContext context) {
    final focus = widget.contextFocus;
    final metadataSession = focus == null ? null : ref.watch(monitoringSessionProvider);
    bool current() => ref.read(appStageProvider) == AppStage.unlocked && ref.read(activeProfileProvider)?.id == _profile &&
        (focus == null || identical(ref.read(monitoringSessionProvider), metadataSession) && identical(metadataSession?.correlationScope, focus.scope));
    void closeIfStale() {
      if (current()) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    }

    ref.listen(appStageProvider, (_, _) => closeIfStale());
    ref.listen(activeProfileProvider, (_, _) => closeIfStale());
    if (!current()) {
      closeIfStale();
      return const SizedBox.shrink();
    }
    final l = context.l10n;
    // A saved-host context never starts local/WSL discovery or falls back to a
    // local target when the exact saved host is missing.
    final profiles = focus == null ? ref.watch(localShellProfilesProvider) : const AsyncValue.data(LocalShellProfiles());
    final q = _query.trim().toLowerCase();
    bool matches(String value) => q.isEmpty || value.toLowerCase().contains(q);
    final local = (profiles.value?.profiles ?? const <LocalShellProfile>[])
        .where((p) => matches('${p.label} ${p.distribution ?? ''} ${p.shell}'))
        .toList();
    final hosts =
        (ref.watch(hostsProvider).value ?? const <Host>[])
            .where((h) => !widget.localOnly && !h.isRdp && (focus == null || h.id.value == focus.selection.hostId) && matches('${h.name} ${h.address} ${h.tags.join(' ')}'))
            .toList()
          ..sort((a, b) => a.name.compareTo(b.name));
    final targets = <TerminalTarget>[...local.map(LocalTerminalTarget.new), ...hosts.map(SshTerminalTarget.new)];
    final selectedHost = hosts.where((host) => host.id.value == _selectedHostId).firstOrNull;
    final reason = profiles.hasError ? 'discovery_failed' : profiles.value?.unavailableReason;
    return GlassDialog(
      key: const ValueKey('terminal-picker'),
      title: widget.localOnly ? l.paletteNewLocalTerminal : l.commandNewTabPickerTitle,
      width: 560,
      content: SizedBox(
        height: 420,
        child: Column(
          children: [
            if (focus != null) ...[
              Text(l.correlationFocusedHostHint, key: const ValueKey('terminal-context-focus-hint')),
              const SizedBox(height: GlassSpacing.s8),
            ],
            GlassField(
              key: const ValueKey('host-picker-search'),
              search: true,
              autofocus: true,
              leadingIcon: Icons.search_rounded,
              placeholder: l.terminalLocalSearch,
              onChanged: (value) => setState(() => _query = value),
              onSubmitted: (_) {
                if (focus == null && targets.isNotEmpty) closeDialog(context, targets.first);
              },
            ),
            const SizedBox(height: GlassSpacing.s8),
            Expanded(
              child: ListView(
                children: [
                  if (!AppPlatform.isMobile && focus == null) ...[
                    ListTile(dense: true, title: Text(l.terminalLocalSection)),
                    if (profiles.isLoading) const LinearProgressIndicator(),
                    for (final profile in local)
                      ListTile(
                        key: ValueKey('pick-local-${profile.id}'),
                        leading: const Icon(Icons.terminal_rounded),
                        title: Text(
                          profile.distribution ?? profile.label,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        subtitle: Text(
                          profile.distribution == null ? l.terminalLocalComputer : 'WSL · ${profile.distribution}',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        onTap: () => closeDialog(context, LocalTerminalTarget(profile)),
                      ),
                    if (!profiles.isLoading && local.isEmpty && q.isEmpty)
                      ListTile(
                        title: Text(
                          reason == 'sandbox_restricted'
                              ? l.terminalLocalSandboxUnavailable
                              : l.terminalLocalUnavailable,
                        ),
                        trailing: IconButton(
                          key: const ValueKey('local-shells-refresh'),
                          icon: const Icon(Icons.refresh_rounded),
                          tooltip: l.terminalLocalRefresh,
                          onPressed: () => ref.invalidate(localShellProfilesProvider),
                        ),
                      ),
                  ],
                  if (!widget.localOnly) ListTile(dense: true, title: Text(l.terminalLocalSshSection)),
                  for (final host in hosts)
                    ListTile(
                      key: ValueKey('pick-${host.name}'),
                      selected: focus != null && host.id.value == _selectedHostId,
                      leading: HostAvatar(host: host, size: 28),
                      title: Text(host.name, maxLines: 1, overflow: TextOverflow.ellipsis),
                      subtitle: Text(host.address, maxLines: 1, overflow: TextOverflow.ellipsis),
                      onTap: () {
                        if (!current()) return;
                        if (focus == null) { closeDialog(context, SshTerminalTarget(host)); }
                        else { setState(() => _selectedHostId = host.id.value); }
                      },
                    ),
                  if (hosts.isEmpty && local.isEmpty && !profiles.isLoading) ListTile(title: Text(l.hostPickerEmpty)),
                ],
              ),
            ),
          ],
        ),
      ),
      primaryAction: focus == null ? null : GlassButton.prominent(
        key: const ValueKey('terminal-context-connect'), label: l.commonConnect,
        onPressed: selectedHost == null || !current() ? null : () {
          if (current()) closeDialog(context, SshTerminalTarget(selectedHost));
        },
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<TerminalTarget>(context))],
    );
  }
}
