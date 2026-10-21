import FlutterMacOS
import Foundation

/// Plugin entry of the Rust core pod. Device unlock (S5) runs inside the
/// core: a Secure Enclave key released only after Touch ID or the account
/// password (`LAContext` prompt from Rust). No method channel here: the UI
/// cannot hand the core a "user authenticated" grant.
public final class CcBridgePlugin: NSObject, FlutterPlugin {
  public static func register(with registrar: FlutterPluginRegistrar) {}
}
