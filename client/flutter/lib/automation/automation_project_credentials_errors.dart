import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/validation.dart';
import 'package:consolecrypt/core/services/errors.dart';

/// Fixed Native reason codes only; never retain/display exception prose or refs.
enum ProjectCredentialsIssue { required, changed, missing, shared, schema, busy }

ProjectCredentialsIssue? projectCredentialsIssueForRule(String rule) => switch (rule) {
  'project_environment_selection_required' => ProjectCredentialsIssue.required,
  'project_environment_selection_changed' => ProjectCredentialsIssue.changed,
  'project_environment_credential_missing' => ProjectCredentialsIssue.missing,
  'project_environment_shared_override_unsupported' => ProjectCredentialsIssue.shared,
  'project_environment_schema_read_only' => ProjectCredentialsIssue.schema,
  'project_environment_prepare_busy' => ProjectCredentialsIssue.busy,
  _ => null,
};
ProjectCredentialsIssue? projectCredentialsIssue(Object failure) => switch (failure) {
  ValidationError(field: 'automation_credentials', :final reason) => projectCredentialsIssueForRule(reason),
  AppException(code: AppErrorCode.validation, reason: AppErrorReason.invalidField, args: final args)
      when args['field'] == 'automation_credentials' =>
    projectCredentialsIssueForRule(args['rule'] ?? ''),
  _ => null,
};
String projectCredentialsIssueMessage(AppLocalizations l, ProjectCredentialsIssue? issue) => switch (issue) {
  ProjectCredentialsIssue.required => l.automationDeploymentErrorRequired,
  ProjectCredentialsIssue.changed => l.automationDeploymentErrorChanged,
  ProjectCredentialsIssue.missing => l.automationDeploymentErrorMissing,
  ProjectCredentialsIssue.shared => l.automationDeploymentErrorShared,
  ProjectCredentialsIssue.schema => l.automationDeploymentErrorSchema,
  ProjectCredentialsIssue.busy => l.automationDeploymentErrorBusy,
  null => l.automationDeploymentUnavailable,
};
