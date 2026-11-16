import 'package:consolecrypt/automation/automation_portable_import_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';

PortableImportLabels portableImportLabels(AppLocalizations l) => PortableImportLabels(
  title: l.automationPortableImportTitle,
  warning: l.automationPortableImportWarning,
  chooseFile: l.automationPortableImportChooseFile,
  noFile: l.automationPortableImportNoFile,
  name: l.automationPortableImportName,
  password: l.automationPortablePassword,
  prepare: l.automationPortableImportPrepare,
  accept: l.automationPortableImportAccept,
  cancel: l.commonCancel,
  close: l.commonClose,
  edit: l.commonEdit,
  preparing: l.automationPortableImportPreparing,
  accepting: l.automationPortableImportAccepting,
  previewOnly: l.automationPortableImportPreviewOnly,
  normalizedName: l.automationPortableImportNormalizedName,
  futureId: l.automationPortableImportFutureId,
  limitedReview: l.automationPortableImportLimitedReview,
  installed: l.automationPortableImportInstalled,
  summary: l.automationPortableSummary,
  issue: (issue) => switch (issue) {
    PortableImportIssue.unavailable => l.automationPortableImportUnavailable,
    PortableImportIssue.invalidInput => l.automationPortableImportInvalidInput,
    PortableImportIssue.changed || PortableImportIssue.cancelled => l.automationPortableImportChanged,
    PortableImportIssue.sourceChanged => l.automationPortableImportSourceChanged,
    PortableImportIssue.unsupported => l.automationPortableImportUnsupported,
    PortableImportIssue.busy => l.automationPortableImportBusy,
    PortableImportIssue.expired => l.automationPortableImportExpired,
    PortableImportIssue.response => l.automationPortableImportResponse,
    PortableImportIssue.failed => l.automationPortableImportFailed,
    PortableImportIssue.recoveryRequired => l.automationPortableImportRecoveryRequired,
  },
  publicationReason: (reason) => switch (reason) {
    'publication_platform_unsupported' => l.automationPortableImportPlatformUnsupported,
    'publication_location_unavailable' => l.automationPortableImportLocationUnavailable,
    'package_tree_unsupported' => l.automationPortableImportTreeUnsupported,
    'import_capacity_reached' => l.automationPortableImportCapacityReached,
    _ => l.automationPortableImportUnsupported,
  },
);
