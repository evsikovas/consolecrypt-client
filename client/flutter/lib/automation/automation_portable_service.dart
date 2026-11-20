import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';

/// Optional capability only. The concrete registered Native gateway must be
/// reviewed separately; missing support exposes no export/inspection action.
abstract interface class AutomationPortableGatewayCapability {
  PortableNativeGateway createPortableGateway();
}
