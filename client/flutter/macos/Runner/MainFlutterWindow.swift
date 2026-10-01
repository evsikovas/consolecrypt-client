import Cocoa
import FlutterMacOS
import macos_window_utils

class MainFlutterWindow: NSWindow {
  private var accessibilityBridge: AccessibilityBridge?
  private var updateBridge: UpdateBridge?

  override func awakeFromNib() {
    // macos_window_utils (MIT) hosts the Flutter view so the Dart side can
    // switch to the transparent, full-size-content unified title bar
    // (LIQUID_GLASS_SPEC §6.7). The window stays opaque: glass samples the
    // in-app ambient backdrop, never the desktop.
    let visibleFrame = (self.screen ?? NSScreen.main)?.visibleFrame ?? self.frame
    let initialSize = NSSize(
      width: min(1440, max(1, visibleFrame.width - 32)),
      height: min(900, max(1, visibleFrame.height - 32)))
    let windowFrame = NSRect(
      x: visibleFrame.midX - initialSize.width / 2,
      y: visibleFrame.midY - initialSize.height / 2,
      width: initialSize.width,
      height: initialSize.height)
    let windowUtilsViewController = MacOSWindowUtilsViewController()
    self.contentViewController = windowUtilsViewController
    self.setFrame(windowFrame, display: true)
    self.minSize = NSSize(width: min(960, initialSize.width), height: min(600, initialSize.height))

    MainFlutterWindowManipulator.start(mainFlutterWindow: self)

    let flutterViewController = windowUtilsViewController.flutterViewController
    RegisterGeneratedPlugins(registry: flutterViewController)
    accessibilityBridge = AccessibilityBridge(messenger: flutterViewController.engine.binaryMessenger)
    updateBridge = UpdateBridge(messenger: flutterViewController.engine.binaryMessenger, window: self)

    super.awakeFromNib()
  }
}

/// `consolecrypt/accessibility` (LIQUID_GLASS_SPEC §6.8): Flutter does not
/// expose Reduce Transparency, Increase Contrast or Reduce Motion on macOS.
///
/// * Dart → native `getSignals` returns the current flags.
/// * native → Dart `signalsChanged` is sent whenever the display
///   accessibility options or the power state (Low Power Mode) change.
final class AccessibilityBridge: NSObject {
  private let channel: FlutterMethodChannel

  init(messenger: FlutterBinaryMessenger) {
    channel = FlutterMethodChannel(name: "consolecrypt/accessibility", binaryMessenger: messenger)
    super.init()
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self = self else {
        result(FlutterMethodNotImplemented)
        return
      }
      switch call.method {
      case "getSignals":
        result(self.signals())
      default:
        result(FlutterMethodNotImplemented)
      }
    }
    NSWorkspace.shared.notificationCenter.addObserver(
      self,
      selector: #selector(signalsDidChange),
      name: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
      object: nil)
    NotificationCenter.default.addObserver(
      self,
      selector: #selector(signalsDidChange),
      name: Notification.Name.NSProcessInfoPowerStateDidChange,
      object: nil)
  }

  deinit {
    NSWorkspace.shared.notificationCenter.removeObserver(self)
    NotificationCenter.default.removeObserver(self)
  }

  private func signals() -> [String: Bool] {
    let workspace = NSWorkspace.shared
    return [
      "reduceTransparency": workspace.accessibilityDisplayShouldReduceTransparency,
      "increaseContrast": workspace.accessibilityDisplayShouldIncreaseContrast,
      "reduceMotion": workspace.accessibilityDisplayShouldReduceMotion,
      "differentiateWithoutColor": workspace.accessibilityDisplayShouldDifferentiateWithoutColor,
      // Screen Sharing / remote sessions are not cheaply detectable on macOS.
      "remoteSession": false,
      "batterySaver": ProcessInfo.processInfo.isLowPowerModeEnabled,
    ]
  }

  @objc private func signalsDidChange(_ notification: Notification) {
    DispatchQueue.main.async { [weak self] in
      guard let self = self else { return }
      self.channel.invokeMethod("signalsChanged", arguments: self.signals())
    }
  }
}
