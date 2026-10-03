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
        if (call.method_name() == "beginRdpFullscreen") {
          result->Success(flutter::EncodableValue(BeginFullscreen()));
          return;
        }
        if (call.method_name() == "endRdpFullscreen") {
          if (EndFullscreen()) result->Success();
          else result->Error("fullscreen_restore", "Could not restore window.");
          return;
        }
        if (call.method_name() == "isRdpFullscreenRestored") {
          result->Success(flutter::EncodableValue(!fullscreen_ && restore_succeeded_));
          return;
        }
        if (call.method_name() == "isFullscreen") {
          result->Success(flutter::EncodableValue(IsFullscreen()));
          return;
        }
        if (call.method_name() == "minimize") {
          ShowWindow(window_, SW_MINIMIZE);
          result->Success();
          return;
        }
        if (call.method_name() == "isRdpMinimized") {
          result->Success(flutter::EncodableValue(IsIconic(window_) != FALSE));
          return;
        }
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

bool GlassWindowBridge::IsFullscreen() const {
  if (!fullscreen_ || (GetWindowLongPtr(window_, GWL_STYLE) & WS_OVERLAPPEDWINDOW) != 0) return false;
  if (IsIconic(window_)) return true;  // OS minimize preserves fullscreen on restore.
  RECT bounds;
  MONITORINFO monitor = {sizeof(MONITORINFO)};
  return GetWindowRect(window_, &bounds) &&
      GetMonitorInfo(MonitorFromWindow(window_, MONITOR_DEFAULTTONEAREST), &monitor) &&
      EqualRect(&bounds, &monitor.rcMonitor);
}

bool GlassWindowBridge::BeginFullscreen() {
  if (fullscreen_) return true;
  if (!GetWindowPlacement(window_, &previous_placement_)) return false;
  previous_style_ = GetWindowLongPtr(window_, GWL_STYLE);
  previous_ex_style_ = GetWindowLongPtr(window_, GWL_EXSTYLE);
  // Save placement before SW_RESTORE so a previously maximized window is
  // restored as maximized, with its original normal rectangle intact.
  ShowWindow(window_, SW_RESTORE);
  SetWindowLongPtr(window_, GWL_STYLE, previous_style_ & ~(WS_OVERLAPPEDWINDOW | WS_MAXIMIZE | WS_MINIMIZE));
  SetWindowLongPtr(window_, GWL_EXSTYLE,
                   previous_ex_style_ & ~(WS_EX_DLGMODALFRAME | WS_EX_WINDOWEDGE |
                                          WS_EX_CLIENTEDGE | WS_EX_STATICEDGE));
  fullscreen_ = true;
  restore_succeeded_ = false;
  if (FitFullscreenMonitor()) return true;
  EndFullscreen();
  return false;
}

bool GlassWindowBridge::FitFullscreenMonitor() {
  if (!fullscreen_) return false;
  MONITORINFO monitor = {sizeof(MONITORINFO)};
  if (!GetMonitorInfo(MonitorFromWindow(window_, MONITOR_DEFAULTTONEAREST), &monitor)) return false;
  const RECT& rect = monitor.rcMonitor;  // Full monitor, including taskbar area.
  return SetWindowPos(window_, HWND_TOP, rect.left, rect.top,
                       rect.right - rect.left, rect.bottom - rect.top,
                       SWP_FRAMECHANGED | SWP_NOOWNERZORDER) != FALSE;
}

bool GlassWindowBridge::EndFullscreen() {
  if (!fullscreen_ && restore_succeeded_) return true;
  fullscreen_ = false;
  SetWindowLongPtr(window_, GWL_STYLE, previous_style_);
  SetWindowLongPtr(window_, GWL_EXSTYLE, previous_ex_style_);
  const bool placed = SetWindowPlacement(window_, &previous_placement_) != FALSE;
  const bool framed = SetWindowPos(window_, nullptr, 0, 0, 0, 0,
      SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED) != FALSE;
  restore_succeeded_ = placed && framed;
  return restore_succeeded_;
}

void GlassWindowBridge::OnWindowMessage(UINT message) {
  if (message == WM_DISPLAYCHANGE && fullscreen_) FitFullscreenMonitor();
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
