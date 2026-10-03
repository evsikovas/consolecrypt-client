import Cocoa
import FlutterMacOS
import macos_window_utils

class MainFlutterWindow: NSWindow {
  private var accessibilityBridge: AccessibilityBridge?
  private var updateBridge: UpdateBridge?
  private var fullscreenBridge: FullscreenBridge?

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

    fullscreenBridge = FullscreenBridge(messenger: flutterViewController.engine.binaryMessenger, window: self)

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


/// Reconciles asynchronous AppKit transitions, including a lock or route
/// disposal while entry is still animating. Does not replace NSWindow.delegate.
final class FullscreenBridge: NSObject {
  private let channel: FlutterMethodChannel
  private weak var window: NSWindow?
  private var leased = false
  private var originalFullscreen = false
  private var toolbarWasVisible = false
  private var originalFrame: NSRect?
  private var restorePending = false
  private var target: Bool?
  private var transitioning = false
  private var minimizePending = false
  private var transitionGeneration = 0
  private var attempts = 0

  init(messenger: FlutterBinaryMessenger, window: NSWindow) {
    self.window = window
    channel = FlutterMethodChannel(name: "consolecrypt/window", binaryMessenger: messenger)
    super.init()
    let center = NotificationCenter.default
    for name in [NSWindow.willEnterFullScreenNotification, NSWindow.willExitFullScreenNotification] {
      center.addObserver(self, selector: #selector(willTransition), name: name, object: window)
    }
    for name in [NSWindow.didEnterFullScreenNotification, NSWindow.didExitFullScreenNotification] {
      center.addObserver(self, selector: #selector(didTransition), name: name, object: window)
    }
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self = self, let window = self.window else {
        result(FlutterError(code: "window_unavailable", message: nil, details: nil)); return
      }
      switch call.method {
      case "beginRdpFullscreen":
        if !self.leased {
          self.originalFullscreen = window.styleMask.contains(.fullScreen)
          self.toolbarWasVisible = window.toolbar?.isVisible ?? false
          self.originalFrame = self.originalFullscreen ? nil : window.frame
          self.leased = true
        }
        window.toolbar?.isVisible = false
        self.attempts = 0
        self.target = true
        self.reconcile()
        result(true)
      case "endRdpFullscreen":
        if self.leased {
          self.leased = false
          self.restorePending = true
          self.attempts = 0
          self.target = self.originalFullscreen
          self.reconcile()
        }
        result(nil)
      case "isFullscreen":
        result(window.styleMask.contains(.fullScreen) && !self.transitioning)
      case "isRdpFullscreenRestored":
        result(!self.leased && !self.transitioning && !self.restorePending)
      case "isRdpMinimized":
        result(!self.leased && !self.transitioning && !self.restorePending
          && !self.minimizePending && window.isMiniaturized)
      case "minimize":
        self.attempts = 0
        self.minimizePending = true
        self.target = false
        self.reconcile()
        result(nil)
      default: result(FlutterMethodNotImplemented)
      }
    }
  }

  deinit { NotificationCenter.default.removeObserver(self) }

  @objc private func willTransition(_ notification: Notification) {
    if leased && target == nil && notification.name == NSWindow.willExitFullScreenNotification {
      originalFullscreen = false // Honour a user's OS-level exit as well.
    }
    armTransitionWatchdog()
  }

  private func armTransitionWatchdog() {
    transitioning = true
    transitionGeneration += 1
    let generation = transitionGeneration
    // AppKit reports failures to its existing delegate, not notifications.
    // Recover a missing did-transition without taking over the plugin delegate.
    DispatchQueue.main.asyncAfter(deadline: .now() + 6) { [weak self] in
      guard let self = self, self.transitioning, self.transitionGeneration == generation else { return }
      self.transitioning = false
      self.reconcile()
    }
  }

  @objc private func didTransition(_ notification: Notification) {
    transitioning = false
    transitionGeneration += 1
    reconcile()
  }

  private func reconcile() {
    guard !transitioning, let window = window else { return }
    if let desired = target, window.styleMask.contains(.fullScreen) != desired {
      guard attempts < 2 else { target = nil; minimizePending = false; return }
      attempts += 1
      armTransitionWatchdog()
      window.toggleFullScreen(nil)
      return
    }
    target = nil // External OS changes remain under the user's control.
    if restorePending {
      // Hiding a toolbar can resize a normal window before AppKit saves its
      // fullscreen restore frame. Restore our original frame after the exit.
      window.toolbar?.isVisible = toolbarWasVisible
      if let frame = originalFrame, !window.styleMask.contains(.fullScreen) {
        window.setFrame(frame, display: true)
      }
      restorePending = false
      originalFrame = nil
    }
    if minimizePending {
      minimizePending = false
      window.miniaturize(nil)
    }
  }
}
