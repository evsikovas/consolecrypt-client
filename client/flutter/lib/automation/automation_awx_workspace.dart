import 'dart:convert';

import 'package:consolecrypt/automation/automation_awx_controller.dart';
import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart'
    show MonitoringSource;
import 'package:material_ui/material_ui.dart';

enum AwxCopy {
  title,
  localSettings,
  refreshConnections,
  addConnection,
  editConnection,
  deleteConnection,
  deleteTitle,
  deleteHint,
  cancel,
  confirmDelete,
  connections,
  noConnections,
  readOnly,
  templates,
  search,
  readTemplates,
  previous,
  next,
  noTemplates,
  parameters,
  jobType,
  inherit,
  apply,
  check,
  emptyUsesDefault,
  limit,
  jobTags,
  skipTags,
  extraVars,
  invalidParameters,
  prepare,
  exactReview,
  reviewConnection,
  reviewHint,
  expiresUtc,
  launch,
  uncertain,
  job,
  refreshJob,
  events,
  readEvents,
  originalResource,
  ignoredParameters,
  eventTruncated,
  status,
  jobNew,
  jobPending,
  jobWaiting,
  jobRunning,
  jobSuccessful,
  jobFailed,
  jobError,
  jobCanceled,
  unavailable,
  configuration,
  invalidInput,
  changed,
  canceled,
  expired,
  transport,
  response,
  budget,
  forbidden,
  unauthorized,
  needsAwxInput,
  unsupportedPrompts,
  unknown,
  retired,
  cancelRead,
}

typedef AwxCopyText = String Function(AwxCopy key);
const _statusCopy = <AwxJobStatus, AwxCopy>{
  AwxJobStatus.newJob: AwxCopy.jobNew,
  AwxJobStatus.pending: AwxCopy.jobPending,
  AwxJobStatus.waiting: AwxCopy.jobWaiting,
  AwxJobStatus.running: AwxCopy.jobRunning,
  AwxJobStatus.successful: AwxCopy.jobSuccessful,
  AwxJobStatus.failed: AwxCopy.jobFailed,
  AwxJobStatus.error: AwxCopy.jobError,
  AwxJobStatus.canceled: AwxCopy.jobCanceled,
};

/// Host supplies the existing secure credential editor/metadata-backed connection
/// form and external-URL policy. This view cannot resolve credentials or HTTP.
class AwxWorkspace extends StatefulWidget {
  const AwxWorkspace({
    super.key,
    required this.controller,
    required this.copy,
    required this.onEditConnection,
    required this.onOpenResource,
  });
  final AwxController controller;
  final AwxCopyText copy;
  final Future<MonitoringSource?> Function(AwxConnection? original)
  onEditConnection;
  final Future<void> Function(String originalNativeUrl) onOpenResource;
  @override
  State<AwxWorkspace> createState() => _AwxWorkspaceState();
}

class _AwxWorkspaceState extends State<AwxWorkspace> {
  final _search = TextEditingController(),
      _limit = TextEditingController(),
      _tags = TextEditingController(),
      _skip = TextEditingController(),
      _vars = TextEditingController();
  String? _jobType;
  AwxTemplate? _formTemplate;
  AwxLaunchParameters? _formParameters;
  int _editorEpoch = 0;
  AwxController get c => widget.controller;
  String t(AwxCopy key) => widget.copy(key);
  @override
  void initState() {
    super.initState();
    _loadForm();
  }

  @override
  void didUpdateWidget(covariant AwxWorkspace oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.controller, c)) {
      _editorEpoch++;
      _loadForm();
    }
  }

  void _loadForm() {
    final values = c.parameters.toJson();
    _jobType = values['job_type'] as String?;
    _limit.text = values['limit'] as String? ?? '';
    _tags.text = values['job_tags'] as String? ?? '';
    _skip.text = values['skip_tags'] as String? ?? '';
    _vars.text = values.containsKey('extra_vars')
        ? jsonEncode(values['extra_vars'])
        : '';
    _formTemplate = c.template;
    _formParameters = c.parameters;
  }

  void _editParameters() {
    c.invalidateParameterDraft();
    try {
      final text = _vars.text.trim();
      final json = text.isEmpty ? null : parseAwxExtraVars(text);

      final parameters = AwxLaunchParameters(
        jobType: _jobType,
        limit: _limit.text.isEmpty ? null : _limit.text,
        jobTags: _tags.text.isEmpty ? null : _tags.text,
        skipTags: _skip.text.isEmpty ? null : _skip.text,
        extraVars: json,
      );
      _formParameters = parameters;
      c.editParameters(parameters);
    } catch (_) {
      /* Invalid draft has already discarded review; no old-payload fallback. */
    }
  }

  Future<void> _editConnection(AwxConnection? original) async {
    if (!c.current || c.busy) return;
    final owner = c, epoch = ++_editorEpoch, selected = owner.selected;
    final config = await widget.onEditConnection(original);
    if (!mounted ||
        !identical(c, owner) ||
        !owner.current ||
        epoch != _editorEpoch ||
        !identical(owner.selected, selected) ||
        config == null) {
      return;
    }
    await owner.saveConnection(config, expectedRevision: original?.revision);
  }

  Future<void> _deleteConnection(AwxConnection original) async {
    final owner = c, epoch = ++_editorEpoch;
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        key: const ValueKey('awx-delete-confirm'),
        title: Text(t(AwxCopy.deleteTitle)),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(original.configuration?.name ?? original.id),
            const SizedBox(height: 8),
            Text(t(AwxCopy.deleteHint)),
          ],
        ),
        actions: [
          GlassButton(
            label: t(AwxCopy.cancel),
            onPressed: () => Navigator.of(context).pop(false),
          ),
          GlassButton.destructive(
            label: t(AwxCopy.confirmDelete),
            onPressed: () => Navigator.of(context).pop(true),
          ),
        ],
      ),
    );
    if (yes != true ||
        !mounted ||
        !identical(c, owner) ||
        !owner.current ||
        epoch != _editorEpoch ||
        !identical(owner.selected, original)) {
      return;
    }
    await owner.deleteConnection(original);
  }

  Widget _card(AwxCopy title, Widget child) => ContentSurface(
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(t(title), style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 12),
        child,
      ],
    ),
  );
  Widget _field(
    TextEditingController controller,
    AwxCopy label, {
    int lines = 1,
  }) => Padding(
    padding: const EdgeInsets.only(bottom: 10),
    child: TextField(
      controller: controller,
      enabled: !c.busy && c.current,
      minLines: lines,
      maxLines: lines == 1 ? 1 : 6,
      onChanged: (_) => _editParameters(),
      decoration: InputDecoration(
        labelText: t(label),
        border: const OutlineInputBorder(),
      ),
    ),
  );
  Widget _connections() => _card(
    AwxCopy.connections,
    Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(t(AwxCopy.localSettings)),
        const SizedBox(height: 12),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            GlassButton(
              key: const ValueKey('awx-catalog-refresh'),
              label: t(AwxCopy.refreshConnections),
              onPressed: c.current && !c.busy ? c.loadCatalog : null,
              icon: Icons.refresh_rounded,
            ),
            GlassButton(
              key: const ValueKey('awx-connection-add'),
              label: t(AwxCopy.addConnection),
              onPressed: c.current && !c.busy && c.catalog?.readOnly == false
                  ? () => _editConnection(null)
                  : null,
              icon: Icons.add_rounded,
            ),
          ],
        ),
        const SizedBox(height: 12),
        if (c.catalog?.readOnly == true)
          InfoBanner(message: t(AwxCopy.readOnly), tone: BannerTone.warning),
        if (c.catalog?.connections.isEmpty ?? true)
          Text(t(AwxCopy.noConnections)),
        for (final connection in c.catalog?.connections ?? <AwxConnection>[])
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: ContentSurface(
              kind: identical(c.selected, connection)
                  ? ContentSurfaceKind.inset
                  : ContentSurfaceKind.content,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Semantics(
                    selected: identical(c.selected, connection),
                    child: GlassButton(
                      key: ValueKey('awx-connection-${connection.id}'),
                      label: connection.configuration?.name ?? connection.id,
                      style: identical(c.selected, connection)
                          ? GlassButtonStyle.prominent
                          : GlassButtonStyle.glass,
                      onPressed: c.current && !c.busy
                          ? () {
                              _editorEpoch++;
                              c.selectConnection(connection);
                              _loadForm();
                            }
                          : null,
                    ),
                  ),
                  if (identical(c.selected, connection))
                    Wrap(
                      spacing: 8,
                      runSpacing: 8,
                      children: [
                        GlassButton(
                          label: t(AwxCopy.editConnection),
                          onPressed: !c.busy && !connection.readOnly
                              ? () => _editConnection(connection)
                              : null,
                        ),
                        GlassButton(
                          label: t(AwxCopy.deleteConnection),
                          style: GlassButtonStyle.destructiveQuiet,
                          onPressed: !c.busy && !connection.readOnly
                              ? () => _deleteConnection(connection)
                              : null,
                        ),
                      ],
                    ),
                ],
              ),
            ),
          ),
      ],
    ),
  );
  Widget _templates() => _card(
    AwxCopy.templates,
    Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        TextField(
          key: const ValueKey('awx-template-search'),
          controller: _search,
          maxLength: 256,
          enabled: c.current && !c.busy,
          decoration: InputDecoration(
            labelText: t(AwxCopy.search),
            border: const OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            GlassButton(
              key: const ValueKey('awx-read-templates'),
              label: t(AwxCopy.readTemplates),
              onPressed: c.current && !c.busy && c.selected?.readOnly == false
                  ? () => c.loadTemplates(search: _search.text)
                  : null,
            ),
            GlassButton(
              label: t(AwxCopy.previous),
              onPressed: !c.busy && (c.templates?.page ?? 1) > 1
                  ? () => c.loadTemplates(
                      page: c.templates!.page - 1,
                      search: _search.text,
                    )
                  : null,
            ),
            GlassButton(
              label: t(AwxCopy.next),
              onPressed: !c.busy && c.templates?.hasNext == true
                  ? () => c.loadTemplates(
                      page: c.templates!.page + 1,
                      search: _search.text,
                    )
                  : null,
            ),
          ],
        ),
        const SizedBox(height: 8),
        if (c.templates?.templates.isEmpty ?? true)
          Text(t(AwxCopy.noTemplates)),
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 260),
          child: ListView(
            shrinkWrap: true,
            children: [
              for (final template in c.templates?.templates ?? <AwxTemplate>[])
                Padding(
                  padding: const EdgeInsets.only(bottom: 6),
                  child: GlassButton(
                    key: ValueKey('awx-template-${template.id}'),
                    label: '${template.name} · #${template.id}',
                    style: identical(c.template, template)
                        ? GlassButtonStyle.prominent
                        : GlassButtonStyle.glass,
                    onPressed: !c.busy
                        ? () {
                            c.selectTemplate(template);
                            _loadForm();
                            _formTemplate = template;
                          }
                        : null,
                  ),
                ),
            ],
          ),
        ),
      ],
    ),
  );
  Widget _parameters() => _card(
    AwxCopy.parameters,
    Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        DropdownButtonFormField<String>(
          key: const ValueKey('awx-job-type'),
          initialValue: _jobType ?? 'inherit',
          isExpanded: true,
          decoration: InputDecoration(labelText: t(AwxCopy.jobType)),
          items: [
            DropdownMenuItem(value: 'inherit', child: Text(t(AwxCopy.inherit))),
            DropdownMenuItem(value: 'run', child: Text(t(AwxCopy.apply))),
            DropdownMenuItem(value: 'check', child: Text(t(AwxCopy.check))),
          ],
          onChanged: !c.busy && c.template != null
              ? (v) {
                  _jobType = v == 'inherit' ? null : v;
                  _editParameters();
                }
              : null,
        ),
        const SizedBox(height: 10),
        Text(t(AwxCopy.emptyUsesDefault)),
        const SizedBox(height: 10),
        _field(_limit, AwxCopy.limit),
        _field(_tags, AwxCopy.jobTags),
        _field(_skip, AwxCopy.skipTags),
        _field(_vars, AwxCopy.extraVars, lines: 3),
        if (!c.parametersReady)
          InfoBanner(
            key: const ValueKey('awx-invalid-parameters'),
            message: t(AwxCopy.invalidParameters),
            tone: BannerTone.danger,
          ),
        const SizedBox(height: 8),
        GlassButton(
          key: const ValueKey('awx-prepare'),
          label: t(AwxCopy.prepare),
          onPressed: c.canPrepare ? c.prepareLaunch : null,
        ),
      ],
    ),
  );
  Widget _review() {
    final review = c.review;
    if (review == null) return const SizedBox.shrink();
    return _card(
      AwxCopy.exactReview,
      Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(t(AwxCopy.reviewHint)),
          const SizedBox(height: 10),
          Text(
            '${t(AwxCopy.reviewConnection)}: ${c.selected?.configuration?.name ?? review.connectionId}',
          ),
          Text(c.selected?.configuration?.endpoint ?? ''),
          const SizedBox(height: 8),
          Text('${review.template.name} · #${review.template.id}'),
          Text(
            '${t(AwxCopy.expiresUtc)}: ${review.expiresUtc.toIso8601String()}',
          ),
          const SizedBox(height: 10),
          ContentSurface(
            kind: ContentSurfaceKind.inset,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 220),
              child: SingleChildScrollView(
                child: SelectableText(
                  review.parameters.canonicalJson,
                  style: const TextStyle(fontFamily: 'monospace'),
                ),
              ),
            ),
          ),
          const SizedBox(height: 12),
          GlassButton.prominent(
            key: const ValueKey('awx-launch'),
            label: t(AwxCopy.launch),
            icon: Icons.play_arrow_rounded,
            onPressed: c.canLaunch ? c.launchReviewed : null,
          ),
        ],
      ),
    );
  }

  Widget _job() {
    final job = c.job, outcome = c.outcome;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (c.launchUncertain)
          InfoBanner(
            key: const ValueKey('awx-unknown-outcome'),
            message: t(AwxCopy.uncertain),
            tone: BannerTone.warning,
          ),
        if (outcome is AwxLaunchStarted && outcome.parametersIgnored)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: InfoBanner(
              message: t(AwxCopy.ignoredParameters),
              tone: BannerTone.warning,
            ),
          ),
        if (job != null)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: _card(
              AwxCopy.job,
              Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(job.name),
                  Text('${t(AwxCopy.status)}: ${t(_statusCopy[job.status]!)}'),
                  const SizedBox(height: 10),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      GlassButton(
                        label: t(AwxCopy.refreshJob),
                        onPressed: !c.busy && c.current ? c.refreshJob : null,
                      ),
                      GlassButton(
                        label: t(AwxCopy.readEvents),
                        onPressed: !c.busy && c.current ? c.loadEvents : null,
                      ),
                      if (outcome is AwxLaunchStarted)
                        GlassButton(
                          label: t(AwxCopy.originalResource),
                          icon: Icons.open_in_new_rounded,
                          onPressed: c.current
                              ? () => widget.onOpenResource(
                                  outcome.originalJobUrl,
                                )
                              : null,
                        ),
                    ],
                  ),
                  if (c.jobEvents.isNotEmpty) ...[
                    const SizedBox(height: 12),
                    Text(t(AwxCopy.events)),
                    ContentSurface(
                      kind: ContentSurfaceKind.inset,
                      child: ConstrainedBox(
                        constraints: const BoxConstraints(maxHeight: 300),
                        child: ListView(
                          shrinkWrap: true,
                          children: [
                            for (final event in c.jobEvents)
                              Padding(
                                key: ValueKey('awx-event-${event.counter}'),
                                padding: const EdgeInsets.only(bottom: 8),
                                child: Column(
                                  crossAxisAlignment:
                                      CrossAxisAlignment.stretch,
                                  children: [
                                    Text(
                                      '${event.counter} · ${event.event}',
                                      style: Theme.of(context)
                                          .textTheme
                                          .labelLarge,
                                    ),
                                    SelectableText(
                                      event.stdout,
                                      style: const TextStyle(
                                        fontFamily: 'monospace',
                                      ),
                                    ),
                                    if (event.truncated)
                                      Text(t(AwxCopy.eventTruncated)),
                                  ],
                                ),
                              ),
                          ],
                        ),
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: c,
    builder: (context, _) {
      if (!identical(_formTemplate, c.template) ||
          (c.parametersReady && !identical(_formParameters, c.parameters))) {
        _loadForm();
      }
      return LayoutBuilder(
        builder: (context, box) {
          final left = Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _connections(),
              const SizedBox(height: 12),
              _templates(),
            ],
          );
          final right = Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _parameters(),
              const SizedBox(height: 12),
              _review(),
              _job(),
            ],
          );
          return SingleChildScrollView(
            key: const PageStorageKey('awx-workspace-scroll'),
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  t(AwxCopy.title),
                  style: Theme.of(context).textTheme.titleLarge,
                ),
                const SizedBox(height: 12),
                if (!c.current)
                  InfoBanner(
                    message: t(AwxCopy.retired),
                    tone: BannerTone.warning,
                  ),
                if (c.failure != null)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: InfoBanner(
                      key: const ValueKey('awx-failure'),
                      message: t(AwxCopy.values.byName(c.failure!.name)),
                      tone: BannerTone.danger,
                    ),
                  ),
                if (c.busy)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: Row(
                      children: [
                        const Expanded(child: LinearProgressIndicator()),
                        const SizedBox(width: 12),
                        GlassButton(
                          label: t(AwxCopy.cancelRead),
                          onPressed: c.cancelRead,
                        ),
                      ],
                    ),
                  ),
                if (box.maxWidth >= 760)
                  Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Expanded(child: left),
                      const SizedBox(width: 12),
                      Expanded(child: right),
                    ],
                  )
                else ...[
                  left,
                  const SizedBox(height: 12),
                  right,
                ],
              ],
            ),
          );
        },
      );
    },
  );
  @override
  void dispose() {
    _editorEpoch++;
    for (final x in [_search, _limit, _tags, _skip, _vars]) {
      x.dispose();
    }
    super.dispose();
  }
}
