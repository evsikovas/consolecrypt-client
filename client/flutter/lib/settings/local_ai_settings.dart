import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/ai/local_ai_providers.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter/services.dart' show rootBundle;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class LocalAiSettings extends ConsumerWidget {
  const LocalAiSettings({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (!ref.watch(deviceLocalAiAvailableProvider)) return const SizedBox.shrink();
    final runtime = ref.watch(localAiRuntimeProvider);
    final status = ref.watch(localAiStatusProvider).value ?? runtime.status;
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final hardware = ref.watch(localAiHardwareProvider).value;
    final l = context.l10n;
    final busy = const {LocalAiPhase.installing, LocalAiPhase.starting, LocalAiPhase.stopping}.contains(status.phase);
    String tier(LocalAiModel model) => switch (model.tier) {
      LocalAiTier.light => l.localAiTierLight,
      LocalAiTier.medium => l.localAiTierMedium,
      LocalAiTier.strong => l.localAiTierStrong,
      LocalAiTier.custom => l.localAiTierCustom,
    };
    Future<void> perform(Future<void> Function() action) async {
      try {
        await action();
      } on LocalAiDownloadCancelled {
        // Explicit cancellation is already reflected in the status.
      } catch (_) {
        if (context.mounted) showSnack(context, l.localAiOperationFailed, error: true);
      }
    }

    final label = switch (status.phase) {
      LocalAiPhase.unsupported => l.localAiUnsupported,
      LocalAiPhase.notInstalled => l.localAiNotInstalled,
      LocalAiPhase.installing => l.localAiInstalling,
      LocalAiPhase.stopped => l.localAiStopped,
      LocalAiPhase.starting => l.localAiStarting,
      LocalAiPhase.ready => l.localAiReady,
      LocalAiPhase.stopping => l.localAiStopping,
      LocalAiPhase.error => l.localAiError,
    };
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s16),
      child: SectionCard(
        key: const ValueKey('local-ai-settings'),
        title: l.localAiTitle,
        icon: Icons.memory_rounded,
        subtitle: l.localAiDescription,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            KeyedSubtree(
              key: const ValueKey('local-ai-model'),
              child: GlassDropdownField<String>(
                key: ValueKey((runtime.selectedModel.id, status.selectionRevision)),
                value: runtime.selectedModel.id,
                label: l.localAiModel,
                items: [
                  for (final model in runtime.models)
                    GlassSelectItem(
                      // Star marks the recommended model, a done mark the installed ones.
                      key: hardware?.recommendedModel?.id == model.id
                          ? ValueKey('local-ai-recommended-${model.id}')
                          : null,
                      value: model.id,
                      label: '${tier(model)} · ${model.label}',
                      subtitle: [
                        l.localAiModelRequirements(formatBytes(l, model.bytes), model.recommendedRamGiB),
                        if (status.installedModelIds.contains(model.id)) l.localAiModelInstalled,
                        if (hardware?.recommendedModel?.id == model.id) l.localAiRecommended,
                      ].join(' · '), // l10n-ignore: Separator between localized facts.
                      icon: hardware?.recommendedModel?.id == model.id
                          ? Icons.star_rounded
                          : status.installedModelIds.contains(model.id)
                          ? Icons.download_done_rounded
                          : null,
                    ),
                ],
                onChanged: busy ? null : (id) => perform(() => runtime.selectModel(id)),
              ),
            ),
            const SizedBox(height: 12),
            Text(l.localAiDownloadSize(formatBytes(l, runtime.selectedModel.bytes))),
            const SizedBox(height: 4),
            Text(l.localAiRamRecommendation(runtime.selectedModel.recommendedRamGiB)),
            if (hardware?.totalMemoryBytes != null) ...[
              const SizedBox(height: 4),
              Text(l.localAiDetectedMemory(formatBytes(l, hardware!.totalMemoryBytes!))),
            ],
            const SizedBox(height: 4),
            Text(
              hardware?.usesMetal == true ? l.localAiMetalHelp : l.localAiCpuHelp,
              style: Theme.of(context).textTheme.bodySmall,
            ),
            const SizedBox(height: 4),
            Text(
              runtime.selectedModel.isCustom
                  ? l.localAiCustomModelNotice
                  : l.localAiModelAttribution(runtime.selectedModel.publisher, runtime.selectedModel.license),
              style: Theme.of(context).textTheme.bodySmall,
            ),
            GlassButton.plain(
              key: const ValueKey('local-ai-notices'),
              label: l.localAiModelNotices,
              onPressed: () => perform(() async {
                final notices = await rootBundle.loadString('assets/licenses/LOCAL-AI-MODEL-NOTICES.txt');
                if (!context.mounted) return;
                await showAppDialog<void>(
                  context,
                  builder: (dialogContext) => GlassDialog(
                    title: l.localAiModelNotices,
                    width: 640,
                    content: SizedBox(height: 360, child: SingleChildScrollView(child: SelectableText(notices))),
                    primaryAction: GlassButton.prominent(
                      label: l.commonClose,
                      onPressed: () => closeDialog<void>(dialogContext),
                    ),
                  ),
                );
              }),
            ),
            const SizedBox(height: 8),
            Text(label, key: const ValueKey('local-ai-status')),
            if (status.phase == LocalAiPhase.installing) ...[
              const SizedBox(height: 8),
              LinearProgressIndicator(key: const ValueKey('local-ai-progress'), value: status.progress),
              const SizedBox(height: 4),
              Text('${formatBytes(l, status.downloadedBytes)} / ${formatBytes(l, status.totalBytes)}'),
            ],
            if (status.phase == LocalAiPhase.error && status.message != null) ...[
              const SizedBox(height: 4),
              Text(switch (status.message) {
                'install_failed' => l.localAiInstallFailed,
                'invalid_gguf' => l.localAiInvalidGguf,
                'import_failed' => l.localAiImportFailed,
                'custom_model_missing' => l.localAiCustomMissing,
                'installed_files_changed' => l.localAiFilesChanged,
                'local_storage_unavailable' => l.localAiStorageUnavailable,
                'bundled_runtime_unavailable' => l.localAiBundledRuntimeMissing,
                'runtime_not_installed' =>
                  AppPlatform.isMacOS ? l.localAiBundledRuntimeMissing : l.localAiRuntimeMissing,
                _ => l.localAiStartFailed,
              }, style: Theme.of(context).textTheme.bodySmall),
            ],
            if (status.hasInstalledModels && !status.isInstalled && local.preferDeviceLocalAi) ...[
              const SizedBox(height: 8),
              Text(l.localAiInstallSelectedHelp, key: const ValueKey('local-ai-selected-missing')),
            ],
            if (status.isInstalled && !status.runtimeInstalled) ...[
              const SizedBox(height: 8),
              Text(AppPlatform.isMacOS ? l.localAiBundledRuntimeMissing : l.localAiRuntimeMissing),
            ],
            CheckboxListTile(
              key: const ValueKey('local-ai-default'),
              contentPadding: EdgeInsets.zero,
              controlAffinity: ListTileControlAffinity.leading,
              value: local.preferDeviceLocalAi,
              onChanged: (value) =>
                  ref.read(settingsServiceProvider).updateLocal(local.copyWith(preferDeviceLocalAi: value ?? false)),
              title: Text(l.localAiDefault),
              subtitle: Text(l.localAiDeviceOnly),
            ),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                if (status.phase == LocalAiPhase.installing)
                  GlassButton(
                    key: const ValueKey('local-ai-cancel'),
                    label: l.commonCancel,
                    onPressed: runtime.cancelInstall,
                  )
                else if (!status.isInstalled && runtime.selectedModel.isCustom)
                  GlassButton.plain(
                    key: const ValueKey('local-ai-remove'),
                    label: l.localAiRemove,
                    icon: Icons.delete_outline_rounded,
                    onPressed: busy ? null : () => perform(runtime.uninstall),
                  )
                else if (!status.isInstalled)
                  GlassButton.prominent(
                    key: const ValueKey('local-ai-install'),
                    label: l.localAiInstall,
                    icon: Icons.download_rounded,
                    onPressed: busy || status.phase == LocalAiPhase.unsupported ? null : () => perform(runtime.install),
                  )
                else ...[
                  if (!status.runtimeInstalled && !AppPlatform.isMacOS)
                    GlassButton(
                      key: const ValueKey('local-ai-repair-runtime'),
                      label: l.localAiRepairRuntime,
                      icon: Icons.download_rounded,
                      onPressed: busy ? null : () => perform(runtime.install),
                    )
                  else if (status.phase == LocalAiPhase.ready)
                    GlassButton(
                      key: const ValueKey('local-ai-stop'),
                      label: l.localAiStop,
                      icon: Icons.stop_rounded,
                      onPressed: busy ? null : () => perform(runtime.stop),
                    )
                  else
                    GlassButton(
                      key: const ValueKey('local-ai-start'),
                      label: l.localAiStart,
                      icon: Icons.play_arrow_rounded,
                      onPressed: busy || !status.runtimeInstalled
                          ? null
                          : () => perform(() async {
                              await runtime.ensureReady();
                            }),
                    ),
                  GlassButton.plain(
                    key: const ValueKey('local-ai-remove'),
                    label: l.localAiRemove,
                    icon: Icons.delete_outline_rounded,
                    onPressed: busy ? null : () => perform(runtime.uninstall),
                  ),
                ],
              ],
            ),
            const SizedBox(height: 8),
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: GlassButton.plain(
                key: const ValueKey('local-ai-import'),
                label: l.localAiImport,
                icon: Icons.add_rounded,
                onPressed: busy
                    ? null
                    : () => perform(() async {
                        final path = await ref
                            .read(fileDialogServiceProvider)
                            .chooseOpenFile(extensions: const ['gguf']);
                        if (path != null) await runtime.importModel(path);
                      }),
              ),
            ),
            Text(l.localAiImportHelp, style: Theme.of(context).textTheme.bodySmall),
            const SizedBox(height: 8),
            Text(l.localAiModelsPreserved, style: Theme.of(context).textTheme.bodySmall),
            const SizedBox(height: 8),
            Text(l.localAiResourceHelp, style: Theme.of(context).textTheme.bodySmall),
          ],
        ),
      ),
    );
  }
}
