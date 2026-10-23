import Flutter
import Foundation
import CoreFoundation
import UIKit

/// Keeps all key material in the Rust core/iOS Keychain; device unlock (S5)
/// is a Secure Enclave key the core asks iOS to use (Face ID / Touch ID /
/// passcode). The channels expose only a private container path, explicit
/// file export and user-requested secret copying with native pasteboard
/// privacy options.
public final class CcBridgePlugin: NSObject, FlutterPlugin {
  private weak var registrar: FlutterPluginRegistrar?

  public static func register(with registrar: FlutterPluginRegistrar) {
    let instance = CcBridgePlugin()
    instance.registrar = registrar
    let kubeBrowser = AuthSessionBrowser(
      registrar: registrar, redirectUri: "consolecrypt://kubernetes/oauth/callback",
      ephemeral: true, allowLoopbackHttp: false)
    let kubeChannel = FlutterMethodChannel(name: "consolecrypt/kube_auth", binaryMessenger: registrar.messenger())
    kubeChannel.setMethodCallHandler { call, result in kubeBrowser.handle(call, result: result) }
    // Enterprise sign-in to the sync server (protocol 1.8): not ephemeral, so
    // an existing session at the organisation's IdP is reused.
    let syncBrowser = AuthSessionBrowser(
      registrar: registrar, redirectUri: "consolecrypt://auth/callback",
      ephemeral: false, allowLoopbackHttp: true)
    let syncChannel = FlutterMethodChannel(name: "consolecrypt/auth_session", binaryMessenger: registrar.messenger())
    syncChannel.setMethodCallHandler { call, result in syncBrowser.handle(call, result: result) }
    for name in ["consolecrypt/ios", "consolecrypt/accessibility", "consolecrypt/clipboard"] {
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
    case "dataDirectory":
      do { result(try privateDataDirectory().path) }
      catch { result(FlutterError(code: "storage", message: "Private app storage unavailable", details: nil)) }
    case "exportFile":
      exportFile(arguments: call.arguments as? [String: Any], result: result)
    case "copySecret":
      copySecret(arguments: call.arguments as? [String: Any], result: result)
    case "copyPrivate":
      guard let text = (call.arguments as? [String: Any])?["text"] as? String else {
        result(FlutterError(code: "bad_args", message: "Invalid clipboard request", details: nil))
        return
      }
      // Device-local, no expiry: terminal selection and query results.
      UIPasteboard.general.setItems([["public.utf8-plain-text": text]], options: [.localOnly: true])
      result(nil)
    case "clearIfOwned":
      clearIfOwned(arguments: call.arguments as? [String: Any], result: result)
    case "forget":
      result(nil)
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
    // Ownership token: the change count right after our write (S8/S9).
    result(String(UIPasteboard.general.changeCount))
  }

  /// Clears our items if the pasteboard has not changed since the copy that
  /// returned [token]; never reads the content (no paste prompt/banner).
  private func clearIfOwned(arguments: [String: Any]?, result: @escaping FlutterResult) {
    guard let token = arguments?["token"] as? String, let count = Int(token) else {
      result(FlutterError(code: "bad_args", message: "Invalid clipboard token", details: nil))
      return
    }
    let pasteboard = UIPasteboard.general
    if pasteboard.changeCount == count {
      pasteboard.items = []
      result("cleared")
    } else {
      result(pasteboard.numberOfItems == 0 ? "gone" : "foreign")
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
