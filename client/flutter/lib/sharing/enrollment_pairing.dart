import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/enrollment_models.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

enum EnrollmentPairingFlow { prepare, endorse, submit, restore }

// Same UTF-8 byte bound as the core's signed public transcript parser.
const maxEnrollmentPacketBytes = 512 * 1024;

bool _boundedPublicJson(String value) =>
    value.isNotEmpty && utf8.encode(value).length <= maxEnrollmentPacketBytes && jsonDecode(value) is Map;

Future<void> showEnrollmentPairing(BuildContext context, EnrollmentPairingFlow flow) =>
    showAppDialog<void>(context, secure: true, builder: (_) => EnrollmentPairingDialog(flow));

Future<void> showEnrollmentPackage(BuildContext context, EnrollmentPairing pairing) =>
    showAppDialog<void>(context, secure: true, builder: (_) => EnrollmentPackageDialog(pairing));

Future<void> showEnrollmentPending(BuildContext context) =>
    showAppDialog<void>(context, secure: true, builder: (_) => const EnrollmentPendingDialog());

String enrollmentRequestLabel(BuildContext context, EnrollmentRequestState state) {
  final l = context.l10n;
  return switch (state) {
    EnrollmentRequestState.pending => l.enrollmentRequested,
    EnrollmentRequestState.challenged => l.enrollmentChallenged,
    EnrollmentRequestState.responded => l.enrollmentResponded,
    EnrollmentRequestState.accepted => l.enrollmentAccepted,
    EnrollmentRequestState.denied => l.enrollmentDisabled,
    EnrollmentRequestState.expired => l.enrollmentExpired,
    EnrollmentRequestState.blocked => l.enrollmentBlocked,
  };
}

class EnrollmentPairingDialog extends ConsumerStatefulWidget {
  const EnrollmentPairingDialog(this.flow, {super.key});
  final EnrollmentPairingFlow flow;
  @override
  ConsumerState<EnrollmentPairingDialog> createState() => _EnrollmentPairingDialogState();
}

class _EnrollmentPairingDialogState extends ConsumerState<EnrollmentPairingDialog> {
  late final Object? _scope;
  final _input = TextEditingController();
  String _lastInput = '';
  EnrollmentPairing? _preview;
  SharingRole _role = SharingRole.reader;

  /// TR-15: the code from the other device's screen, typed by the user.
  final _code = TextEditingController();
  bool _busy = false, _working = false;
  Object? _failure;
  bool get _failed => _failure != null;
  bool get _confirmed => _code.text.replaceAll(RegExp(r'\D'), '').length == 30;
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    _scope = ref.read(sharingSessionScopeProvider);
    _input.addListener(_changed);
  }

  void _changed() {
    if (_input.text == _lastInput) return;
    _lastInput = _input.text;
    _invalidate();
  }

  void _invalidate() {
    _generation++;
    setState(() {
      _preview = null;
      _code.clear();
      _failure = null;
      if (!_working) _busy = false;
    });
  }

  @override
  void dispose() {
    _input.removeListener(_changed);
    _input.clear();
    _input.dispose();
    _code.dispose();
    super.dispose();
  }

  Future<void> _open() async {
    if (_busy || !sharingSessionCurrent(ref, _scope)) return;
    final generation = ++_generation;
    setState(() => _busy = true);
    try {
      final path = await ref.read(fileDialogServiceProvider).chooseOpenFile(extensions: const ['json']);
      if (path == null || !mounted || !sharingSessionCurrent(ref, _scope) || generation != _generation) return;
      final file = await File(path).open();
      final String contents;
      try {
        // Bound reads even if the selected file grows after the picker returns.
        final bytes = await file.read(maxEnrollmentPacketBytes + 1);
        if (bytes.length > maxEnrollmentPacketBytes) throw const FormatException('Packet too large');
        contents = utf8.decode(bytes);
      } finally {
        await file.close();
      }
      if (!mounted || !sharingSessionCurrent(ref, _scope) || generation != _generation) return;
      if (!_boundedPublicJson(contents)) throw const FormatException('Invalid public packet');
      _input.text = contents;
    } catch (e) {
      if (mounted && sharingSessionCurrent(ref, _scope) && generation == _generation) {
        setState(() => _failure = e);
      }
    } finally {
      if (mounted && sharingSessionCurrent(ref, _scope) && generation == _generation) {
        setState(() => _busy = false);
      }
    }
  }

  Future<void> _review() async {
    if (_busy || !sharingSessionCurrent(ref, _scope)) return;
    final generation = ++_generation;
    final input = _input.text;
    setState(() {
      _busy = true;
      _preview = null;
      _code.clear();
      _failure = null;
    });
    try {
      if (!_boundedPublicJson(input)) throw const FormatException('Invalid public packet');
      final preview = await ref.read(enrollmentServiceProvider).inspectPairing(input);
      if (!mounted || !sharingSessionCurrent(ref, _scope) || generation != _generation) return;
      if (!_boundedPublicJson(preview.bundle) || !preview.expires.isAfter(DateTime.now())) {
        throw const FormatException('Expired public packet');
      }
      // A source grant has no target/code. It can prepare a request, but may
      // never stand in for a fully inspected target confirmation.
      if (widget.flow != EnrollmentPairingFlow.prepare &&
          (preview.code?.isNotEmpty != true || preview.target == null || preview.requestedRole == null)) {
        throw const FormatException('Missing target request');
      }
      if (widget.flow == EnrollmentPairingFlow.prepare && preview.requestId != null) {
        throw const FormatException('Expected a source grant');
      }
      setState(() => _preview = preview);
    } catch (e) {
      if (mounted && sharingSessionCurrent(ref, _scope) && generation == _generation) {
        setState(() => _failure = e);
      }
    } finally {
      if (mounted && sharingSessionCurrent(ref, _scope) && generation == _generation) {
        setState(() => _busy = false);
      }
    }
  }

  Future<void> _continue() async {
    final preview = _preview;
    if (_busy ||
        preview == null ||
        (widget.flow != EnrollmentPairingFlow.prepare && !_confirmed) ||
        !preview.expires.isAfter(DateTime.now()) ||
        !sharingSessionCurrent(ref, _scope)) {
      return;
    }
    setState(() {
      _working = true;
      _busy = true;
      _failure = null;
    });
    try {
      final service = ref.read(enrollmentServiceProvider);
      if (widget.flow == EnrollmentPairingFlow.restore) {
        final restored = await runWithFeedback(context, () async {
          await service.restorePairing(preview.bundle, _code.text);
          return true;
        });
        if (!mounted || !sharingSessionCurrent(ref, _scope) || restored == null) return;
        // Restores only signed local request history under existing OS pins.
        // It never creates a permission or confirms a fresh target request.
        refreshSharing(ref);
        closeDialog<void>(context);
      } else if (widget.flow == EnrollmentPairingFlow.submit) {
        final request = await runWithFeedback(context, () => service.submitTarget(preview.bundle, _code.text));
        if (!mounted || !sharingSessionCurrent(ref, _scope) || request == null) return;
        // Submission alone does not authorize or import any shared content.
        showSnack(context, context.l10n.enrollmentWait);
        closeDialog<void>(context);
      } else {
        final result = await runWithFeedback(
          context,
          () => widget.flow == EnrollmentPairingFlow.prepare
              ? service.prepareTarget(preview.bundle, _role)
              : service.endorseTarget(preview.bundle, _code.text),
        );
        if (!mounted || !sharingSessionCurrent(ref, _scope) || result == null) return;
        // Clear the completed composer before opening an output dialog: closing
        // it later must not offer a duplicate prepare/endorse operation.
        _input.clear();
        await showEnrollmentPackage(context, result);
        if (mounted && sharingSessionCurrent(ref, _scope)) closeDialog<void>(context);
      }
    } catch (e) {
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _failure = e);
    } finally {
      if (mounted && sharingSessionCurrent(ref, _scope)) {
        setState(() {
          _busy = false;
          _working = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final title = switch (widget.flow) {
      EnrollmentPairingFlow.prepare => l.enrollmentPrepare,
      EnrollmentPairingFlow.endorse => l.enrollmentEndorse,
      EnrollmentPairingFlow.submit => l.enrollmentSubmit,
      EnrollmentPairingFlow.restore => l.sharingReconcile,
    };
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        key: const ValueKey('enrollment-pairing-dialog'),
        title: title,
        width: 620,
        content: _EnrollmentScroll(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.enrollmentPackageHelp),
              const SizedBox(height: 12),
              TextField(
                key: const ValueKey('enrollment-packet'),
                controller: _input,
                readOnly: _working,
                minLines: 3,
                maxLines: 6,
                maxLength: maxEnrollmentPacketBytes,
                decoration: InputDecoration(labelText: l.enrollmentPackage),
              ),
              GlassButton(
                key: const ValueKey('enrollment-open'),
                label: l.enrollmentOpenPackage,
                onPressed: _busy ? null : _open,
              ),
              if (widget.flow == EnrollmentPairingFlow.prepare) ...[
                const SizedBox(height: 12),
                GlassSelect<SharingRole>(
                  key: const ValueKey('enrollment-request-role'),
                  value: _role,
                  items: [
                    GlassSelectItem(value: SharingRole.reader, label: l.sharingReader),
                    GlassSelectItem(value: SharingRole.editor, label: l.sharingEditor),
                  ],
                  onChanged: _busy
                      ? null
                      : (role) {
                          _role = role;
                          _invalidate();
                        },
                ),
                Text(_role == SharingRole.reader ? l.sharingReaderHelp : l.sharingEditorHelp),
              ],
              const SizedBox(height: 12),
              GlassButton(
                key: const ValueKey('enrollment-review'),
                label: l.enrollmentPreview,
                onPressed: _busy || _input.text.trim().isEmpty ? null : _review,
              ),
              if (_busy) const LinearProgressIndicator(),
              if (_failed) Text(rateLimitedMessage(l, _failure) ?? l.enrollmentBlocked),
              if (_preview case final preview?) ...[
                const SizedBox(height: 12),
                _EnrollmentPreview(preview),
                if (widget.flow != EnrollmentPairingFlow.prepare) ...[
                  Text(l.enrollmentCodeHelp),
                  const SizedBox(height: 8),
                  // Typed from the other screen; never filled in, never pasted.
                  Actions(
                    actions: {PasteTextIntent: DoNothingAction(consumesKey: true)},
                    child: TextField(
                      key: const ValueKey('enrollment-code-typed'),
                      controller: _code,
                      enabled: !_busy,
                      enableInteractiveSelection: false,
                      contextMenuBuilder: null,
                      keyboardType: TextInputType.number,
                      inputFormatters: [FilteringTextInputFormatter.allow(RegExp(r'[0-9 \-]'))],
                      decoration: InputDecoration(
                        labelText: l.enrollmentTypeCode,
                        helperText: l.enrollmentTypeCodeHelp,
                      ),
                      onChanged: (_) => setState(() {}),
                    ),
                  ),
                ],
              ],
            ],
          ),
        ),
        secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('enrollment-continue'),
          label: title,
          onPressed: _busy || _preview == null || (widget.flow != EnrollmentPairingFlow.prepare && !_confirmed)
              ? null
              : _continue,
        ),
      ),
    );
  }
}

class EnrollmentPackageDialog extends ConsumerStatefulWidget {
  const EnrollmentPackageDialog(this.pairing, {super.key});
  final EnrollmentPairing pairing;
  @override
  ConsumerState<EnrollmentPackageDialog> createState() => _EnrollmentPackageDialogState();
}

class _EnrollmentPackageDialogState extends ConsumerState<EnrollmentPackageDialog> {
  late final Object? _scope;
  EnrollmentPairing? _verified;
  bool _busy = true;
  Object? _failure;
  bool get _failed => _failure != null;
  @override
  void initState() {
    super.initState();
    _scope = ref.read(sharingSessionScopeProvider);
    _inspect();
  }

  Future<void> _inspect() async {
    try {
      if (!_boundedPublicJson(widget.pairing.bundle)) throw const FormatException('Invalid public packet');
      final result = await ref.read(enrollmentServiceProvider).inspectPairing(widget.pairing.bundle);
      if (!mounted || !sharingSessionCurrent(ref, _scope)) return;
      if (!_boundedPublicJson(result.bundle) || !result.expires.isAfter(DateTime.now())) {
        throw const FormatException('Expired public packet');
      }
      setState(() => _verified = result);
    } catch (e) {
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _failure = e);
    } finally {
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _busy = false);
    }
  }

  Future<void> _copy() async {
    final verified = _verified;
    if (_busy || verified == null || !sharingSessionCurrent(ref, _scope)) return;
    await copyPlainWithNotice(context, ref, verified.bundle);
  }

  Future<void> _save() async {
    final verified = _verified;
    if (_busy || verified == null || !sharingSessionCurrent(ref, _scope)) return;
    setState(() => _busy = true);
    try {
      final files = ref.read(fileDialogServiceProvider);
      final path = await files.chooseSaveFile(
        suggestedName: 'ConsoleCrypt-device-package.json',
        extensions: const ['json'],
      );
      if (path == null || !mounted || !sharingSessionCurrent(ref, _scope)) return;
      await File(path).writeAsString(verified.bundle, flush: true);
      if (!mounted || !sharingSessionCurrent(ref, _scope)) return;
      await files.finishSaveFile(path);
    } catch (e) {
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _failure = e);
    } finally {
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        key: const ValueKey('enrollment-package-dialog'),
        title: l.enrollmentPackage,
        width: 620,
        content: _EnrollmentScroll(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.enrollmentPackageHelp),
              if (_busy) const LinearProgressIndicator(),
              if (_failed) Text(rateLimitedMessage(l, _failure) ?? l.enrollmentBlocked),
              if (_verified case final verified?) ...[
                const SizedBox(height: 12),
                _EnrollmentPreview(verified, showCode: true),
              ],
            ],
          ),
        ),
        secondaryActions: [
          GlassButton(
            key: const ValueKey('enrollment-package-copy'),
            label: l.enrollmentCopyPackage,
            onPressed: _busy || _verified == null ? null : _copy,
          ),
          GlassButton(
            key: const ValueKey('enrollment-package-save'),
            label: l.enrollmentSavePackage,
            onPressed: _busy || _verified == null ? null : _save,
          ),
        ],
        primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      ),
    );
  }
}

class EnrollmentPendingDialog extends ConsumerStatefulWidget {
  const EnrollmentPendingDialog({super.key});
  @override
  ConsumerState<EnrollmentPendingDialog> createState() => _EnrollmentPendingDialogState();
}

class _EnrollmentPendingDialogState extends ConsumerState<EnrollmentPendingDialog> {
  late final Object? _scope;
  List<EnrollmentRequest> _requests = [];
  bool _busy = false;
  Object? _failure;
  bool get _failed => _failure != null;
  @override
  void initState() {
    super.initState();
    _scope = ref.read(sharingSessionScopeProvider);
    _load();
  }

  Future<void> _load() async {
    if (!sharingSessionCurrent(ref, _scope)) return;
    setState(() {
      _busy = true;
      _failure = null;
    });
    try {
      final requests = await ref.read(enrollmentServiceProvider).pendingRequests();
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _requests = requests);
    } catch (e) {
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _failure = e);
    } finally {
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _busy = false);
    }
  }

  Future<void> _respond(EnrollmentRequest request) async {
    if (_busy || !sharingSessionCurrent(ref, _scope)) return;
    setState(() => _busy = true);
    try {
      await runWithFeedback(context, () => ref.read(enrollmentServiceProvider).respond(request.shareId, request.id));
      if (mounted && sharingSessionCurrent(ref, _scope)) {
        refreshSharing(ref);
        await _load();
      }
    } catch (e) {
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _failure = e);
    } finally {
      if (mounted && sharingSessionCurrent(ref, _scope)) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        key: const ValueKey('enrollment-pending-dialog'),
        title: l.enrollmentPending,
        width: 620,
        content: _EnrollmentScroll(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.enrollmentWait),
              if (_busy) const LinearProgressIndicator(),
              if (_failed) Text(rateLimitedMessage(l, _failure) ?? l.enrollmentBlocked),
              for (final request in _requests)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: ContentSurface(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Text(request.target.deviceId),
                        Text(request.role == SharingRole.editor ? l.sharingEditor : l.sharingReader),
                        Text(enrollmentRequestLabel(context, request.state)),
                        SelectableText(request.code),
                        if (request.blockedReason == null &&
                            request.expires.isAfter(DateTime.now()) &&
                            const [
                              EnrollmentRequestState.pending,
                              EnrollmentRequestState.challenged,
                              EnrollmentRequestState.responded,
                            ].contains(request.state))
                          GlassButton(
                            key: ValueKey('enrollment-respond-${request.id}'),
                            label: l.enrollmentRespond,
                            onPressed: _busy ? null : () => _respond(request),
                          ),
                      ],
                    ),
                  ),
                ),
            ],
          ),
        ),
        secondaryActions: [
          GlassButton(
            key: const ValueKey('enrollment-reload'),
            label: l.sharingRefresh,
            onPressed: _busy ? null : _load,
          ),
        ],
        primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      ),
    );
  }
}

class _EnrollmentPreview extends StatelessWidget {
  const _EnrollmentPreview(this.pairing, {this.showCode = false});
  final EnrollmentPairing pairing;

  /// Only the device that shows a package displays its code; the device
  /// that reviews one must type the code from the other screen (TR-15).
  final bool showCode;
  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      SelectableText(pairing.shareId),
      Text('${context.l10n.enrollmentExpiry}: ${pairing.expires.toLocal()}'),
      if (pairing.target case final target?) Text(target.deviceId),
      if (pairing.requestedRole case final role?)
        Text(role == SharingRole.reader ? context.l10n.sharingReader : context.l10n.sharingEditor),
      if (pairing.code case final code? when showCode)
        SelectableText(
          code,
          key: const ValueKey('enrollment-full-code'),
          style: const TextStyle(fontFamily: 'monospace'),
        ),
    ],
  );
}

class _EnrollmentScroll extends StatelessWidget {
  const _EnrollmentScroll({required this.child});
  final Widget child;
  @override
  Widget build(BuildContext context) => ConstrainedBox(
    constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
    child: SingleChildScrollView(child: child),
  );
}
