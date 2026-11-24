import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/platform/system_browser.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/tcpdump/capture.dart';
import 'package:consolecrypt/tcpdump/capture_run.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

enum _Mode { receiver, servers, local }

class TcpdumpScreen extends ConsumerStatefulWidget {
  const TcpdumpScreen({super.key});
  @override
  ConsumerState<TcpdumpScreen> createState() => _TcpdumpScreenState();
}

class _TcpdumpScreenState extends ConsumerState<TcpdumpScreen> {
  _Mode mode = _Mode.receiver;
  Host? source, target;
  CaptureRun? run;
  final password = TextEditingController();
  final device = TextEditingController();
  bool get busy => run?.active == true;

  void resetRun() {
    run?.dispose();
    run = null;
    password.clear();
  }

  @override
  void dispose() {
    resetRun();
    password.dispose();
    device.dispose();
    super.dispose();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (!TickerMode.valuesOf(context).enabled && busy) run?.stop();
  }

  Future<void> choose(bool first) async {
    final profile = ref.read(activeProfileProvider)?.id;
    final host = await showHostPicker(context, exclude: mode == _Mode.servers ? (first ? target : source)?.id : null);
    if (!mounted || host == null || busy || profile != ref.read(activeProfileProvider)?.id) return;
    setState(() {
      resetRun();
      if (first) {
        source = host;
      } else {
        target = host;
      }
    });
  }

  void start({bool privileged = false}) {
    if (busy || target == null || mode == _Mode.servers && source == null) return;
    resetRun();
    final next = CaptureRun(ref.read(terminalServiceProvider));
    setState(() => run = next);
    if (mode == _Mode.local) {
      unawaited(next.startLocal(target!.address, device: device.text.trim()));
    } else {
      unawaited(
        next.startRemote(
          mode == _Mode.servers ? source! : target!,
          mode == _Mode.servers ? target!.address : null,
          privileged: privileged,
        ),
      );
    }
  }

  Future<void> install(CaptureRun current) async {
    final l = context.l10n;
    final command = current.installCommand;
    final local = mode == _Mode.local;
    final host = mode == _Mode.servers ? source : target;
    if (command == null && local && Platform.isWindows) {
      try {
        await openInSystemBrowser(Uri.parse('https://marketplace.microsoft.com/en-us/product/microolap.tcpdump'));
      } catch (_) {
        if (mounted) showSnack(context, l.captureFailed, error: true);
      }
      return;
    }
    if (command == null) return;
    final confirmed = await showConfirmDialog(
      context,
      title: l.captureInstall,
      message: l.captureInstallReview(local ? l.captureLocalPoint : host!.name),
      confirmLabel: l.navTerminal,
      extra: SelectableText(command),
    );
    if (!mounted || current != run || !confirmed) return;
    final tabs = ref.read(terminalTabsProvider.notifier);
    try {
      final TerminalTab tab;
      if (local) {
        final profiles = await ref.read(terminalServiceProvider).localProfiles();
        if (!mounted || current != run) return;
        if (profiles.profiles.isEmpty) {
          showSnack(context, l.terminalConnectionClosed, error: true);
          return;
        }
        tab = await tabs.openTarget(LocalTerminalTarget(profiles.profiles.first));
      } else {
        tab = await tabs.open(host!);
      }
      if (!mounted || current != run) {
        await tabs.close(tab);
        return;
      }
      if (!identical(ref.read(terminalTabsProvider).active, tab)) return;
      tabs.insertIntoActive(command);
      ref.read(routerProvider).go(AppRoutes.terminal);
    } catch (e) {
      if (mounted) showSnack(context, e.toString(), error: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(
      activeProfileProvider.select((p) => p?.id),
      (_, _) => setState(() {
        resetRun();
        source = null;
        target = null;
      }),
    );
    ref.listen(appStageProvider, (_, stage) {
      if (stage != AppStage.unlocked) {
        setState(() {
          resetRun();
          source = null;
          target = null;
        });
      }
    });
    final current = run;
    return current == null ? content() : ListenableBuilder(listenable: current, builder: (_, _) => content());
  }

  Widget content() {
    final l = context.l10n, current = run;
    final theme = GlassTokens.of(context);
    final mono = theme.typography.mono.copyWith(fontSize: 12);
    final body = current?.packets.toList(growable: false) ?? const <CapturePacket>[];
    final status = switch (current?.status) {
      null => l.captureChoose,
      CaptureStatus.connecting => l.terminalConnecting,
      CaptureStatus.listening => l.captureListening,
      CaptureStatus.stopped => l.captureStopped,
      CaptureStatus.missing => l.captureMissing,
      CaptureStatus.denied => l.captureDenied,
      CaptureStatus.failed => l.captureFailed,
      CaptureStatus.noPeer => l.captureNoPeer,
    };
    return PageScaffold(
      title: l.captureTitle,
      subtitle: l.captureSubtitle,
      body: CustomScrollView(
        slivers: [
          SliverToBoxAdapter(
            child: GlassPanel(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      GlassSelect<_Mode>(
                        value: mode,
                        items: [
                          GlassSelectItem(value: _Mode.receiver, label: l.captureReceiver),
                          GlassSelectItem(value: _Mode.servers, label: l.captureServers),
                          if (Platform.isWindows || Platform.isLinux || Platform.isMacOS)
                            GlassSelectItem(value: _Mode.local, label: l.captureLocal),
                        ],
                        onChanged: busy
                            ? null
                            : (v) => setState(() {
                                resetRun();
                                mode = v;
                                if (source?.id == target?.id) source = null;
                              }),
                      ),
                      if (mode == _Mode.servers)
                        GlassButton(
                          label: source?.name ?? l.captureSource,
                          icon: Icons.dns_rounded,
                          onPressed: busy ? null : () => choose(true),
                        ),
                      if (mode == _Mode.servers) const Icon(Icons.swap_horiz_rounded, size: 16),
                      GlassButton(
                        label: target?.name ?? l.captureTarget,
                        icon: Icons.dns_rounded,
                        onPressed: busy ? null : () => choose(false),
                      ),
                      if (mode == _Mode.local)
                        SizedBox(
                          width: 180,
                          child: GlassField(controller: device, placeholder: l.captureInterface, enabled: !busy),
                        ),
                      if (busy)
                        GlassButton(label: l.captureStop, icon: Icons.stop_rounded, onPressed: current!.stop)
                      else
                        GlassButton.prominent(
                          label: l.captureStart,
                          icon: Icons.play_arrow_rounded,
                          onPressed: target == null || (mode == _Mode.servers && source == null) ? null : start,
                        ),
                    ],
                  ),
                  const SizedBox(height: 8),
                  Text(switch (mode) {
                    _Mode.receiver => l.captureReceiverHelp,
                    _Mode.servers => l.captureServersHelp,
                    _Mode.local => l.captureLocalHelp,
                  }, style: theme.typography.caption),
                ],
              ),
            ),
          ),
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Wrap(
                    spacing: 12,
                    runSpacing: 4,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      Text(status, style: theme.typography.bodyEmph),
                      if (current?.observedPair case final pair?) Text(pair, style: mono),
                      if (current != null && current.count > 0)
                        Text(l.captureCount(body.length, current.count), style: theme.typography.caption),
                    ],
                  ),
                  if (current != null && !busy && current.error != null) SelectableText(current.error!, style: mono),
                  if (current?.status == CaptureStatus.missing) ...[
                    if (installCommands.containsKey(current!.manager) || mode == _Mode.local && Platform.isWindows)
                      GlassButton(
                        label: l.captureInstall,
                        icon: Icons.download_rounded,
                        onPressed: () => install(current),
                      )
                    else
                      Text(l.captureManualInstall),
                  ],
                  if (current?.status == CaptureStatus.denied && mode != _Mode.local)
                    GlassButton(label: l.captureSudo, onPressed: () => start(privileged: true)),
                  if (current?.hostKey case final key?) ...[
                    Text(l.terminalUnknownHostTitle(key.hostPattern)),
                    SelectableText('${key.keyType}  ${key.fingerprintSha256}', style: mono),
                    Wrap(
                      spacing: 8,
                      children: [
                        GlassButton(
                          label: l.terminalHostKeyReject,
                          onPressed: () => current!.answerKey(HostKeyDecision.reject),
                        ),
                        GlassButton(
                          label: l.terminalHostKeyAcceptOnce,
                          onPressed: () => current!.answerKey(HostKeyDecision.acceptOnce),
                        ),
                        GlassButton(
                          label: l.terminalHostKeyAcceptAndSave,
                          onPressed: () => current!.answerKey(HostKeyDecision.acceptAndSave),
                        ),
                      ],
                    ),
                  ],
                  if (current?.passwordPrompt case final prompt?) ...[
                    Text(
                      prompt.keyName == null
                          ? l.terminalPasswordPromptTitle(prompt.username, prompt.hostLabel)
                          : l.terminalKeyPassphrasePromptTitle(prompt.keyName!, prompt.hostLabel),
                    ),
                    if (prompt.retry) Text(l.terminalPasswordRetry),
                    SizedBox(
                      width: 320,
                      child: GlassField(
                        controller: password,
                        secret: true,
                        autofocus: true,
                        onSubmitted: (_) => submit(current!),
                      ),
                    ),
                    GlassButton(label: l.commonContinue, onPressed: () => submit(current!)),
                  ],
                ],
              ),
            ),
          ),
          SliverList.builder(
            itemCount: body.length,
            itemBuilder: (context, i) {
              final p = body[i];
              final time = p.time.toLocal().toIso8601String().split('T').last.substring(0, 12);
              return Container(
                decoration: BoxDecoration(
                  border: Border(bottom: BorderSide(color: theme.surfaces.separator)),
                ),
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
                child: Wrap(
                  spacing: 12,
                  runSpacing: 2,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    Text(time, style: mono.copyWith(color: theme.secondaryLabel)),
                    Text(p.protocol, style: mono.copyWith(color: theme.palette.accent)),
                    SelectableText('${p.source} → ${p.destination}', style: mono),
                    Text(p.summary, style: theme.typography.caption),
                  ],
                ),
              );
            },
          ),
        ],
      ),
    );
  }

  void submit(CaptureRun current) {
    final secret = SecretText(password.text);
    password.clear();
    unawaited(current.answerPassword(secret));
  }
}
