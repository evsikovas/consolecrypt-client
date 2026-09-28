//
//  Generated file. Do not edit.
//

import FlutterMacOS
import Foundation

import cc_bridge
import desktop_drop
import file_selector_macos
import macos_window_utils

func RegisterGeneratedPlugins(registry: FlutterPluginRegistry) {
  CcBridgePlugin.register(with: registry.registrar(forPlugin: "CcBridgePlugin"))
  DesktopDropPlugin.register(with: registry.registrar(forPlugin: "DesktopDropPlugin"))
  FileSelectorPlugin.register(with: registry.registrar(forPlugin: "FileSelectorPlugin"))
  MacOSWindowUtilsPlugin.register(with: registry.registrar(forPlugin: "MacOSWindowUtilsPlugin"))
}
