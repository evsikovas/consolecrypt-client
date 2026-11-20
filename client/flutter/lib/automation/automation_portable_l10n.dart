import 'package:consolecrypt/automation/automation_portable_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_host.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';

PortableHostLabels portableHostLabels(AppLocalizations l, {bool importMode = false}) => PortableHostLabels(
  preparing: l.automationPortablePreparing,
  unavailable: l.automationPortableUnavailable,
  dialog: PortableDialogLabels(
    exportTitle: l.automationPortableExportTitle,
    importTitle: l.automationPortableInspectTitle,
    separatePassword: l.automationPortablePassword,
    confirmPassword: l.automationPortableConfirmPassword,
    warning: importMode ? l.automationPortableInspectWarning : l.automationPortableWarning,
    knownExcluded: l.automationPortableExcluded,
    wholeProject: l.automationPortableWholeProject,
    chooseFiles: l.automationPortableChooseFiles,
    sizeLimit: l.automationPortableSizeLimit,
    contents: (files, bytes) => l.automationPortableContents(files, formatBytes(l, bytes)),
    export: l.automationPortableExport,
    inspect: l.automationPortableInspect,
    cancel: l.commonCancel,
    success: l.automationPortableSaved,
    failed: l.automationPortableFailed,
    publicationBlocked: l.automationPortableInspectionOnly,
    summary: l.automationPortableSummary,
  ),
);
