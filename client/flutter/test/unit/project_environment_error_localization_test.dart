import 'package:consolecrypt/automation/automation_project_credentials_errors.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/models/validation.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  for (final AppLocalizations l in [AppLocalizationsEn(), AppLocalizationsRu()]) {
    test('six fixed project reasons are actionable and private prose stays hidden in ${l.localeName}', () {
      final expected = {
        'project_environment_selection_required': l.automationDeploymentErrorRequired,
        'project_environment_selection_changed': l.automationDeploymentErrorChanged,
        'project_environment_credential_missing': l.automationDeploymentErrorMissing,
        'project_environment_shared_override_unsupported': l.automationDeploymentErrorShared,
        'project_environment_schema_read_only': l.automationDeploymentErrorSchema,
        'project_environment_prepare_busy': l.automationDeploymentErrorBusy,
      };
      for (final entry in expected.entries) {
        final failure = AppException(
          AppErrorCode.validation,
          'DO_NOT_RENDER_PRIVATE_TOKEN',
          reason: AppErrorReason.invalidField,
          args: {'field': 'automation_credentials', 'rule': entry.key},
        );
        expect(errorMessage(l, failure), entry.value);
        expect(projectCredentialsIssueMessage(l, projectCredentialsIssue(failure)), entry.value);
        expect(
          projectCredentialsIssueMessage(
            l,
            projectCredentialsIssue(ValidationError('automation_credentials', entry.key)),
          ),
          entry.value,
        );
        expect(errorMessage(l, failure), isNot(contains('DO_NOT_RENDER')));
      }
      expect(
        validationMessage(l, 'automation_credentials', 'unrecognized raw secret'),
        l.automationDeploymentUnavailable,
      );
      expect(projectCredentialsIssue(const ValidationError('other', 'project_environment_prepare_busy')), isNull);
      expect(errorMessage(l, const AppException(AppErrorCode.vaultKeyMismatch, 'raw')), l.errorVaultKeyMismatch);
    });
  }
}
