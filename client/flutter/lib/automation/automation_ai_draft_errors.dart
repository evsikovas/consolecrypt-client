import 'package:consolecrypt/automation/automation_ai_draft.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';

String automationAiDraftErrorMessage(AppLocalizations l, AutomationAiDraftException error) => switch (error.code) {
  AutomationAiDraftError.invalidProposal => l.automationAiInvalidResponse,
  AutomationAiDraftError.invalidPath => l.automationPlaybookPathHint,
  AutomationAiDraftError.invalidRequirements ||
  AutomationAiDraftError.dependencyConflict ||
  AutomationAiDraftError.limitExceeded ||
  AutomationAiDraftError.canonicalFilename => l.automationAiRequirementsInvalid,
};
