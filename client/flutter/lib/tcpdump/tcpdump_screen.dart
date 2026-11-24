import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/tcpdump/capture.dart';
import 'package:consolecrypt/tcpdump/capture_run.dart';
import 'package:consolecrypt/tcpdump/connection_details.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class TcpdumpScreen extends ConsumerStatefulWidget {
  const TcpdumpScreen({super.key});
  @override
  ConsumerState<TcpdumpScreen> createState() => _TcpdumpScreenState();
}

class _TcpdumpScreenState extends ConsumerState<TcpdumpScreen> {
  Host? source, target;
  CaptureRun? run, preparation;
  CaptureRun? get currentOperation => preparation ?? run;
  bool useSudo = false;
  CapturePacket? selected;
  List<CapturePacket> exchange = const [];

  final password = TextEditingController();
  bool get busy => currentOperation?.active == true;

  void resetRun() {
    preparation?.dispose();
    preparation = null;
    run?.dispose();
    run = null;
    selected = null;
    exchange = const [];
    password.clear();
  }

  @override
  void dispose() {
    resetRun();
    password.dispose();
    super.dispose();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (!TickerMode.valuesOf(context).enabled && busy) currentOperation?.stop();
  }

  Future<void> choose(bool first) async {
    final profile = ref.read(activeProfileProvider)?.id;
    final host = await showHostPicker(context, exclude: (first ? target : source)?.id);
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

  void check() {
    final host = source;
    if (busy || host == null) return;
    preparation?.dispose();
    final next = CaptureRun(ref.read(terminalServiceProvider));
    setState(() => preparation = next);
    unawaited(next.checkRemote(host));
  }

  void start() {
    if (busy || target == null || source == null) return;
    resetRun();
    final next = CaptureRun(ref.read(terminalServiceProvider));
    setState(() => run = next);
    unawaited(next.startRemote(source!, target!.address, privileged: useSudo));
  }

  Future<void> install(CaptureRun current) async {
    final l = context.l10n;
    final command = current.installCommand;
    final host = source;
    if (host == null || command == null) return;
    final confirmed = await showConfirmDialog(
      context,
      title: l.captureInstall,
      message: l.captureInstallHost(host.name),
      confirmLabel: l.captureInstall,
      extra: SelectableText(command),
    );
    if (!mounted || current != currentOperation || !confirmed) return;
    final tabs = ref.read(terminalTabsProvider.notifier);
    try {
      final tab = await tabs.open(host);
      if (!mounted || current != currentOperation) {
        await tabs.close(tab);
        return;
      }
      if (!identical(ref.read(terminalTabsProvider).active, tab)) return;
      tabs.runInTab(tab, command);
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
    final current = currentOperation;
    return current == null ? content() : ListenableBuilder(listenable: current, builder: (_, _) => content());
  }

  Widget content() {
    final l = context.l10n, current = currentOperation;
    final theme = GlassTokens.of(context);
    final mono = theme.typography.mono.copyWith(fontSize: 12);
    final body = run?.packets.toList(growable: false) ?? const <CapturePacket>[];
    final status = switch (current?.status) {
      null => l.captureChoose,
      CaptureStatus.connecting => l.terminalConnecting,
      CaptureStatus.listening => l.captureListening,
      CaptureStatus.ready => l.captureReady,
      CaptureStatus.stopped => l.captureStopped,
      CaptureStatus.missing => l.captureMissing,
      CaptureStatus.denied => l.captureDenied,
      CaptureStatus.failed => l.captureFailed,
      CaptureStatus.noPeer => l.captureNoPeer,
    };
    return PageScaffold(
      title: l.captureTitle,
      subtitle: l.captureSubtitle,
      body: LayoutBuilder(
        builder: (context, bounds) => Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ConstrainedBox(
              constraints: BoxConstraints(maxHeight: bounds.maxHeight * 0.65),
              child: SingleChildScrollView(
                primary: false,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    GlassPanel(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Wrap(
                            spacing: 8,
                            runSpacing: 8,
                            crossAxisAlignment: WrapCrossAlignment.center,
                            children: [
                              GlassButton(
                                label: source?.name ?? l.captureSource,
                                icon: Icons.dns_rounded,
                                onPressed: busy ? null : () => choose(true),
                              ),
                              const Icon(Icons.swap_horiz_rounded, size: 16),
                              GlassButton(
                                label: target?.name ?? l.captureTarget,
                                icon: Icons.dns_rounded,
                                onPressed: busy ? null : () => choose(false),
                              ),
                              GlassButton(
                                icon: Icons.build_outlined,
                                label: l.captureCheck,
                                onPressed: busy || source == null ? null : check,
                              ),
                              GlassIconButton(
                                icon: Icons.admin_panel_settings_outlined,
                                tooltip: l.captureUseSudo,
                                selected: useSudo,
                                onPressed: busy ? null : () => setState(() => useSudo = !useSudo),
                              ),
                              if (busy)
                                GlassButton(label: l.captureStop, icon: Icons.stop_rounded, onPressed: current!.stop)
                              else
                                GlassButton.prominent(
                                  label: l.captureStart,
                                  icon: Icons.play_arrow_rounded,
                                  onPressed: target == null || source == null ? null : start,
                                ),
                            ],
                          ),
                          const SizedBox(height: 8),
                          Text(l.captureServersHelp, style: theme.typography.caption),
                          Text(l.capturePrivilegeHint, style: theme.typography.caption),
                        ],
                      ),
                    ),
                    Padding(
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
                              if (run != null && run!.count > 0)
                                Text(l.captureCount(body.length, run!.count), style: theme.typography.caption),
                            ],
                          ),
                          if (current != null && !busy && current.error != null)
                            SelectableText(current.error!, style: mono),
                          if (current?.status == CaptureStatus.missing) ...[
                            if (installCommands.containsKey(current!.manager))
                              GlassButton(
                                label: l.captureInstall,
                                icon: Icons.download_rounded,
                                onPressed: () => install(current),
                              )
                            else
                              Text(l.captureManualInstall),
                          ],
                          if (current?.status == CaptureStatus.denied)
                            GlassButton(
                              label: l.captureSudo,
                              onPressed: () {
                                useSudo = true;
                                start();
                              },
                            ),
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
                  ],
                ),
              ),
            ),
            Expanded(
              child: selected != null
                  ? CaptureConnectionDetails(
                      packet: selected!,
                      packets: exchange,
                      back: () => setState(() {
                        selected = null;
                        exchange = const [];
                      }),
                      select: (item) => setState(() => selected = item),
                    )
                  : ListView.builder(
                      key: const PageStorageKey('tcpdump-packets'),
                      itemCount: body.length,
                      itemBuilder: (context, i) {
                        final p = body[i];
                        return InkWell(
                          onTap: () => setState(() {
                            selected = p;
                            exchange = body.where(p.sharesEndpoints).toList(growable: false);
                          }),
                          child: Container(
                            decoration: BoxDecoration(
                              border: Border(bottom: BorderSide(color: theme.surfaces.separator)),
                            ),
                            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
                            child: Row(
                              children: [
                                Expanded(
                                  child: Wrap(
                                    spacing: 12,
                                    runSpacing: 2,
                                    crossAxisAlignment: WrapCrossAlignment.center,
                                    children: [
                                      Text(packetTime(p), style: mono.copyWith(color: theme.secondaryLabel)),
                                      Text(p.protocol, style: mono.copyWith(color: theme.palette.accent)),
                                      Text('${p.source} → ${p.destination}', style: mono),
                                      Text(p.summary, style: theme.typography.caption),
                                    ],
                                  ),
                                ),
                                Icon(Icons.chevron_right_rounded, size: 16, color: theme.secondaryLabel),
                              ],
                            ),
                          ),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
    );
  }

  void submit(CaptureRun current) {
    final secret = SecretText(password.text);
    password.clear();
    unawaited(current.answerPassword(secret));
  }
}
