import Flutter
import UIKit

class SceneDelegate: FlutterSceneDelegate {
  private var privacyCover: UIView?

  override func sceneWillResignActive(_ scene: UIScene) {
    super.sceneWillResignActive(scene)
    guard let window = window, privacyCover == nil else { return }
    // iOS has no supported API to ban screenshots. Hide app-switcher snapshots
    // so terminal output and vault screens do not remain in the task switcher.
    let cover = UIView(frame: window.bounds)
    cover.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    cover.backgroundColor = UIColor(red: 0.055, green: 0.07, blue: 0.10, alpha: 1)
    cover.accessibilityElementsHidden = true
    window.addSubview(cover)
    privacyCover = cover
  }

  override func sceneDidBecomeActive(_ scene: UIScene) {
    super.sceneDidBecomeActive(scene)
    privacyCover?.removeFromSuperview()
    privacyCover = nil
  }
}
