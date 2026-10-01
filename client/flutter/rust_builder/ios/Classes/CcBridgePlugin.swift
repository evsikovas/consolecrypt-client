import Flutter
import Foundation
import CoreFoundation
import LocalAuthentication
import UIKit

/// Keeps all key material in the Rust core/iOS Keychain. The channels expose
/// only user presence, a private container path, explicit file export and
/// user-requested secret copying with native pasteboard privacy options.
public final class CcBridgePlugin: NSObject, FlutterPlugin {
  private weak var registrar: FlutterPluginRegistrar?
  private var authentication: LAContext?
  private var authenticationResult: FlutterResult?

  public static func register(with registrar: FlutterPluginRegistrar) {
    let instance = CcBridgePlugin()
    instance.registrar = registrar
    for name in ["consolecrypt/local_auth", "consolecrypt/ios", "consolecrypt/accessibility", "consolecrypt/clipboard"] {
      let channel = FlutterMethodChannel(name: name, binaryMessenger: registrar.messenger())
      if name == "consolecrypt/accessibility" {
        channel.setMethodCallHandler { call, result in
          if call.method == "getSignals" { result(instance.accessibilitySignals()) }
          else { result(FlutterMethodNotImplemented) }
        }
      } else {
        registrar.addMethodCallDelegate(instance, channel: channel)
      }
    }
  }

  public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    switch call.method {
    case "availability":
      result(availability())
    case "authenticate":
      let arguments = call.arguments as? [String: Any]
      let supplied = arguments?["reason"] as? String
      authenticate(reason: supplied?.isEmpty == false ? supplied! : "Unlock your ConsoleCrypt vault", result: result)
    case "dataDirectory":
      do { result(try privateDataDirectory().path) }
      catch { result(FlutterError(code: "storage", message: "Private app storage unavailable", details: nil)) }
    case "exportFile":
      exportFile(arguments: call.arguments as? [String: Any], result: result)
    case "copySecret":
      copySecret(arguments: call.arguments as? [String: Any], result: result)
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func copySecret(arguments: [String: Any]?, result: @escaping FlutterResult) {
    guard let text = arguments?["text"] as? String,
          let milliseconds = arguments?["clearAfterMilliseconds"] as? NSNumber,
          CFGetTypeID(milliseconds) != CFBooleanGetTypeID(),
          milliseconds.doubleValue.isFinite,
          milliseconds.doubleValue >= 1, milliseconds.doubleValue <= 86_400_000,
          milliseconds.doubleValue == Double(milliseconds.int64Value) else {
      result(FlutterError(code: "bad_args", message: "Invalid secret clipboard request", details: nil))
      return
    }
    // System expiry survives suspension/termination, while localOnly prevents
    // Handoff to another device. Plain public copies retain Flutter's default.
    UIPasteboard.general.setItems([["public.utf8-plain-text": text]], options: [
      .localOnly: true,
      .expirationDate: Date().addingTimeInterval(milliseconds.doubleValue / 1_000),
    ])
    result(nil)
  }

  private func availability() -> [String: Any] {
    let context = LAContext()
    var error: NSError?
    if context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) {
      return ["kind": context.biometryType == .faceID ? "face_id" : "touch_id", "not_enrolled": false]
    }
    let notEnrolled = (error as? LAError)?.code == .biometryNotEnrolled
    if context.canEvaluatePolicy(.deviceOwnerAuthentication, error: nil) {
      return ["kind": "device_credential", "not_enrolled": notEnrolled]
    }
    return ["kind": NSNull(), "not_enrolled": notEnrolled]
  }

  private func authenticate(reason: String, result: @escaping FlutterResult) {
    guard authentication == nil else { result(false); return }
    let context = LAContext()
    guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: nil) else {
      result(false)
      return
    }
    // Every call uses a new context; no biometric reuse window or cached grant.
    context.touchIDAuthenticationAllowableReuseDuration = 0
    authentication = context
    authenticationResult = result
    context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { [weak self] success, _ in
      DispatchQueue.main.async {
        guard let self = self, self.authentication === context else { return }
        let completion = self.authenticationResult
        self.authenticationResult = nil
        self.authentication = nil
        context.invalidate()
        completion?(success)
      }
    }
  }

  private func privateDataDirectory() throws -> URL {
    let manager = FileManager.default
    let support = try manager.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                  appropriateFor: nil, create: true)
    var root = support.appendingPathComponent("ConsoleCrypt", isDirectory: true)
    try manager.createDirectory(at: root, withIntermediateDirectories: true, attributes: [
      .posixPermissions: 0o700,
      .protectionKey: FileProtectionType.complete,
    ])
    // Existing directories must retain the policy after application updates.
    try manager.setAttributes([.posixPermissions: 0o700,
                               .protectionKey: FileProtectionType.complete], ofItemAtPath: root.path)
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    try root.setResourceValues(values)
    return root
  }

  private func exportFile(arguments: [String: Any]?, result: @escaping FlutterResult) {
    guard let path = arguments?["path"] as? String else {
      result(FlutterError(code: "export_path", message: "Invalid export file", details: nil))
      return
    }
    do {
      let root = try privateDataDirectory().resolvingSymlinksInPath().standardizedFileURL
      let source = URL(fileURLWithPath: path).resolvingSymlinksInPath().standardizedFileURL
      let attributes = try FileManager.default.attributesOfItem(atPath: source.path)
      guard source.path.hasPrefix(root.path + "/"), attributes[.type] as? FileAttributeType == .typeRegular else {
        result(FlutterError(code: "export_path", message: "Invalid export file", details: nil))
        return
      }
      guard let controller = registrar?.viewController else {
        result(FlutterError(code: "export_unavailable", message: "Document picker unavailable", details: nil))
        return
      }
      guard controller.presentedViewController == nil else {
        result(FlutterError(code: "busy", message: "A document dialog is already open", details: nil))
        return
      }
      let picker = UIActivityViewController(activityItems: [source], applicationActivities: nil)
      picker.completionWithItemsHandler = { _, completed, _, _ in result(completed) }
      picker.popoverPresentationController?.sourceView = controller.view
      picker.popoverPresentationController?.sourceRect = CGRect(x: controller.view.bounds.midX,
                                                               y: controller.view.bounds.midY,
                                                               width: 1, height: 1)
      controller.present(picker, animated: true)
    } catch {
      result(FlutterError(code: "export_path", message: "Invalid export file", details: nil))
    }
  }

  private func accessibilitySignals() -> [String: Bool] {
    return [
      "reduceMotion": UIAccessibility.isReduceMotionEnabled,
      "reduceTransparency": UIAccessibility.isReduceTransparencyEnabled,
      "increaseContrast": UIAccessibility.isDarkerSystemColorsEnabled,
      "differentiateWithoutColor": UIAccessibility.shouldDifferentiateWithoutColor,
      "batterySaver": ProcessInfo.processInfo.isLowPowerModeEnabled,
    ]
  }
}
