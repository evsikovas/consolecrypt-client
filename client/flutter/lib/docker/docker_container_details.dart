import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/db/db_document_view.dart';
import 'package:consolecrypt/docker/docker_container_list.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_host_path.dart';
import 'package:consolecrypt/docker/docker_logs_view.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

enum DockerDetailsTab { overview, logs, json }

/// Container drawer: overview from `inspect`, live logs and the raw JSON.
class DockerContainerDetails extends ConsumerStatefulWidget {
  const DockerContainerDetails({
    required this.service,
    required this.session,
    required this.container,
    required this.onClose,
    required this.actions,
    this.initialTab = DockerDetailsTab.overview,
    this.tabRequest = 0,
    super.key,
  });
  final DockerService service;
  final DockerSessionInfo session;
  final DockerContainer container;
  final VoidCallback onClose;
  final DockerContainerActions actions;
  final DockerDetailsTab initialTab;

  /// Changes when the workspace asks for [initialTab] again (e.g. "Logs" on
  /// the selected row).
  final int tabRequest;
  @override
  ConsumerState<DockerContainerDetails> createState() => _DockerContainerDetailsState();
}

class _DockerContainerDetailsState extends ConsumerState<DockerContainerDetails> with SingleTickerProviderStateMixin {
  late final TabController _tabs = TabController(
    length: DockerDetailsTab.values.length,
    initialIndex: widget.initialTab.index,
    vsync: this,
  )..addListener(() => mounted ? setState(() {}) : null);
  DockerContainerDetail? _detail;
  bool _loading = true;
  Object? _error;
  int _generation = 0;

  /// The unmasked inspect after the vault passphrase; dropped on close,
  /// reload, lock and profile change.
  DockerJson? _fullInspect;
  late final _scope = ref.read(dockerScopeProvider);

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  @override
  void didUpdateWidget(covariant DockerContainerDetails oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.tabRequest != widget.tabRequest) _tabs.animateTo(widget.initialTab.index);
    if (oldWidget.container.state != widget.container.state) unawaited(_load());
  }

  @override
  void dispose() {
    _generation++;
    _fullInspect = null;
    _tabs.dispose();
    super.dispose();
  }

  Future<void> _revealJson() async {
    final container = widget.container;
    final full = await showAppDialog<DockerJson>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (context) => _RevealDialog(
        dialogKey: 'docker-json-reveal-dialog',
        title: context.l10n.dockerShowFull,
        help: context.l10n.dockerJsonRevealHelp(container.name),
        reveal: (passphrase) => widget.service.revealInspect(widget.session.id, container.id, passphrase),
      ),
    );
    if (full != null && mounted && identical(_scope, ref.read(dockerScopeProvider))) {
      setState(() => _fullInspect = full);
    }
  }

  Future<void> _load() async {
    final generation = ++_generation;
    setState(() {
      _loading = true;
      _error = null;
      _fullInspect = null;
    });
    try {
      final detail = await widget.service.inspect(widget.session.id, widget.container.id);
      if (mounted && generation == _generation) setState(() => _detail = detail);
    } catch (error) {
      if (mounted && generation == _generation) setState(() => _error = error);
    } finally {
      if (mounted && generation == _generation) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dockerScopeProvider, (_, next) {
      if (!identical(next, _scope) && mounted) setState(() => _fullInspect = null);
    });
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final container = widget.container;
    final running = container.state == DockerContainerState.running;
    final busy = widget.actions.busy(container);
    final database = widget.actions.databaseTarget(container, _detail?.inspect);
    Widget action(String key, String label, IconData icon, VoidCallback? onPressed, {String? tooltip}) => GlassButton(
      key: ValueKey('docker-details-$key'),
      label: label,
      icon: icon,
      tooltip: tooltip,
      size: GlassControlSize.sm,
      onPressed: busy ? null : onPressed,
    );
    final full = _fullInspect;
    final shown = full ?? _detail?.inspect;
    final json = shown == null ? '' : const JsonEncoder.withIndent('  ').convert(shown);
    final masked = full == null && json.contains(dockerSecretMask);
    return ContentSurface(
      key: const ValueKey('docker-container-details'),
      kind: ContentSurfaceKind.solid,
      padding: EdgeInsets.zero,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s12, GlassSpacing.s8, 0),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        container.name,
                        key: const ValueKey('docker-details-name'),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.title3.copyWith(color: p.label),
                      ),
                      const SizedBox(height: 2),
                      Row(
                        children: [
                          GlassBadge(
                            label: dockerStateLabel(l, container.state),
                            tone: dockerStateTone(container.state),
                            dense: true,
                          ),
                          const SizedBox(width: 6),
                          Flexible(
                            child: Text(
                              container.shortId,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: tokens.typography.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
                GlassIconButton(
                  key: const ValueKey('docker-details-refresh'),
                  icon: Icons.refresh,
                  tooltip: l.dockerRefresh,
                  style: GlassIconButtonStyle.plain,
                  onPressed: _loading ? null : _load,
                ),
                GlassMenuButton<Object>(
                  key: const ValueKey('docker-details-menu'),
                  entries: dockerContainerMenu(l, container, database: database),
                  onSelected: (value) => runDockerMenuValue(widget.actions, container, value, database: database),
                  builder: (context, open) => GlassIconButton(
                    icon: Icons.more_horiz_rounded,
                    tooltip: MaterialLocalizations.of(context).moreButtonTooltip,
                    style: GlassIconButtonStyle.plain,
                    onPressed: open,
                  ),
                ),
                GlassIconButton(
                  key: const ValueKey('docker-details-close'),
                  icon: Icons.close_rounded,
                  tooltip: l.commonClose,
                  style: GlassIconButtonStyle.plain,
                  onPressed: widget.onClose,
                ),
              ],
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s8, GlassSpacing.s16, 0),
            child: Wrap(
              spacing: GlassSpacing.s6,
              runSpacing: GlassSpacing.s6,
              children: [
                if (dockerActionAllowed(DockerAction.start, container.state))
                  action(
                    'start',
                    l.dockerActionStart,
                    Icons.play_arrow_rounded,
                    () => unawaited(widget.actions.run(container, DockerAction.start)),
                  )
                else
                  action(
                    'stop',
                    l.dockerActionStop,
                    Icons.stop_rounded,
                    dockerActionAllowed(DockerAction.stop, container.state)
                        ? () => unawaited(widget.actions.run(container, DockerAction.stop))
                        : null,
                  ),
                action(
                  'restart',
                  l.dockerActionRestart,
                  Icons.restart_alt_rounded,
                  dockerActionAllowed(DockerAction.restart, container.state)
                      ? () => unawaited(widget.actions.run(container, DockerAction.restart))
                      : null,
                ),
                action(
                  'terminal',
                  l.dockerTerminal,
                  Icons.terminal_rounded,
                  running ? () => unawaited(widget.actions.terminal(container)) : null,
                ),
                action(
                  'forward',
                  l.dockerForwardPort,
                  Icons.swap_horiz_rounded,
                  running ? () => unawaited(widget.actions.forward(container)) : null,
                ),
                if (database != null)
                  action(
                    'database',
                    l.kubeConnectDatabase,
                    Icons.storage_rounded,
                    running ? () => unawaited(widget.actions.database!(container, database)) : null,
                    tooltip: l.dockerConnectDatabaseHelp(database.engine.label),
                  ),
              ],
            ),
          ),
          if (_loading) const Padding(padding: EdgeInsets.only(top: 6), child: LinearProgressIndicator(minHeight: 2)),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s8, GlassSpacing.s16, 0),
              child: Text(
                dockerErrorText(l, _error!),
                key: const ValueKey('docker-details-error'),
                style: tokens.typography.callout.copyWith(color: p.danger),
              ),
            ),
          const SizedBox(height: GlassSpacing.s8),
          TabBar(
            controller: _tabs,
            isScrollable: true,
            tabAlignment: TabAlignment.start,
            labelColor: p.accent,
            unselectedLabelColor: tokens.secondaryLabel,
            labelStyle: tokens.typography.bodyEmph,
            unselectedLabelStyle: tokens.typography.body,
            indicatorColor: p.accent,
            indicatorSize: TabBarIndicatorSize.label,
            dividerColor: tokens.surfaces.separator,
            labelPadding: const EdgeInsets.symmetric(horizontal: 12),
            padding: const EdgeInsets.symmetric(horizontal: 4),
            tabs: [
              Tab(height: 36, key: const ValueKey('docker-tab-overview'), text: l.dockerOverview),
              Tab(height: 36, key: const ValueKey('docker-tab-logs'), text: l.dockerLogs),
              Tab(height: 36, key: const ValueKey('docker-tab-json'), text: l.dockerInspectJson),
            ],
          ),
          Expanded(
            child: TabBarView(
              controller: _tabs,
              children: [
                _DockerOverview(
                  container: container,
                  detail: _detail,
                  service: widget.service,
                  session: widget.session,
                ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s12, 0),
                  child: DockerLogsView(
                    service: widget.service,
                    sessionId: widget.session.id,
                    containerId: container.id,
                    active: _tabs.index == DockerDetailsTab.logs.index,
                  ),
                ),
                _detail == null
                    ? const SizedBox.shrink()
                    : Padding(
                        key: const ValueKey('docker-inspect-json'),
                        padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s4, GlassSpacing.s12, 0),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Wrap(
                              spacing: GlassSpacing.s6,
                              crossAxisAlignment: WrapCrossAlignment.center,
                              children: [
                                GlassButton.plain(
                                  key: const ValueKey('docker-json-copy'),
                                  label: l.dockerCopyJson,
                                  icon: Icons.content_copy_rounded,
                                  // The full JSON is a secret: auto-cleared.
                                  onPressed: () => full != null
                                      ? copySecretWithNotice(context, ref, json, what: l.dockerInspectJson)
                                      : copyPlainWithNotice(context, ref, json),
                                ),
                                if (masked)
                                  GlassButton.plain(
                                    key: const ValueKey('docker-json-reveal'),
                                    label: l.dockerShowFull,
                                    icon: Icons.visibility_outlined,
                                    onPressed: () => unawaited(_revealJson()),
                                  ),
                                if (full != null)
                                  GlassButton.plain(
                                    key: const ValueKey('docker-json-hide'),
                                    label: l.dockerHideSecrets,
                                    icon: Icons.visibility_off_outlined,
                                    onPressed: () => setState(() => _fullInspect = null),
                                  ),
                                if (masked)
                                  Text(
                                    l.dockerSecretsMasked,
                                    key: const ValueKey('docker-json-masked'),
                                    style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                                  ),
                              ],
                            ),
                            Expanded(
                              child: RevealedSecret(
                                visible: full != null,
                                child: DbDocumentView(
                                  key: ValueKey(full == null ? 'docker-json-masked-view' : 'docker-json-full-view'),
                                  document: shown!,
                                  rawText: json,
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _DockerOverview extends StatelessWidget {
  const _DockerOverview({required this.container, required this.detail, required this.service, required this.session});
  final DockerContainer container;
  final DockerContainerDetail? detail;
  final DockerService service;
  final DockerSessionInfo session;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final inspect = detail?.inspect ?? const <String, dynamic>{};
    final config = inspect['Config'] as Map? ?? const {};
    final state = inspect['State'] as Map? ?? const {};
    String text(Object? value) => value is String || value is num || value is bool ? value.toString() : '';
    final created = DateTime.tryParse(text(inspect['Created']))?.toLocal() ?? container.createdAt?.toLocal();
    final command = [
      if (inspect['Path'] case final String path when path.isNotEmpty) path,
      for (final arg in inspect['Args'] as List? ?? const []) text(arg),
    ].join(' ');
    final mounts = (inspect['Mounts'] as List? ?? const []).whereType<Map<Object?, Object?>>().toList();
    final networkSettings = (inspect['NetworkSettings'] as Map?)?['Networks'];
    final networks = <String, String>{
      ...container.networks,
      if (networkSettings is Map)
        for (final entry in networkSettings.entries)
          entry.key.toString(): text((entry.value as Map?)?['IPAddress']).isEmpty
              ? (container.networks[entry.key.toString()] ?? '')
              : text((entry.value as Map?)?['IPAddress']),
    };
    final labels = <String, String>{
      for (final entry in (config['Labels'] as Map? ?? const {}).entries) entry.key.toString(): text(entry.value),
    };
    final env = [for (final value in config['Env'] as List? ?? const []) text(value)];
    final ports = formatDockerPorts(container.ports);
    final health = dockerHealth(l, container.health);
    final info = <(String, String)>[
      (l.dockerImage, text(config['Image']).isEmpty ? container.image : text(config['Image'])),
      (l.dockerStatus, container.status.isEmpty ? dockerStateLabel(l, container.state) : container.status),
      if (health != null) (l.dockerHealth, health.$1),
      if (created != null) (l.dockerCreated, formatDateTime(l, created)),
      if (command.isNotEmpty) (l.dockerCommand, command),
      if (inspect['RestartCount'] is num) (l.dockerRestartCount, text(inspect['RestartCount'])),
      if (text(state['StartedAt']).isNotEmpty && container.state == DockerContainerState.running)
        if (DateTime.tryParse(text(state['StartedAt'])) case final started?)
          (l.dockerStarted, formatDateTime(l, started.toLocal())),
      if (container.composeProject != null) (l.dockerComposeProject, container.composeProject!),
      if (container.composeService != null) (l.dockerComposeService, container.composeService!),
      (l.dockerContainerId, container.id),
    ];
    Widget mono(String value, {Color? color}) => SelectableText(
      value,
      style: tokens.typography.mono.copyWith(fontSize: 12, color: color ?? tokens.palette.label),
    );
    return ListView(
      key: const ValueKey('docker-overview'),
      padding: const EdgeInsets.all(GlassSpacing.s16),
      children: [
        _Section(
          title: l.dockerDetails,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [for (final (label, value) in info) _Fact(label: label, value: value)],
          ),
        ),
        if (ports.isNotEmpty)
          _Section(
            title: l.dockerPorts,
            child: Column(
              key: const ValueKey('docker-ports'),
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final port in ports) Padding(padding: const EdgeInsets.symmetric(vertical: 2), child: mono(port)),
              ],
            ),
          ),
        if (networks.isNotEmpty)
          _Section(
            title: l.dockerNetworks,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final entry in networks.entries)
                  _Fact(label: entry.key, value: entry.value.isEmpty ? '—' : entry.value, mono: true),
              ],
            ),
          ),
        if (mounts.isNotEmpty)
          _Section(
            title: l.dockerMounts,
            child: _MountsTable(mounts: mounts, text: text, session: session),
          ),
        if (labels.isNotEmpty)
          _Section(
            title: l.dockerLabels,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [for (final entry in labels.entries) _Fact(label: entry.key, value: entry.value, mono: true)],
            ),
          ),
        if (env.isNotEmpty)
          _EnvironmentSection(
            key: ValueKey('docker-env-section-${container.id}'),
            env: env,
            service: service,
            session: session,
            container: container,
          ),
      ],
    );
  }
}

/// Volumes and bind mounts: type, source (volume name or host path),
/// destination and read-only flag. Sources open the host folder (SFTP over
/// SSH, the file manager for local Docker).
class _MountsTable extends StatelessWidget {
  const _MountsTable({required this.mounts, required this.text, required this.session});
  final List<Map<Object?, Object?>> mounts;
  final String Function(Object?) text;
  final DockerSessionInfo session;

  /// Volume name or host path; volumes and binds link to the host folder.
  Widget _mountSource(Map<Object?, Object?> mount, TextStyle mono) {
    final type = text(mount['Type']);
    final source = text(mount['Source']);
    final name = text(mount['Name']);
    final volume = type == 'volume'; // l10n-ignore: mount type
    if (volume || type == 'bind') {
      return DockerHostPathLink(
        session: session,
        path: source,
        volume: volume,
        label: volume && name.isNotEmpty ? name : null,
        style: mono,
      );
    }
    return SelectableText(name.isNotEmpty ? name : source, style: mono);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final head = tokens.typography.caption.copyWith(color: tokens.secondaryLabel);
    final mono = tokens.typography.mono.copyWith(fontSize: 12, color: tokens.palette.label);
    TableRow row(List<Widget> cells) => TableRow(
      children: [
        for (final cell in cells) Padding(padding: const EdgeInsets.symmetric(vertical: 4, horizontal: 6), child: cell),
      ],
    );
    return DecoratedBox(
      key: const ValueKey('docker-mounts'),
      decoration: ShapeDecoration(
        color: tokens.surfaces.fillField,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(tokens.radii.card),
          side: BorderSide(color: tokens.surfaces.hairlineCard),
        ),
      ),
      child: Padding(
        padding: const EdgeInsets.all(GlassSpacing.s6),
        child: Table(
          columnWidths: const {
            0: IntrinsicColumnWidth(),
            1: FlexColumnWidth(),
            2: FlexColumnWidth(),
            3: IntrinsicColumnWidth(),
          },
          defaultVerticalAlignment: TableCellVerticalAlignment.middle,
          children: [
            row([
              Text(l.dockerMountType, style: head),
              Text(l.dockerMountSource, style: head),
              Text(l.dockerMountDestination, style: head),
              Text(l.dockerMountMode, style: head),
            ]),
            for (final mount in mounts)
              row([
                Text(text(mount['Type']), style: mono.copyWith(color: tokens.secondaryLabel)),
                _mountSource(mount, mono),
                SelectableText(text(mount['Destination']), style: mono),
                Text(mount['RW'] == false ? l.dockerMountReadOnly : l.dockerMountReadWrite, style: mono),
              ]),
          ],
        ),
      ),
    );
  }
}

/// Environment, with secret values hidden until the vault passphrase is
/// entered; revealed values can be copied one by one (auto-cleared).
class _EnvironmentSection extends ConsumerStatefulWidget {
  const _EnvironmentSection({
    required this.env,
    required this.service,
    required this.session,
    required this.container,
    super.key,
  });
  final List<String> env;
  final DockerService service;
  final DockerSessionInfo session;
  final DockerContainer container;
  @override
  ConsumerState<_EnvironmentSection> createState() => _EnvironmentSectionState();
}

class _EnvironmentSectionState extends ConsumerState<_EnvironmentSection> {
  List<String>? _revealed;
  late final _scope = ref.read(dockerScopeProvider);
  bool get _current => mounted && identical(_scope, ref.read(dockerScopeProvider));

  bool get _hasSecrets => widget.env.any((entry) => entry.contains(dockerSecretMask));

  Future<void> _reveal() async {
    final revealed = await showAppDialog<List<String>>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (context) => _RevealDialog(
        dialogKey: 'docker-reveal-dialog',
        title: context.l10n.dockerRevealSecrets,
        help: context.l10n.dockerRevealHelp(widget.container.name),
        reveal: (passphrase) => widget.service.revealEnvironment(widget.session.id, widget.container.id, passphrase),
      ),
    );
    if (revealed != null && _current) setState(() => _revealed = revealed);
  }

  @override
  void dispose() {
    _revealed = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dockerScopeProvider, (_, next) {
      if (!identical(next, _scope) && mounted) setState(() => _revealed = null);
    });
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final entries = _revealed ?? widget.env;
    final mono = tokens.typography.mono.copyWith(fontSize: 12, color: tokens.palette.label);
    return _Section(
      title: l.dockerEnvironment,
      trailing: _revealed != null
          ? GlassButton.plain(
              key: const ValueKey('docker-env-hide'),
              label: l.dockerHideSecrets,
              icon: Icons.visibility_off_outlined,
              onPressed: () => setState(() => _revealed = null),
            )
          : _hasSecrets
          ? GlassButton.plain(
              key: const ValueKey('docker-env-reveal'),
              label: l.dockerRevealSecrets,
              icon: Icons.visibility_outlined,
              onPressed: () => unawaited(_reveal()),
            )
          : null,
      child: RevealedSecret(
        visible: _revealed != null,
        child: Column(
          key: const ValueKey('docker-env'),
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (final entry in entries)
              Row(
                children: [
                  Expanded(
                    child: Padding(
                      padding: const EdgeInsets.symmetric(vertical: 2),
                      child: SelectableText(entry, style: mono),
                    ),
                  ),
                  if (_revealed != null && entry.contains('='))
                    GlassIconButton(
                      icon: Icons.copy_rounded,
                      tooltip: l.commonCopy,
                      size: 24,
                      iconSize: 14,
                      style: GlassIconButtonStyle.plain,
                      onPressed: () => unawaited(
                        copySecretWithNotice(
                          context,
                          ref,
                          entry.substring(entry.indexOf('=') + 1),
                          what: entry.split('=').first,
                        ),
                      ),
                    ),
                ],
              ),
          ],
        ),
      ),
    );
  }
}

/// Asks for the vault passphrase and returns what [reveal] produced with it
/// (null on cancel). The passphrase is wiped after the attempt.
class _RevealDialog<T> extends ConsumerStatefulWidget {
  const _RevealDialog({required this.dialogKey, required this.title, required this.help, required this.reveal});
  final String dialogKey, title, help;
  final Future<T> Function(SecretText passphrase) reveal;
  @override
  ConsumerState<_RevealDialog<T>> createState() => _RevealDialogState<T>();
}

class _RevealDialogState<T> extends ConsumerState<_RevealDialog<T>> {
  final _passphrase = TextEditingController();
  bool _busy = false, _wrong = false, _failed = false;

  @override
  void dispose() {
    _passphrase
      ..clear()
      ..dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_busy || _passphrase.text.isEmpty) return;
    setState(() {
      _busy = true;
      _wrong = false;
      _failed = false;
    });
    final secret = SecretText(_passphrase.text);
    _passphrase.clear();
    try {
      final revealed = await widget.reveal(secret);
      if (mounted) closeDialog(context, revealed);
    } on DockerWrongPassphrase {
      if (mounted) setState(() => _wrong = true);
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      secret.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return GlassDialog(
      key: ValueKey(widget.dialogKey),
      title: widget.title,
      content: SizedBox(
        width: 420,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(widget.help),
            const SizedBox(height: GlassSpacing.s12),
            SecretField(
              key: const ValueKey('docker-reveal-passphrase'),
              controller: _passphrase,
              label: l.dockerVaultPassphrase,
              autofocus: true,
              enabled: !_busy,
              onSubmitted: (_) => unawaited(_submit()),
            ),
            if (_wrong) Text(l.dockerWrongPassphrase, style: TextStyle(color: Theme.of(context).colorScheme.error)),
            if (_failed) Text(l.dockerRequestFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
      ],
      primaryAction: GlassButton(
        key: const ValueKey('docker-reveal-submit'),
        label: widget.title,
        busy: _busy,
        onPressed: _busy ? null : () => unawaited(_submit()),
      ),
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.child, this.trailing});
  final String title;
  final Widget child;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(title, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
              ),
              ?trailing,
            ],
          ),
          const SizedBox(height: GlassSpacing.s6),
          child,
        ],
      ),
    );
  }
}

class _Fact extends StatelessWidget {
  const _Fact({required this.label, required this.value, this.mono = false});
  final String label, value;
  final bool mono;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final style = (mono ? tokens.typography.mono.copyWith(fontSize: 12) : tokens.typography.callout).copyWith(
      color: tokens.palette.label,
    );
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final labelText = Text(label, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel));
          if (constraints.maxWidth < 360) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                labelText,
                SelectableText(value, style: style),
              ],
            );
          }
          return Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(width: 140, child: labelText),
              const SizedBox(width: 8),
              Expanded(child: SelectableText(value, style: style)),
            ],
          );
        },
      ),
    );
  }
}
