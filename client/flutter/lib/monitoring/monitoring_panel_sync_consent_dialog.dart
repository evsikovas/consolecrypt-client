import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panels.dart';
import 'package:material_ui/material_ui.dart';

class MonitoringPanelSyncConsentDialog extends StatelessWidget {
  const MonitoringPanelSyncConsentDialog({super.key, required this.owner, required this.onClose});
  final PanelSyncConsentController owner;
  final VoidCallback onClose;
  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: owner,
    builder: (context, _) {
      final l = context.l10n, review = owner.review, result = owner.result;
      return GlassDialog(
        key: const ValueKey('panel-policy-dialog'),
        title: l.monitoringPanelPolicyTitle,
        icon: Icons.verified_user_outlined,
        width: 640,
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InfoBanner(message: l.monitoringPanelPolicyScope, tone: BannerTone.warning),
              const SizedBox(height: 8),
              Text(l.monitoringPanelPolicyNoWriter),
              if (review == null && !owner.spent && owner.failure == null) Text(l.monitoringPanelPolicyExplicitRead),
              if (review != null) ...[
                const SizedBox(height: 12),
                MonitoringPanelSyncStatus(snapshot: review.snapshot),
                Text(l.monitoringPanelPolicyAccount(review.accountId)),
                Text(l.monitoringPanelPolicyRevision(review.policyRevision)),
                SelectableText(l.monitoringPanelPolicyRoster(review.rosterDigest)),
                Text(l.monitoringPanelPolicyOffered(review.offered ?? l.commonUnknown)),
                Text(
                  l.monitoringPanelPolicyCheckpoint(
                    review.checkpoint.vaultId,
                    review.checkpoint.epoch,
                    review.checkpoint.sequence,
                  ),
                ),
                Text(
                  l.monitoringPanelPolicyCatalog(
                    review.catalog.settingsId,
                    review.catalog.objectRevision,
                    review.catalog.schema.toString(),
                    review.catalog.revision,
                  ),
                ),
                if (!owner.spent)
                  Text(
                    l.monitoringPanelPolicyRemaining(owner.remainingSeconds.toString()),
                    key: const ValueKey('panel-policy-remaining'),
                  ),
              ],
              if (result != null)
                InfoBanner(
                  key: const ValueKey('panel-policy-result'),
                  message: result.rechecked ? l.monitoringPanelPolicyRecaptured : l.monitoringPanelPolicyConfirmed,
                ),
              if (owner.failure case final reason?)
                InfoBanner(
                  key: const ValueKey('panel-policy-error'),
                  tone: BannerTone.danger,
                  message: switch (reason) {
                    PanelSyncConsentFailure.unavailable => l.monitoringPanelPolicyUnavailable,
                    PanelSyncConsentFailure.invalid => l.monitoringPanelPolicyInvalid,
                    PanelSyncConsentFailure.expired => l.monitoringPanelPolicyExpired,
                    PanelSyncConsentFailure.retired => l.monitoringPanelPolicyRetired,
                    PanelSyncConsentFailure.outcomeUnknown => l.monitoringPanelPolicyUnknownOutcome,
                  },
                ),
              if (owner.busy) const LinearProgressIndicator(key: ValueKey('panel-policy-busy')),
            ],
          ),
        ),
        secondaryActions: [
          GlassButton(key: const ValueKey('panel-policy-close'), label: l.commonClose, onPressed: onClose),
        ],
        primaryAction: owner.review == null && !owner.spent
            ? GlassButton(
                key: const ValueKey('panel-policy-prepare'),
                label: l.monitoringPanelPolicyRead,
                onPressed: owner.current && !owner.busy ? owner.prepare : null,
              )
            : review?.canRecheck == true
            ? GlassButton(
                key: const ValueKey('panel-policy-recheck'),
                label: l.monitoringPanelPolicyRecheck,
                onPressed: owner.canRecheck ? () => owner.act(recheck: true) : null,
              )
            : GlassButton(
                key: const ValueKey('panel-policy-confirm'),
                label: l.monitoringPanelPolicyConfirm,
                onPressed: owner.canConfirm ? () => owner.act(recheck: false) : null,
              ),
      );
    },
  );
}
