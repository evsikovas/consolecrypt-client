import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/enrollment_models.dart';
import 'package:consolecrypt/sharing/enrollment_pairing.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showEnrollmentOwner(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => EnrollmentOwnerDialog(item));

class EnrollmentOwnerDialog extends ConsumerStatefulWidget {
  const EnrollmentOwnerDialog(this.item, {super.key});
  final SharingItem item;
  @override
  ConsumerState<EnrollmentOwnerDialog> createState() => _EnrollmentOwnerDialogState();
}

class _EnrollmentOwnerDialogState extends ConsumerState<EnrollmentOwnerDialog> {
  late final Object? _scope;
  List<EnrollmentGrant> _grants = [];
  List<EnrollmentRequest> _requests = [];
  bool _busy = false, _failed = false;
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
      _failed = false;
    });
    try {
      final service = ref.read(enrollmentServiceProvider);
      final grants = await service.grants(widget.item.id);
      if (!mounted || !sharingSessionCurrent(ref, _scope)) return;
      final requests = await service.requests(widget.item.id);
      if (!mounted || !sharingSessionCurrent(ref, _scope)) return;
      setState(() {
        _grants = grants;
        _requests = requests;
      });
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _disable(EnrollmentGrant grant) async {
    final l = context.l10n;
    final yes = await showConfirmDialog(
      context,
      title: l.enrollmentDisable,
      message: l.enrollmentDisableHelp,
      confirmLabel: l.enrollmentDisable,
      destructive: true,
      secure: true,
    );
    if (!yes || !mounted || !sharingSessionCurrent(ref, _scope)) return;
    setState(() => _busy = true);
    await runWithFeedback(context, () => ref.read(enrollmentServiceProvider).revokeGrant(widget.item.id, grant.id));
    if (mounted && sharingSessionCurrent(ref, _scope)) await _load();
  }

  Future<void> _challenge(EnrollmentRequest request) async {
    if (!sharingSessionCurrent(ref, _scope)) return;
    setState(() => _busy = true);
    await runWithFeedback(context, () => ref.read(enrollmentServiceProvider).challenge(widget.item.id, request.id));
    if (mounted && sharingSessionCurrent(ref, _scope)) await _load();
  }

  Future<void> _accept(EnrollmentRequest request) async {
    final l = context.l10n;
    final yes = await showConfirmDialog(
      context,
      title: l.enrollmentAccept,
      message: l.enrollmentManualConfirmed,
      confirmLabel: l.enrollmentAccept,
      secure: true,
      extra: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(request.target.deviceId),
          Text(request.role == SharingRole.editor ? l.sharingEditor : l.sharingReader),
          SelectableText(request.code),
        ],
      ),
    );
    if (!yes || !mounted || !sharingSessionCurrent(ref, _scope)) return;
    setState(() => _busy = true);
    await runWithFeedback(
      context,
      () => ref.read(enrollmentServiceProvider).accept(widget.item.id, request.id, confirmedManual: true),
    );
    if (mounted && sharingSessionCurrent(ref, _scope)) {
      refreshSharing(ref);
      await _load();
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final available = ref.watch(sharingStatusProvider).value?.supportsOwnerOnlineEnrollment == true;
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        title: l.enrollmentTitle,
        content: SizedBox(
          width: 620,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(l.enrollmentHelp),
                  if (_busy) const LinearProgressIndicator(),
                  if (_failed) ...[
                    Text(l.enrollmentBlocked),
                    GlassButton(
                      label: l.sharingReconcile,
                      icon: Icons.verified_user_outlined,
                      onPressed: _busy
                          ? null
                          : () async {
                              await showAppDialog<void>(
                                context,
                                secure: true,
                                builder: (_) =>
                                    SharingAcceptDialog(widget.item, reconcile: true, enrollmentRecovery: true),
                              );
                              if (mounted && sharingSessionCurrent(ref, _scope)) await _load();
                            },
                    ),
                  ],
                  const SizedBox(height: 12),
                  GlassButton(
                    key: const ValueKey('enrollment-new-grant'),
                    label: l.enrollmentCreate,
                    icon: Icons.add_rounded,
                    onPressed: _busy || !available
                        ? null
                        : () async {
                            await showAppDialog<void>(
                              context,
                              secure: true,
                              builder: (_) => EnrollmentCreateDialog(widget.item),
                            );
                            if (mounted && sharingSessionCurrent(ref, _scope)) await _load();
                          },
                  ),
                  for (final g in _grants)
                    ListTile(
                      title: Text(g.anchor.deviceId),
                      subtitle: Text(
                        '${g.active ? l.enrollmentActive : l.enrollmentDisabled} · ${g.admitted}/${g.maxAdmissions}',
                      ),
                      trailing: Wrap(
                        spacing: 4,
                        children: [
                          if (g.active && available)
                            GlassIconButton(
                              tooltip: l.enrollmentExport,
                              icon: Icons.devices_outlined,
                              onPressed: _busy
                                  ? null
                                  : () async {
                                      final bundle = await runWithFeedback(
                                        context,
                                        () => ref.read(enrollmentServiceProvider).exportBundle(widget.item.id, g.id),
                                      );
                                      if (bundle != null && context.mounted && sharingSessionCurrent(ref, _scope)) {
                                        await showEnrollmentPackage(context, bundle);
                                      }
                                    },
                            ),
                          if (g.state == EnrollmentGrantState.active && !g.frozen)
                            GlassIconButton(
                              tooltip: l.enrollmentDisable,
                              icon: Icons.link_off_rounded,
                              onPressed: _busy ? null : () => _disable(g),
                            ),
                        ],
                      ),
                    ),
                  const SizedBox(height: 12),
                  Text(l.enrollmentRequests, style: Theme.of(context).textTheme.titleMedium),
                  for (final r in _requests)
                    ContentSurface(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          Text(r.target.deviceId),
                          Text(r.role == SharingRole.editor ? l.sharingEditor : l.sharingReader),
                          Text(enrollmentRequestLabel(context, r.state)),
                          if (r.state == EnrollmentRequestState.pending ||
                              r.state == EnrollmentRequestState.challenged ||
                              r.state == EnrollmentRequestState.responded)
                            GlassButton(label: l.enrollmentChallenge, onPressed: _busy ? null : () => _challenge(r)),
                          if (r.state == EnrollmentRequestState.responded)
                            GlassButton.prominent(
                              label: l.enrollmentAccept,
                              onPressed: _busy ? null : () => _accept(r),
                            ),
                        ],
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
        secondaryActions: [GlassButton(label: l.sharingRefresh, onPressed: _busy ? null : _load)],
        primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      ),
    );
  }
}

class EnrollmentCreateDialog extends ConsumerStatefulWidget {
  const EnrollmentCreateDialog(this.item, {super.key});
  final SharingItem item;
  @override
  ConsumerState<EnrollmentCreateDialog> createState() => _EnrollmentCreateDialogState();
}

class _EnrollmentCreateDialogState extends ConsumerState<EnrollmentCreateDialog> {
  late final Object? _scope;
  SharingGrant? _anchor;
  SharingRole _role = SharingRole.reader;
  int _days = 7, _quota = 1;
  bool _automatic = false, _confirmed = false, _busy = false;
  @override
  void initState() {
    super.initState();
    _scope = ref.read(sharingSessionScopeProvider);
  }

  Future<void> _create() async {
    if (_anchor == null || !_confirmed || !sharingSessionCurrent(ref, _scope)) return;
    setState(() => _busy = true);
    final grant = await runWithFeedback(
      context,
      () => ref
          .read(enrollmentServiceProvider)
          .createGrant(
            widget.item.id,
            EnrollmentGrantCreate(
              anchor: _anchor!.identity,
              roleCeiling: _role,
              mode: _automatic ? EnrollmentMode.automatic : EnrollmentMode.manual,
              expires: DateTime.now().toUtc().add(Duration(days: _days)),
              maxAdmissions: _quota,
            ),
          ),
    );
    if (!mounted || !sharingSessionCurrent(ref, _scope)) return;
    if (grant != null) {
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        title: l.enrollmentCreate,
        content: SizedBox(
          width: 560,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(l.enrollmentHelp),
                  const SizedBox(height: 12),
                  Text(l.enrollmentAnchor),
                  GlassSelect<String?>(
                    value: _anchor?.identity.deviceId,
                    items: [
                      GlassSelectItem<String?>(value: null, label: l.enrollmentAnchor),
                      for (final g in widget.item.memberRoles)
                        GlassSelectItem(value: g.identity.deviceId, label: g.identity.deviceId),
                    ],
                    onChanged: _busy
                        ? null
                        : (id) => setState(() {
                            _anchor = id == null
                                ? null
                                : widget.item.memberRoles.firstWhere((g) => g.identity.deviceId == id);
                            _role = SharingRole.reader;
                            _confirmed = false;
                          }),
                  ),
                  if (_anchor != null) ...[
                    const SizedBox(height: 8),
                    SelectableText(_anchor!.identity.code),
                    GlassSelect<SharingRole>(
                      value: _role,
                      items: [
                        GlassSelectItem(value: SharingRole.reader, label: l.sharingReader),
                        if (_anchor!.role == SharingRole.editor)
                          GlassSelectItem(value: SharingRole.editor, label: l.sharingEditor),
                      ],
                      onChanged: _busy ? null : (role) => setState(() => _role = role),
                    ),
                  ],
                  const SizedBox(height: 12),
                  Text(l.enrollmentExpiry),
                  GlassSelect<int>(
                    value: _days,
                    items: [
                      GlassSelectItem(value: 1, label: l.enrollmentOneDay),
                      GlassSelectItem(value: 7, label: l.enrollmentSevenDays),
                      GlassSelectItem(value: 30, label: l.enrollmentThirtyDays),
                    ],
                    onChanged: _busy ? null : (days) => setState(() => _days = days),
                  ),
                  const SizedBox(height: 12),
                  Text(l.enrollmentQuota),
                  GlassSelect<int>(
                    value: _quota,
                    items: [
                      for (final n in [1, 2, 4, 8, 16]) GlassSelectItem(value: n, label: n.toString()),
                    ],
                    onChanged: _busy ? null : (n) => setState(() => _quota = n),
                  ),
                  CheckboxListTile(
                    contentPadding: EdgeInsets.zero,
                    title: Text(l.enrollmentAutomatic),
                    value: _automatic,
                    onChanged: _busy ? null : (v) => setState(() => _automatic = v == true),
                  ),
                  Text(l.enrollmentAutomaticHelp),
                  CheckboxListTile(
                    key: const ValueKey('enrollment-grant-confirmed'),
                    contentPadding: EdgeInsets.zero,
                    title: Text(l.enrollmentCreateConfirmed),
                    value: _confirmed,
                    onChanged: _busy ? null : (v) => setState(() => _confirmed = v == true),
                  ),
                ],
              ),
            ),
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('enrollment-grant-create'),
          label: l.commonSave,
          onPressed: _busy || _anchor == null || !_confirmed ? null : _create,
        ),
      ),
    );
  }
}
