import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/automation/automation_project_credentials_errors.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// A local Native metadata read. Never saves, connects, prepares or defaults a choice.
class AutomationDeploymentEnvironmentSelection extends ConsumerStatefulWidget {
  const AutomationDeploymentEnvironmentSelection({
    super.key,
    required this.scope,
    required this.project,
    required this.service,
    required this.onChanged,
    this.busy = false,
  });
  final AutomationScope scope;
  final AutomationProject project;
  final AutomationProjectCredentialsService service;
  final bool busy;
  final void Function(bool ready, ProjectCredentialsContext? context, String? environmentId, bool Function() original)
  onChanged;
  @override
  ConsumerState<AutomationDeploymentEnvironmentSelection> createState() => _State();
}

class _State extends ConsumerState<AutomationDeploymentEnvironmentSelection> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;
  ProjectCredentialsIssue? issue;
  ProjectCredentialsContext? data;
  String? selection;
  bool loading = true, failed = false, retired = false, announcedRetirement = false;
  int generation = 0;
  Object? capturedApp, capturedRuntime;
  int? capturedEpoch;
  String? capturedNonce;
  StreamSubscription<dynamic>? notices;
  bool get current {
    if (!mounted || retired) return false;
    final runtime = ref.read(criticalRuntimeEventsProvider), frame = runtime?.currentScope;
    final valid =
        identical(ref.read(automationScopeProvider), widget.scope) &&
        identical(ref.read(appServicesProvider), capturedApp) &&
        identical(ref.read(automationServiceProvider), widget.service) &&
        identical(runtime, capturedRuntime) &&
        frame?.epoch == capturedEpoch &&
        frame?.scope?.nonce == capturedNonce &&
        frame?.scope?.profileId == widget.scope.profileId?.toString() &&
        frame?.unsupported == false &&
        frame?.streamUnavailable == false;
    if (!valid) retired = true;
    return valid;
  }

  @override
  void initState() {
    super.initState();
    capturedApp = ref.read(appServicesProvider);
    final runtime = ref.read(criticalRuntimeEventsProvider);
    capturedRuntime = runtime;
    capturedEpoch = runtime?.currentScope.epoch;
    capturedNonce = runtime?.currentScope.scope?.nonce;
    void observe() {
      if (current) return;
      if (announcedRetirement) return;
      announcedRetirement = true;
      generation++;
      scheduleMicrotask(() {
        if (mounted) {
          setState(() {
            failed = true;
            loading = false;
            data = null;
            selection = null;
          });
          widget.onChanged(false, null, null, () => false);
        }
      });
    }

    ref.listenManual(appServicesProvider, (_, _) => observe());
    ref.listenManual(automationServiceProvider, (_, _) => observe());
    ref.listenManual(automationScopeProvider, (_, _) => observe());
    ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe());
    notices = runtime?.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) {
        retired = true;
        observe();
      },
      onDone: () {
        retired = true;
        observe();
      },
    );
    scheduleMicrotask(load);
  }

  @override
  void didUpdateWidget(AutomationDeploymentEnvironmentSelection old) {
    super.didUpdateWidget(old);
    if (!identical(old.scope, widget.scope) ||
        !identical(old.service, widget.service) ||
        old.project.id != widget.project.id ||
        old.project.revision != widget.project.revision) {
      retired = true;
      generation++;
    }
  }

  @override
  void dispose() {
    generation++;
    retired = true;
    unawaited(notices?.cancel());
    super.dispose();
  }

  void publish() {
    widget.onChanged(
      current && !loading && !failed && (data?.policy == null || selection != null),
      data,
      selection,
      () => current,
    );
  }

  Future<void> load() async {
    if (!mounted) return;
    final token = ++generation;
    setState(() {
      loading = true;
      failed = false;
      issue = null;
      data = null;
      selection = null;
    });
    publish();
    try {
      if (!current || capturedNonce == null) throw const FormatException('scope');
      final raw = await widget.service.projectCredentialsContext(widget.project.id);
      if (!current || token != generation) return;
      final value = ProjectCredentialsContext.parse(
        raw,
        profileId: widget.scope.profileId.toString(),
        nonce: capturedNonce!,
        project: widget.project,
      );
      setState(() => data = value);
    } catch (failure) {
      if (mounted && token == generation) {
        setState(() {
          failed = true;
          issue = projectCredentialsIssue(failure);
        });
      }
    } finally {
      if (mounted && token == generation) {
        setState(() => loading = false);
        publish();
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (loading) const LinearProgressIndicator(),
        if (failed || retired) InfoBanner(tone: BannerTone.warning, message: projectCredentialsIssueMessage(l, issue)),
        if (failed && !retired)
          TextButton(
            key: const ValueKey('deployment-reload'),
            onPressed: widget.busy || loading ? null : load,
            child: Text(l.automationDeploymentRefresh),
          ),
        if (!loading && data?.policy == null && !failed && !retired) Text(l.automationDeploymentLegacy),
        if (data?.policy case final ProjectCredentialsPolicy policy) ...[
          Text(l.automationDeploymentHint),
          GlassSelect<String?>(
            key: const ValueKey('deployment-run-selection'),
            value: selection,
            expand: true,
            semanticLabel: l.automationDeploymentChoose,
            onChanged: widget.busy || retired
                ? null
                : (value) {
                    setState(() => selection = value);
                    publish();
                  },
            items: [
              GlassSelectItem(value: null, label: l.automationDeploymentChoose),
              for (final env in policy.environments) GlassSelectItem(value: env.id, label: env.name),
            ],
          ),
          if (policy.environments.where((env) => env.id == selection).firstOrNull?.production == true)
            InfoBanner(tone: BannerTone.warning, message: l.automationDeploymentProduction),
          Text(l.automationDeploymentProductionHint),
        ],
      ],
    );
  }
}
