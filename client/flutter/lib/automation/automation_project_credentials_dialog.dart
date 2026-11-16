import 'dart:convert';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart' show DependencyOriginalGuard;
import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/automation/automation_project_credentials_errors.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/hosts/host_auth_section.dart' show SshCredentialReferencePicker;
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class _Draft {
  _Draft(this.id, String name, this.production, Iterable<DeploymentCredentialReference> references)
    : name = TextEditingController(text: name),
      references = references.toList();
  final String id;
  final TextEditingController name;
  bool production;
  final List<DeploymentCredentialReference> references;
  DeploymentEnvironment freeze() =>
      DeploymentEnvironment(id: id, name: name.text.trim(), production: production, references: references);
}

/// Only references to existing objects are edited. Native owns all CAS/approval.
class AutomationProjectCredentialsDialog extends ConsumerStatefulWidget {
  const AutomationProjectCredentialsDialog({
    super.key,
    required this.scope,
    required this.project,
    required this.service,
    required this.originalCurrent,
    required this.originalLive,
  });
  final AutomationScope scope;
  final AutomationProject project;
  final AutomationProjectCredentialsService service;
  final bool Function() originalCurrent;
  final ValueListenable<bool> originalLive;
  @override
  ConsumerState<AutomationProjectCredentialsDialog> createState() => _State();
}

class _State extends ConsumerState<AutomationProjectCredentialsDialog> {
  final drafts = <_Draft>[];
  ProjectCredentialsIssue? issue;
  ProjectCredentialsContext? original;
  String? selected;
  bool loading = true, saving = false, failed = false, removePolicy = false;
  int generation = 0;
  bool get current => mounted && widget.originalCurrent() && identical(ref.read(automationScopeProvider), widget.scope);
  @override
  void initState() {
    super.initState();
    load();
  }

  @override
  void dispose() {
    generation++;
    for (final draft in drafts) {
      draft.name.dispose();
    }
    super.dispose();
  }

  Future<void> load() async {
    final token = ++generation;
    if (!current) return;
    setState(() {
      loading = true;
      failed = false;
      issue = null;
      original = null;
    });
    try {
      final runtime = ref.read(criticalRuntimeEventsProvider), frame = runtime?.currentScope;
      final scope = frame?.scope;
      if (scope == null ||
          frame?.streamUnavailable != false ||
          frame?.unsupported != false ||
          scope.profileId != widget.scope.profileId?.toString()) {
        throw const FormatException('scope');
      }
      final raw = await widget.service.projectCredentialsContext(widget.project.id);
      if (!mounted ||
          !current ||
          token != generation ||
          runtime?.currentScope.epoch != frame?.epoch ||
          runtime?.currentScope.scope != scope) {
        return;
      }
      final value = ProjectCredentialsContext.parse(
        raw,
        profileId: scope.profileId,
        nonce: scope.nonce,
        project: widget.project,
      );
      setState(() {
        for (final draft in drafts) {
          draft.name.dispose();
        }
        drafts.clear();
        for (final env in value.policy?.environments ?? <DeploymentEnvironment>[]) {
          drafts.add(_Draft(env.id, env.name, env.production, env.references));
        }
        selected = drafts.firstOrNull?.id;
        original = value;
        removePolicy = false;
      });
    } catch (failure) {
      if (current && token == generation) {
        setState(() {
          failed = true;
          issue = projectCredentialsIssue(failure);
        });
      }
    } finally {
      if (current && token == generation) setState(() => loading = false);
    }
  }

  _Draft? get selectedDraft => drafts.where((v) => v.id == selected).firstOrNull;
  Future<void> chooseReference(_Draft draft, {String? hostId}) async {
    if (!mounted || !current || saving || loading) return;
    final picked = hostId == null
        ? await showGlassDialog<Host>(
            context,
            builder: (_) => DependencyOriginalGuard(
              original: widget.originalLive,
              builder: (_) => AutomationScopeGuard(scope: widget.scope, builder: (_) => const HostPickerDialog()),
            ),
          )
        : null;
    if (!mounted || !current || !drafts.contains(draft)) return;
    final id = hostId ?? picked?.id.value;
    if (id == null ||
        !(ref.read(hostsProvider).value ?? <Host>[]).any((h) => h.id.value == id && h.protocol == HostProtocol.ssh)) {
      return;
    }
    final credential = await showGlassDialog<Credential>(
      context,
      builder: (_) => DependencyOriginalGuard(
        original: widget.originalLive,
        builder: (_) => AutomationScopeGuard(scope: widget.scope, builder: (_) => const SshCredentialReferencePicker()),
      ),
    );
    if (!mounted ||
        !current ||
        !drafts.contains(draft) ||
        credential == null ||
        !(ref.read(credentialsProvider).value ?? <Credential>[]).any(
          (v) =>
              v.id == credential.id &&
              const {
                CredentialKind.password,
                CredentialKind.sshPrivateKey,
                CredentialKind.sshCertificate,
              }.contains(v.kind),
        )) {
      return;
    }
    if (!(ref.read(hostsProvider).value ?? <Host>[]).any((h) => h.id.value == id && h.protocol == HostProtocol.ssh)) {
      return;
    }
    setState(() {
      draft.references.removeWhere((v) => v.hostId == id);
      draft.references.add(DeploymentCredentialReference(id, credential.id.value));
      removePolicy = false;
    });
  }

  Future<void> requestRemoval() async {
    if (!mounted || !current || saving || loading || original?.policy == null) return;
    final accepted = await showGlassDialog<bool>(
      context,
      builder: (_) => DependencyOriginalGuard(
        original: widget.originalLive,
        builder: (context) => AlertDialog(
          title: Text(context.l10n.automationDeploymentRemoveTitle),
          content: Text(context.l10n.automationDeploymentRemoveHint),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context, false), child: Text(context.l10n.commonCancel)),
            TextButton(
              key: const ValueKey('deployment-remove-accept'),
              onPressed: () => Navigator.pop(context, true),
              child: Text(context.l10n.commonDelete),
            ),
          ],
        ),
      ),
    );
    if (accepted == true && current) setState(() => removePolicy = true);
  }

  Future<void> save() async {
    final context = original;
    if (!mounted || !current || loading || saving || context == null) return;
    ProjectCredentialsPolicy? policy;
    try {
      if (!removePolicy) policy = ProjectCredentialsPolicy(drafts.map((v) => v.freeze()).toList());
    } catch (failure) {
      setState(() {
        failed = true;
        issue = projectCredentialsIssue(failure);
      });
      return;
    }
    final token = ++generation;
    setState(() {
      saving = true;
      failed = false;
      issue = null;
    });
    try {
      final result = await widget.service.saveProjectCredentials(context.saveRequest(policy));
      if (!mounted || !current || token != generation) return;
      final next = context.saved(result);
      if (jsonEncode(next.policy?.toJson()) != jsonEncode(policy?.toJson())) throw const FormatException('save');
      Navigator.of(this.context).pop(AutomationProject.fromJson(result['project'] as AutomationJson));
    } catch (failure) {
      if (current && token == generation) {
        setState(() {
          failed = true;
          issue = projectCredentialsIssue(failure);
        });
      }
    } finally {
      if (current && token == generation) setState(() => saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, env = selectedDraft;
    final hosts = ref.watch(hostsProvider).value ?? <Host>[],
        credentials = ref.watch(credentialsProvider).value ?? <Credential>[];
    return GlassDialog(
      title: l.automationDeploymentTitle,
      width: 680,
      content: ConstrainedBox(
        constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InfoBanner(tone: BannerTone.info, message: l.automationDeploymentHint),
              if (loading || saving) const LinearProgressIndicator(),
              if (failed) InfoBanner(tone: BannerTone.warning, message: projectCredentialsIssueMessage(l, issue)),
              if (original == null && !loading) TextButton(onPressed: load, child: Text(l.automationDeploymentRefresh)),
              if (original != null) ...[
                if (original!.policy == null) Text(l.automationDeploymentLegacy),
                GlassSelect<String?>(
                  value: selected,
                  expand: true,
                  onChanged: saving ? null : (v) => setState(() => selected = v),
                  items: [
                    GlassSelectItem(value: null, label: l.automationDeploymentChoose),
                    for (final value in drafts)
                      GlassSelectItem(
                        value: value.id,
                        label: value.name.text.isEmpty ? l.automationDeploymentNew : value.name.text,
                      ),
                  ],
                ),
                TextButton(
                  key: const ValueKey('deployment-add-environment'),
                  onPressed: saving || drafts.length >= 16
                      ? null
                      : () => setState(() {
                          final value = _Draft(generateUuidV4(), '', false, const []);
                          drafts.add(value);
                          selected = value.id;
                          removePolicy = false;
                        }),
                  child: Text(l.automationDeploymentAdd),
                ),
                if (env != null) ...[
                  TextField(
                    key: const ValueKey('deployment-name'),
                    controller: env.name,
                    enabled: !saving,
                    decoration: InputDecoration(labelText: l.commonName),
                  ),
                  CheckboxListTile(
                    key: const ValueKey('deployment-production'),
                    value: env.production,
                    onChanged: saving ? null : (v) => setState(() => env.production = v == true),
                    title: Text(l.automationDeploymentProduction),
                  ),
                  Text(l.automationDeploymentProductionHint),
                  for (final reference in env.references)
                    ListTile(
                      key: ValueKey('deployment-reference-${reference.hostId}'),
                      title: Text(
                        hosts.where((v) => v.id.value == reference.hostId).firstOrNull?.name ?? reference.hostId,
                      ),
                      subtitle: Text(
                        credentials.where((v) => v.id.value == reference.credentialId).firstOrNull?.name ??
                            l.automationDeploymentReferenceMissing,
                      ),
                      onTap: saving ? null : () => chooseReference(env, hostId: reference.hostId),
                      trailing: IconButton(
                        icon: const Icon(Icons.remove_circle_outline),
                        onPressed: saving ? null : () => setState(() => env.references.remove(reference)),
                      ),
                    ),
                  TextButton(
                    key: const ValueKey('deployment-add-reference'),
                    onPressed: saving || env.references.length >= 256 ? null : () => chooseReference(env),
                    child: Text(l.automationDeploymentAddReference),
                  ),
                  Text(l.automationDeploymentReuse),
                  TextButton(
                    key: const ValueKey('deployment-delete-environment'),
                    onPressed: saving
                        ? null
                        : () => setState(() {
                            drafts.remove(env);
                            env.name.dispose();
                            selected = drafts.firstOrNull?.id;
                          }),
                    child: Text(l.commonDelete),
                  ),
                ],
                if (original!.policy != null)
                  TextButton(
                    key: const ValueKey('deployment-remove-policy'),
                    onPressed: saving ? null : requestRemoval,
                    child: Text(l.automationDeploymentRemoveTitle),
                  ),
                if (removePolicy) InfoBanner(tone: BannerTone.warning, message: l.automationDeploymentRemoveHint),
              ],
            ],
          ),
        ),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: saving ? null : () => Navigator.pop(context))],
      primaryAction: GlassButton(
        key: const ValueKey('deployment-save'),
        label: l.commonSave,
        busy: saving,
        onPressed: loading || saving || original == null || !removePolicy && drafts.isEmpty ? null : save,
      ),
    );
  }
}
