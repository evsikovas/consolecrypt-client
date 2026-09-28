#include "glass_window_bridge.h"

#include <dwmapi.h>
#include <flutter/standard_method_codec.h>

#include <cstdint>
#include <variant>

namespace {

constexpr char kAccessibilityChannel[] = "consolecrypt/accessibility";
constexpr char kWindowChannel[] = "consolecrypt/window";

// DWMWINDOWATTRIBUTE values (Windows 11 build 22000+), spelled out so older
// SDKs still build. Older Windows versions return an error, which we ignore.
constexpr DWORD kDwmCaptionColor = 35;
constexpr DWORD kDwmTextColor = 36;

constexpr wchar_t kPersonalizeKey[] =
    L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
constexpr wchar_t kEnableTransparencyValue[] = L"EnableTransparency";

bool HighContrastOn() {
  HIGHCONTRAST contrast{};
  contrast.cbSize = static_cast<UINT>(sizeof(contrast));
  if (!SystemParametersInfo(SPI_GETHIGHCONTRAST,
                            static_cast<UINT>(sizeof(contrast)), &contrast,
                            0)) {
    return false;
  }
  return (contrast.dwFlags & HCF_HIGHCONTRASTON) != 0;
}

// Settings > Personalization > Colors > Transparency effects.
bool TransparencyEffectsOn() {
  DWORD value = 1;
  DWORD size = static_cast<DWORD>(sizeof(value));
  const LSTATUS status =
      RegGetValue(HKEY_CURRENT_USER, kPersonalizeKey, kEnableTransparencyValue,
                  RRF_RT_REG_DWORD, nullptr, &value, &size);
  return status != ERROR_SUCCESS || value != 0;
}

// Settings > Accessibility > Visual effects > Animation effects.
bool ClientAreaAnimationsOn() {
  BOOL enabled = TRUE;
  if (!SystemParametersInfo(SPI_GETCLIENTAREAANIMATION, 0, &enabled, 0)) {
    return true;
  }
  return enabled != FALSE;
}

bool BatterySaverOn() {
  SYSTEM_POWER_STATUS status{};
  if (!GetSystemPowerStatus(&status)) {
    return false;
  }
  return status.SystemStatusFlag == 1;
}

COLORREF ToColorRef(int64_t argb) {
  const BYTE red = static_cast<BYTE>((argb >> 16) & 0xFF);
  const BYTE green = static_cast<BYTE>((argb >> 8) & 0xFF);
  const BYTE blue = static_cast<BYTE>(argb & 0xFF);
  return RGB(red, green, blue);
}

bool ReadColor(const flutter::EncodableMap& map, const char* key,
               COLORREF* out) {
  const auto it = map.find(flutter::EncodableValue(key));
  if (it == map.end()) {
    return false;
  }
  const flutter::EncodableValue& value = it->second;
  if (const auto* narrow = std::get_if<int32_t>(&value)) {
    *out = ToColorRef(static_cast<int64_t>(*narrow));
    return true;
  }
  if (const auto* wide = std::get_if<int64_t>(&value)) {
    *out = ToColorRef(*wide);
    return true;
  }
  return false;
}

}  // namespace

GlassWindowBridge::GlassWindowBridge(flutter::BinaryMessenger* messenger,
                                     HWND window)
    : window_(window) {
  const auto* codec = &flutter::StandardMethodCodec::GetInstance();

  accessibility_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          messenger, kAccessibilityChannel, codec);
  accessibility_channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
             std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
                 result) {
        if (call.method_name() == "getSignals") {
          last_signals_ = ReadSignals();
          result->Success(flutter::EncodableValue(ToMap(last_signals_)));
        } else {
          result->NotImplemented();
        }
      });

  window_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          messenger, kWindowChannel, codec);
  window_channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
             std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
                 result) {
        if (call.method_name() != "setCaptionColors") {
          result->NotImplemented();
          return;
        }
        const auto* args =
            std::get_if<flutter::EncodableMap>(call.arguments());
        COLORREF caption = 0;
        COLORREF text = 0;
        if (args == nullptr || !ReadColor(*args, "caption", &caption) ||
            !ReadColor(*args, "text", &text)) {
          result->Error("bad_args", "Expected {caption, text} ARGB ints.");
          return;
        }
        DwmSetWindowAttribute(window_, kDwmCaptionColor, &caption,
                              static_cast<DWORD>(sizeof(caption)));
        DwmSetWindowAttribute(window_, kDwmTextColor, &text,
                              static_cast<DWORD>(sizeof(text)));
        result->Success();
      });

  last_signals_ = ReadSignals();
}

GlassWindowBridge::~GlassWindowBridge() {
  // The handlers capture `this`; unregister before the engine outlives us.
  if (accessibility_channel_) {
    accessibility_channel_->SetMethodCallHandler(nullptr);
  }
  if (window_channel_) {
    window_channel_->SetMethodCallHandler(nullptr);
  }
}

void GlassWindowBridge::OnWindowMessage(UINT message) {
  switch (message) {
    case WM_SETTINGCHANGE:
    case WM_THEMECHANGED:
    case WM_SYSCOLORCHANGE:
    case WM_POWERBROADCAST:
    case WM_DISPLAYCHANGE:
    case WM_DWMCOMPOSITIONCHANGED:
      SendSignalsIfChanged();
      break;
    default:
      break;
  }
}

// static
GlassWindowBridge::Signals GlassWindowBridge::ReadSignals() {
  Signals signals;
  signals.reduce_transparency = !TransparencyEffectsOn();
  signals.increase_contrast = HighContrastOn();
  signals.reduce_motion = !ClientAreaAnimationsOn();
  signals.remote_session = GetSystemMetrics(SM_REMOTESESSION) != 0;
  signals.battery_saver = BatterySaverOn();
  return signals;
}

// static
flutter::EncodableMap GlassWindowBridge::ToMap(const Signals& signals) {
  flutter::EncodableMap map;
  map[flutter::EncodableValue("reduceTransparency")] =
      flutter::EncodableValue(signals.reduce_transparency);
  map[flutter::EncodableValue("increaseContrast")] =
      flutter::EncodableValue(signals.increase_contrast);
  map[flutter::EncodableValue("reduceMotion")] =
      flutter::EncodableValue(signals.reduce_motion);
  map[flutter::EncodableValue("differentiateWithoutColor")] =
      flutter::EncodableValue(false);
  map[flutter::EncodableValue("remoteSession")] =
      flutter::EncodableValue(signals.remote_session);
  map[flutter::EncodableValue("batterySaver")] =
      flutter::EncodableValue(signals.battery_saver);
  return map;
}

void GlassWindowBridge::SendSignalsIfChanged() {
  const Signals signals = ReadSignals();
  if (signals == last_signals_) {
    return;
  }
  last_signals_ = signals;
  accessibility_channel_->InvokeMethod(
      "signalsChanged",
      std::make_unique<flutter::EncodableValue>(ToMap(signals)));
}
