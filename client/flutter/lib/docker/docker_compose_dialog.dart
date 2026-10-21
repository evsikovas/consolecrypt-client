import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/docker/docker_compose_yaml.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// The compose files of [project], read on the engine host: read-only with
/// line numbers, editable after the vault passphrase. Saving validates on
/// the host first, keeps a backup and (optionally) runs `up -d`, whose
/// output is shown here. Contents are secrets and are wiped on close.
Future<void> showDockerComposeDialog(
  BuildContext context, {
  required DockerService service,
  required DockerSessionInfo session,
  required String project,
  bool edit = false,
}) => showAppDialog<void>(
  context,
  secure: true,
  barrierDismissible: false,
  builder: (_) => DockerComposeDialog(service: service, session: session, project: project, edit: edit),
);

enum _Mode { view, unlock, edit }

class DockerComposeDialog extends ConsumerStatefulWidget {
  const DockerComposeDialog({
    required this.service,
    required this.session,
    required this.project,
    this.edit = false,
    super.key,
  });
  final DockerService service;
  final DockerSessionInfo session;
  final String project;

  /// Start at the passphrase step (the "Edit YAML" menu item).
  final bool edit;
  @override
  ConsumerState<DockerComposeDialog> createState() => _DockerComposeDialogState();
}

class _DockerComposeDialogState extends ConsumerState<DockerComposeDialog> {
  late final _scope = ref.read(dockerScopeProvider);
  late _Mode _mode = widget.edit ? _Mode.unlock : _Mode.view;
  DockerComposeProject? _project;
  List<String?> _texts = const [];
  List<DockerYamlController?> _editors = const [];
  Object? _loadError;
  bool _loading = true;
  int _index = 0;

  final _passphrase = TextEditingController();
  String? _grant;
  bool _unlocking = false, _wrong = false, _unlockFailed = false, _expired = false;

  /// A save waiting for a fresh grant (`true`: then apply).
  bool? _pendingSave;
  bool _saving = false;
  DockerComposeSaveResult? _result;
  bool _requestFailed = false;
  final _output = <String>[];
  StreamSubscription<DockerComposeJobUpdate>? _job;
  DockerComposeJobUpdate? _jobEnd;
  bool _jobFailed = false;

  bool get _applying => _job != null && _jobEnd == null && !_jobFailed;
  bool get _dirty => [
    for (final (i, editor) in _editors.indexed)
      if (editor != null && i < _texts.length && editor.text != _texts[i]) i,
  ].isNotEmpty;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  @override
  void dispose() {
    unawaited(_job?.cancel());
    _project?.wipe();
    _project = null;
    _texts = const [];
    _disposeEditors();
    _passphrase
      ..clear()
      ..dispose();
    super.dispose();
  }

  void _disposeEditors() {
    for (final editor in _editors) {
      editor
        ?..removeListener(_edited)
        ..clear()
        ..dispose();
    }
    _editors = const [];
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _loadError = null;
    });
    try {
      final project = await widget.service.composeProject(widget.session.id, widget.project);
      if (!mounted) {
        project.wipe();
        return;
      }
      _project?.wipe();
      setState(() {
        _project = project;
        _texts = [for (final file in project.files) file.content?.expose()];
        if (_index >= project.files.length) _index = 0;
        // Prefer a readable file.
        if (_texts[_index] == null) _index = _texts.indexWhere((text) => text != null).clamp(0, _texts.length - 1);
      });
    } catch (error) {
      if (mounted) setState(() => _loadError = error);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  void _startEditing() {
    _disposeEditors();
    setState(() {
      _editors = [
        for (final text in _texts) text == null ? null : (DockerYamlController(text: text)..addListener(_edited)),
      ];
      _wasDirty = false;
      _mode = _Mode.edit;
      _result = null;
      _requestFailed = false;
    });
  }

  /// Rebuilds the actions when the first change appears or the last goes.
  bool _wasDirty = false;
  void _edited() {
    final dirty = _dirty;
    if (dirty != _wasDirty && mounted) setState(() => _wasDirty = dirty);
  }

  void _edit() {
    if (_grant != null) {
      _startEditing();
    } else {
      setState(() {
        _mode = _Mode.unlock;
        _expired = false;
      });
    }
  }

  Future<void> _unlock() async {
    if (_unlocking || _passphrase.text.isEmpty) return;
    setState(() {
      _unlocking = true;
      _wrong = false;
      _unlockFailed = false;
    });
    final secret = SecretText(_passphrase.text);
    _passphrase.clear();
    try {
      final grant = await widget.service.composeUnlock(widget.session.id, widget.project, secret);
      if (!mounted) return;
      _grant = grant;
      final pending = _pendingSave;
      if (pending != null && _editors.isNotEmpty) {
        // The grant expired mid-edit: keep the texts and retry the save.
        setState(() {
          _mode = _Mode.edit;
          _pendingSave = null;
          _expired = false;
        });
        await _save(apply: pending);
      } else {
        _startEditing();
      }
    } on DockerWrongPassphrase {
      if (mounted) setState(() => _wrong = true);
    } catch (_) {
      if (mounted) setState(() => _unlockFailed = true);
    } finally {
      secret.wipe();
      if (mounted) setState(() => _unlocking = false);
    }
  }

  Future<void> _save({required bool apply}) async {
    final grant = _grant;
    final project = _project;
    if (_saving || grant == null || project == null || !_dirty) return;
    final edits = [
      for (final (i, editor) in _editors.indexed)
        if (editor != null && editor.text != _texts[i] && project.files[i].digest != null)
          DockerComposeEdit(
            path: project.files[i].path,
            content: SecretText(editor.text),
            digest: project.files[i].digest!,
          ),
    ];
    setState(() {
      _saving = true;
      _result = null;
      _requestFailed = false;
      _output.clear();
      _jobEnd = null;
      _jobFailed = false;
    });
    try {
      final result = await widget.service.composeSave(widget.session.id, widget.project, grant, edits, apply: apply);
      if (!mounted) return;
      switch (result) {
        case DockerComposeLocked():
          setState(() {
            _grant = null;
            _pendingSave = apply;
            _expired = true;
            _mode = _Mode.unlock;
          });
        case DockerComposeSaved(:final jobId):
          setState(() {
            _result = result;
            _mode = _Mode.view;
          });
          _disposeEditors();
          await _load();
          if (jobId != null && mounted) _follow(jobId);
        default:
          setState(() => _result = result);
      }
    } catch (_) {
      if (mounted) setState(() => _requestFailed = true);
    } finally {
      for (final edit in edits) {
        if (!edit.content.isWiped) edit.content.wipe();
      }
      if (mounted) setState(() => _saving = false);
    }
  }

  void _follow(String jobId) {
    unawaited(_job?.cancel());
    _job = widget.service
        .composeJob(jobId)
        .listen(
          (update) {
            if (!mounted) return;
            setState(() {
              _output.addAll(update.lines);
              if (_output.length > 2000) _output.removeRange(0, _output.length - 2000);
              if (update.done) _jobEnd = update;
            });
          },
          onError: (Object _) {
            if (mounted) setState(() => _jobFailed = true);
          },
        );
    setState(() {});
  }

  Future<bool> _confirmDiscard() async {
    if (!_dirty) return true;
    final l = context.l10n;
    return showConfirmDialog(
      context,
      title: l.dockerComposeDiscardTitle,
      message: l.dockerComposeDiscardHelp,
      confirmLabel: l.dockerComposeDiscard,
      destructive: true,
      secure: true,
    );
  }

  Future<void> _leaveEdit() async {
    if (!await _confirmDiscard() || !mounted) return;
    _disposeEditors();
    setState(() {
      _mode = _Mode.view;
      _result = null;
      _pendingSave = null;
    });
  }

  Future<void> _close() async {
    if (_mode == _Mode.edit && !await _confirmDiscard()) return;
    if (mounted) closeDialog<void>(context);
  }

  void _cancelUnlock() {
    _passphrase.clear();
    if (_project == null || (widget.edit && _editors.isEmpty && _result == null && _pendingSave == null)) {
      closeDialog<void>(context);
      return;
    }
    setState(() {
      _mode = _editors.isEmpty ? _Mode.view : _Mode.edit;
      _pendingSave = null;
      _expired = false;
      _wrong = false;
    });
  }

  String _fileError(AppLocalizations l, DockerComposeFileError error) => switch (error) {
    DockerComposeFileError.notFound => l.dockerComposeFileNotFound,
    DockerComposeFileError.permissionDenied => l.dockerComposeFileDenied,
    DockerComposeFileError.tooLarge => l.dockerComposeFileTooLarge,
    DockerComposeFileError.notText => l.dockerComposeFileNotText,
    DockerComposeFileError.notAFile || DockerComposeFileError.readFailed => l.dockerComposeFileReadFailed,
  };

  Widget _files(AppLocalizations l, DockerComposeProject project) {
    final files = project.files;
    if (files.length < 2) return const SizedBox.shrink();
    void select(int index) => setState(() => _index = index);
    final picker = files.length <= 5
        ? GlassSegmented<int>(
            key: const ValueKey('docker-compose-files'),
            inChrome: false,
            semanticLabel: l.dockerComposeFiles,
            selected: _index,
            onChanged: select,
            segments: [
              for (final (i, file) in files.indexed)
                GlassSegment(key: ValueKey('docker-compose-file-$i'), value: i, label: file.name),
            ],
          )
        : GlassSelect<int>(
            key: const ValueKey('docker-compose-files'),
            semanticLabel: l.dockerComposeFiles,
            value: _index,
            onChanged: select,
            items: [
              for (final (i, file) in files.indexed)
                GlassSelectItem(
                  key: ValueKey('docker-compose-file-$i'),
                  value: i,
                  label: file.name,
                  subtitle: file.path,
                ),
            ],
          );
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
      child: Align(
        alignment: Alignment.centerLeft,
        child: SingleChildScrollView(scrollDirection: Axis.horizontal, child: picker),
      ),
    );
  }

  Widget _code(AppLocalizations l, DockerComposeProject project) {
    final file = project.files[_index];
    final text = _index < _texts.length ? _texts[_index] : null;
    if (text == null) {
      return Align(
        alignment: Alignment.topCenter,
        child: InfoBanner(
          key: const ValueKey('docker-compose-file-error'),
          tone: BannerTone.warning,
          message: _fileError(l, file.error ?? DockerComposeFileError.readFailed),
        ),
      );
    }
    if (_mode == _Mode.edit && _index < _editors.length && _editors[_index] != null) {
      return DockerYamlEditor(
        key: ValueKey('docker-compose-editor-$_index'),
        controller: _editors[_index]!,
        readOnly: _saving,
      );
    }
    return DockerYamlView(key: ValueKey('docker-compose-view-$_index'), text: text);
  }

  Widget _unlockPanel(AppLocalizations l) {
    final tokens = GlassTokens.of(context);
    return Align(
      alignment: const Alignment(0, -.4),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 440),
        child: Column(
          key: const ValueKey('docker-compose-unlock-panel'),
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              _expired ? l.dockerComposeUnlockExpired : l.dockerComposeUnlockHelp(widget.project),
              style: tokens.typography.callout.copyWith(color: tokens.palette.label),
            ),
            const SizedBox(height: GlassSpacing.s12),
            SecretField(
              key: const ValueKey('docker-compose-passphrase'),
              controller: _passphrase,
              label: l.dockerVaultPassphrase,
              autofocus: true,
              enabled: !_unlocking,
              onSubmitted: (_) => unawaited(_unlock()),
            ),
            if (_wrong)
              Text(
                l.dockerWrongPassphrase,
                key: const ValueKey('docker-compose-wrong-passphrase'),
                style: tokens.typography.callout.copyWith(color: tokens.palette.danger),
              ),
            if (_unlockFailed)
              Text(l.dockerRequestFailed, style: tokens.typography.callout.copyWith(color: tokens.palette.danger)),
          ],
        ),
      ),
    );
  }

  Widget _outputBox(String text, {Color? color}) {
    final tokens = GlassTokens.of(context);
    return Container(
      key: const ValueKey('docker-compose-output'),
      constraints: const BoxConstraints(maxHeight: 150),
      margin: const EdgeInsets.only(top: GlassSpacing.s6),
      decoration: ShapeDecoration(
        color: tokens.surfaces.fillField,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(tokens.radii.card),
          side: BorderSide(color: tokens.surfaces.hairlineCard),
        ),
      ),
      child: SingleChildScrollView(
        reverse: true,
        padding: const EdgeInsets.all(GlassSpacing.s8),
        child: SizedBox(
          width: double.infinity,
          child: SelectableText(
            text,
            style: tokens.typography.mono.copyWith(fontSize: 11, color: color ?? tokens.palette.label),
          ),
        ),
      ),
    );
  }

  /// Validation, save and apply results below the code.
  Widget? _status(AppLocalizations l) {
    final tokens = GlassTokens.of(context);
    final danger = tokens.typography.callout.copyWith(color: tokens.palette.danger);
    final caption = tokens.typography.callout.copyWith(color: tokens.palette.label);
    Widget line(String text, TextStyle style, {Widget? leading}) => Row(
      children: [
        if (leading != null) ...[leading, const SizedBox(width: GlassSpacing.s8)],
        Expanded(child: Text(text, style: style)),
      ],
    );
    const spinner = SizedBox.square(dimension: 14, child: CircularProgressIndicator(strokeWidth: 2));
    if (_saving) return line(l.dockerComposeValidating, caption, leading: spinner);
    if (_requestFailed) return line(l.dockerRequestFailed, danger);
    final result = _result;
    switch (result) {
      case null:
        return null;
      case DockerComposeInvalid(:final output):
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            line(l.dockerComposeInvalid, danger),
            if (output.isNotEmpty) _outputBox(output, color: tokens.palette.danger),
          ],
        );
      case DockerComposeConflict(:final path):
        return line(l.dockerComposeConflict(path), danger);
      case DockerComposeWriteFailed(:final path, :final reason):
        return line(
          reason == DockerComposeWriteError.permissionDenied
              ? l.dockerComposeWriteDenied(path)
              : l.dockerComposeWriteFailed(path),
          danger,
        );
      case DockerComposeMissing():
        return line(l.dockerComposeMissing, danger);
      case DockerComposeRunFailed(:final detail):
        return line(l.dockerComposeRunFailed(detail), danger);
      case DockerComposeLocked():
        return null;
      case DockerComposeSaved(:final backups, :final jobId):
        final end = _jobEnd;
        final success = tokens.typography.callout.copyWith(color: tokens.palette.success);
        return Column(
          key: const ValueKey('docker-compose-result'),
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            line(l.dockerComposeSavedBackups(backups.length), success),
            for (final backup in backups)
              SelectableText(
                backup,
                style: tokens.typography.mono.copyWith(fontSize: 11, color: tokens.secondaryLabel),
              ),
            const SizedBox(height: GlassSpacing.s6),
            if (jobId == null)
              line(l.dockerComposeSavedNotApplied, caption)
            else if (_jobFailed || (end != null && end.error != null))
              line(l.dockerComposeApplyError, danger)
            else if (end == null)
              line(l.dockerComposeApplying, caption, leading: spinner)
            else if (end.success)
              line(l.dockerComposeApplied, success)
            else
              line(l.dockerComposeApplyFailed(end.exitCode ?? -1), danger),
            if (_output.isNotEmpty) _outputBox(_output.join('\n')),
          ],
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dockerScopeProvider, (_, next) {
      if (!identical(next, _scope) && mounted) closeDialog<void>(context);
    });
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final project = _project;
    final size = MediaQuery.sizeOf(context);
    final height = (size.height * .62).clamp(260.0, 640.0);
    final text = project != null && _index < _texts.length ? _texts[_index] : null;
    final status = _status(l);
    final facts = [
      if (project != null && project.workingDir.isNotEmpty) l.dockerComposeWorkingDir(project.workingDir),
      if (project != null && project.envFiles.isNotEmpty) l.dockerComposeEnvFiles(project.envFiles.join(', ')),
    ];
    final Widget body;
    if (_loading && project == null) {
      body = const Center(child: CircularProgressIndicator());
    } else if (project == null) {
      final error = _loadError;
      body = Align(
        alignment: Alignment.topCenter,
        child: InfoBanner(
          key: const ValueKey('docker-compose-load-error'),
          tone: BannerTone.danger,
          message: switch (error) {
            DockerComposeException(failure: DockerComposeFailure.projectGone) => l.dockerComposeProjectGone,
            DockerComposeException(failure: DockerComposeFailure.filesUnknown) => l.dockerComposeFilesUnknown,
            _ => l.dockerRequestFailed,
          },
          action: GlassButton(label: l.dockerRetry, onPressed: _load),
        ),
      );
    } else if (_mode == _Mode.unlock) {
      body = _unlockPanel(l);
    } else {
      body = Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _files(l, project),
          Expanded(child: _code(l, project)),
          if (status != null) ...[const SizedBox(height: GlassSpacing.s8), Flexible(flex: 0, child: status)],
        ],
      );
    }
    final canEdit = _texts.any((text) => text != null) && !_applying && !_saving;
    final List<Widget> secondary;
    final Widget? primary;
    switch (_mode) {
      case _Mode.view:
        secondary = [
          GlassButton(
            key: const ValueKey('docker-compose-close'),
            label: l.commonClose,
            onPressed: () => unawaited(_close()),
          ),
        ];
        primary = project == null
            ? null
            : GlassButton.prominent(
                key: const ValueKey('docker-compose-edit'),
                label: l.dockerComposeEditYaml,
                icon: Icons.edit_outlined,
                onPressed: canEdit ? _edit : null,
              );
      case _Mode.unlock:
        secondary = [
          GlassButton(
            key: const ValueKey('docker-compose-unlock-cancel'),
            label: l.commonCancel,
            onPressed: _unlocking ? null : _cancelUnlock,
          ),
        ];
        primary = project == null
            ? null
            : GlassButton.prominent(
                key: const ValueKey('docker-compose-unlock'),
                label: l.dockerComposeUnlock,
                busy: _unlocking,
                onPressed: _unlocking ? null : () => unawaited(_unlock()),
              );
      case _Mode.edit:
        final ready = _dirty && !_saving;
        secondary = [
          GlassButton(
            key: const ValueKey('docker-compose-cancel-edit'),
            label: l.commonCancel,
            onPressed: _saving ? null : () => unawaited(_leaveEdit()),
          ),
          GlassButton(
            key: const ValueKey('docker-compose-save-only'),
            label: l.dockerComposeSaveOnly,
            onPressed: ready ? () => unawaited(_save(apply: false)) : null,
          ),
        ];
        primary = GlassButton.prominent(
          key: const ValueKey('docker-compose-save-apply'),
          label: l.dockerComposeSaveApply,
          busy: _saving,
          onPressed: ready ? () => unawaited(_save(apply: true)) : null,
        );
    }
    return PopScope(
      canPop: !(_mode == _Mode.edit && _dirty),
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) unawaited(_close());
      },
      child: GlassDialog(
        key: const ValueKey('docker-compose-dialog'),
        title: l.dockerComposeFilesTitle(widget.project),
        width: 960,
        content: SizedBox(
          width: 960,
          height: height,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (facts.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
                  child: Text(
                    facts.join(' · '),
                    key: const ValueKey('docker-compose-facts'),
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                  ),
                ),
              Expanded(child: body),
            ],
          ),
        ),
        leadingAction: _mode == _Mode.view && text != null
            ? GlassButton.plain(
                key: const ValueKey('docker-compose-copy'),
                label: l.commonCopy,
                icon: Icons.content_copy_rounded,
                onPressed: () => unawaited(copySecretWithNotice(context, ref, text, what: project!.files[_index].name)),
              )
            : null,
        secondaryActions: secondary,
        primaryAction: primary,
      ),
    );
  }
}
