import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:material_ui/material_ui.dart';

/// Disclosure for the one shipped cAdvisor source; never interprets user YAML.
/// Acceptance only permits adding that source, never installation or a grant.
class AutomationCadvisorReview extends StatelessWidget {
  const AutomationCadvisorReview({super.key});
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return AlertDialog(
      key: const ValueKey('automation-cadvisor-review'),
      title: Text(l.automationCadvisorReviewTitle),
      content: SizedBox(
        width: 620,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              InfoBanner(tone: BannerTone.warning, message: l.automationCadvisorHostAccess),
              const SizedBox(height: 12),
              // l10n-ignore: exact fixed Docker flags and mounts from the shipped template.
              const SelectableText( // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                '--privileged\n--device=/dev/kmsg\n' // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                '--volume=/:/rootfs:ro\n--volume=/var/run:/var/run:ro\n' // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                '--volume=/sys:/sys:ro\n--volume=/var/lib/docker:/var/lib/docker:ro\n' // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                '--volume=/dev/disk:/dev/disk:ro\n' // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                '--publish=127.0.0.1:<cc_cadvisor_port>:8080\n--restart=unless-stopped', // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
              ),
              const SizedBox(height: 12),
              Text(l.automationCadvisorDockerAccess),
              const SizedBox(height: 12),
              Text(l.automationCadvisorSourceOnly),
              const SizedBox(height: 12),
              Text(l.automationCadvisorDeclaration),
              // l10n-ignore: literal source variable and value, not an authority flag.
              const SelectableText('cc_cadvisor_host_access: privileged-host-filesystems-and-kmsg'), // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('automation-cadvisor-cancel'),
          onPressed: () => Navigator.pop(context, false),
          child: Text(l.commonCancel),
        ),
        TextButton(
          key: const ValueKey('automation-cadvisor-add-source'),
          onPressed: () => Navigator.pop(context, true),
          child: Text(l.automationCadvisorAddSource),
        ),
      ],
    );
  }
}
