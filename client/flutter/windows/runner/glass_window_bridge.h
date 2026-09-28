#ifndef RUNNER_GLASS_WINDOW_BRIDGE_H_
#define RUNNER_GLASS_WINDOW_BRIDGE_H_

#include <flutter/binary_messenger.h>
#include <flutter/encodable_value.h>
#include <flutter/method_channel.h>
#include <windows.h>

#include <memory>

// Native side of the Liquid Glass kit on Windows (LIQUID_GLASS_SPEC §6.7,
// §6.8):
//
// * `consolecrypt/accessibility` — Dart calls `getSignals`; the runner sends
//   `signalsChanged` when high contrast, "Transparency effects", "Animation
//   effects", a Remote Desktop session or battery saver change.
// * `consolecrypt/window` — Dart calls `setCaptionColors` with ARGB ints so
//   the caption blends into the ambient backdrop (Windows 11 22000+; the
//   HRESULT is ignored on older builds).
//
// Mica / acrylic behind a transparent Flutter surface is deliberately not
// enabled: it is unverified with Impeller (see the spec's implementation
// notes). The window stays opaque and the app paints its own backdrop.
class GlassWindowBridge {
 public:
  // OS display/accessibility state sent to Dart.
  struct Signals {
    bool reduce_transparency = false;
    bool increase_contrast = false;
    bool reduce_motion = false;
    bool remote_session = false;
    bool battery_saver = false;

    bool operator==(const Signals& other) const {
      return reduce_transparency == other.reduce_transparency &&
             increase_contrast == other.increase_contrast &&
             reduce_motion == other.reduce_motion &&
             remote_session == other.remote_session &&
             battery_saver == other.battery_saver;
    }
    bool operator!=(const Signals& other) const { return !(*this == other); }
  };

  GlassWindowBridge(flutter::BinaryMessenger* messenger, HWND window);
  ~GlassWindowBridge();

  GlassWindowBridge(const GlassWindowBridge&) = delete;
  GlassWindowBridge& operator=(const GlassWindowBridge&) = delete;

  // Called for every top-level window message; never consumes it.
  void OnWindowMessage(UINT message);

 private:
  static Signals ReadSignals();
  static flutter::EncodableMap ToMap(const Signals& signals);
  void SendSignalsIfChanged();

  HWND window_;
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>>
      accessibility_channel_;
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>>
      window_channel_;
  Signals last_signals_;
};

#endif  // RUNNER_GLASS_WINDOW_BRIDGE_H_
