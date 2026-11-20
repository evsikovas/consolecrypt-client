import 'package:consolecrypt/automation/automation_service.dart';

/// Fixed Native service purpose. Observations and UI choices grant no authority.
abstract interface class AutomationServiceDelegationService {
  Future<AutomationJson> prepareServiceDelegation(AutomationJson request);
  Future<AutomationJson> confirmServiceDelegation(AutomationJson request);
  Future<AutomationJson> cancelServiceDelegation(AutomationJson request);
  Future<AutomationJson> serviceDelegationStatus(AutomationJson request);
  Future<AutomationJson> revokeServiceDelegation(AutomationJson request);
}

/// Optional fixed read-only observation; does not connect a Helper or grant jobs.
abstract interface class AutomationServiceResultsService {
  Future<AutomationJson> serviceDelegationResults(AutomationJson request);
}
