import FlutterMacOS
import Foundation
import LocalAuthentication

/// `consolecrypt/local_auth`: Touch ID / device-password prompt for
/// "Unlock with Touch ID" (ADR-0101 §8, ADR-0107 OS authentication).
///
/// * `availability` → `{"kind": "touch_id" | "device_credential" | null,
///   "not_enrolled": Bool}`
/// * `authenticate({"reason": String})` → `Bool` (false on failure / cancel)
///
/// The result is only a user-presence check: the vault key stays in the
/// Rust core behind the device envelope.
public final class CcBridgePlugin: NSObject, FlutterPlugin {
  public static func register(with registrar: FlutterPluginRegistrar) {
    let channel = FlutterMethodChannel(
      name: "consolecrypt/local_auth", binaryMessenger: registrar.messenger)
    let instance = CcBridgePlugin()
    registrar.addMethodCallDelegate(instance, channel: channel)
  }

  public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    switch call.method {
    case "availability":
      result(availability())
    case "authenticate":
      let args = call.arguments as? [String: Any]
      let reason = (args?["reason"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        ?? "Unlock your ConsoleCrypt vault"
      authenticate(reason: reason, result: result)
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func availability() -> [String: Any] {
    let context = LAContext()
    var error: NSError?
    if context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) {
      return ["kind": "touch_id", "not_enrolled": false]
    }
    let notEnrolled = (error as? LAError)?.code == .biometryNotEnrolled
    var fallbackError: NSError?
    if context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &fallbackError) {
      return ["kind": "device_credential", "not_enrolled": notEnrolled]
    }
    return ["kind": NSNull(), "not_enrolled": notEnrolled]
  }

  private func authenticate(reason: String, result: @escaping FlutterResult) {
    let context = LAContext()
    // Biometrics with the account password as fallback.
    context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { success, _ in
      DispatchQueue.main.async { result(success) }
    }
  }
}
