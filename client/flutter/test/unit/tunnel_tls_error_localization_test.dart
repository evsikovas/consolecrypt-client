import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  for (final AppLocalizations l in [AppLocalizationsEn(), AppLocalizationsRu()]) {
    test('closed TLS expiry and uncertain custody remain unavailable in ${l.localeName}', () {
      String message(String rule, {String field = 'agent_tunnel'}) => errorMessage(
        l,
        AppException(
          AppErrorCode.validation,
          'UNTRUSTED diagnostic PRIVATE KEY must never become UI text',
          reason: AppErrorReason.invalidField,
          args: {'field': field, 'rule': rule},
        ),
      );
      expect(message('tls_identity_expired'), l.agentTunnelTlsExpired);
      expect(message('tls_identity_unavailable'), l.agentTunnelTlsUnavailable);
      expect(message('tls_identity_unknown'), l.agentTunnelTlsUnavailable);
      expect(message('tls_identity_future'), l.validationInvalid('agent_tunnel'));
      expect(message('tls_identity_expired', field: 'other'), l.validationInvalid('other'));
      expect(message('tls_identity_expired'), isNot(contains('PRIVATE KEY')));
      expect(errorMessage(l, const AppException(AppErrorCode.vaultKeyMismatch, 'diagnostic')), l.errorVaultKeyMismatch);
    });
  }
}
