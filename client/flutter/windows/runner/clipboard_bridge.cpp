#include "clipboard_bridge.h"

#include <bcrypt.h>
#include <flutter/standard_method_codec.h>

#include <cstring>
#include <string>
#include <vector>

namespace {

constexpr char kClipboardChannel[] = "consolecrypt/clipboard";
constexpr size_t kIdBytes = 16;

UINT Format(const wchar_t* name) { return RegisterClipboardFormatW(name); }

// The clipboard can be held by another process for a moment: retry briefly.
bool OpenClipboardRetrying(HWND window) {
  for (int attempt = 0; attempt < 10; ++attempt) {
    if (OpenClipboard(window)) return true;
    Sleep(15);
  }
  return false;
}

// Moves [bytes] into a movable global block owned by the clipboard on
// success. The caller must have the clipboard open.
bool SetBlock(UINT format, const void* bytes, size_t size) {
  HGLOBAL block = GlobalAlloc(GMEM_MOVEABLE, size == 0 ? 1 : size);
  if (!block) return false;
  void* target = GlobalLock(block);
  if (!target) {
    GlobalFree(block);
    return false;
  }
  if (size > 0) std::memcpy(target, bytes, size);
  GlobalUnlock(block);
  if (!SetClipboardData(format, block)) {
    GlobalFree(block);
    return false;
  }
  return true;
}

std::wstring Utf16(const std::string& utf8) {
  if (utf8.empty()) return std::wstring();
  const int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, utf8.data(),
                                         static_cast<int>(utf8.size()), nullptr, 0);
  if (length <= 0) return std::wstring();
  std::wstring wide(static_cast<size_t>(length), L'\0');
  MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, utf8.data(), static_cast<int>(utf8.size()),
                      wide.data(), length);
  return wide;
}

std::string NewId() {
  unsigned char bytes[kIdBytes] = {};
  if (BCryptGenRandom(nullptr, bytes, sizeof(bytes), BCRYPT_USE_SYSTEM_PREFERRED_RNG) != 0) {
    return std::string();
  }
  static const char kHex[] = "0123456789abcdef";
  std::string id;
  for (unsigned char byte : bytes) {
    id.push_back(kHex[byte >> 4]);
    id.push_back(kHex[byte & 0x0f]);
  }
  return id;
}

const std::string* StringArgument(const flutter::EncodableValue* arguments, const char* key) {
  const auto* map = std::get_if<flutter::EncodableMap>(arguments);
  if (!map) return nullptr;
  const auto found = map->find(flutter::EncodableValue(key));
  if (found == map->end()) return nullptr;
  return std::get_if<std::string>(&found->second);
}

}  // namespace

ClipboardBridge::ClipboardBridge(flutter::BinaryMessenger* messenger, HWND window)
    : window_(window) {
  channel_ = std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
      messenger, kClipboardChannel, &flutter::StandardMethodCodec::GetInstance());
  channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
             std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result) {
        const std::string& method = call.method_name();
        if (method == "copySecret" || method == "copyPrivate") {
          const std::string* text = StringArgument(call.arguments(), "text");
          if (!text) {
            result->Error("bad_args", "Invalid clipboard request");
            return;
          }
          const bool secret = method == "copySecret";
          const std::string token = Copy(*text, secret);
          if (token.empty()) {
            result->Error("clipboard", "Secret clipboard unavailable");
          } else if (secret) {
            result->Success(flutter::EncodableValue(token));
          } else {
            result->Success();
          }
        } else if (method == "clearIfOwned") {
          const std::string* token = StringArgument(call.arguments(), "token");
          if (!token || token->empty()) {
            result->Error("bad_args", "Invalid clipboard token");
            return;
          }
          const std::string outcome = ClearIfOwned(*token);
          if (outcome == "busy") {
            result->Error("busy", "The clipboard is in use");
          } else {
            result->Success(flutter::EncodableValue(outcome));
          }
        } else if (method == "forget") {
          last_secret_.clear();
          result->Success();
        } else {
          result->NotImplemented();
        }
      });
}

ClipboardBridge::~ClipboardBridge() {
  if (channel_) channel_->SetMethodCallHandler(nullptr);
}

std::string ClipboardBridge::Copy(const std::string& utf8, bool secret) {
  std::wstring wide = Utf16(utf8);
  if (wide.empty() && !utf8.empty()) return std::string();
  const std::string id = NewId();
  if (id.empty() || !OpenClipboardRetrying(window_)) {
    SecureZeroMemory(wide.data(), wide.size() * sizeof(wchar_t));
    return std::string();
  }
  const DWORD zero = 0;
  bool ok = EmptyClipboard() != 0;
  ok = ok && SetBlock(CF_UNICODETEXT, wide.c_str(), (wide.size() + 1) * sizeof(wchar_t));
  ok = ok && SetBlock(Format(L"ExcludeClipboardContentFromMonitorProcessing"), &zero, sizeof(zero));
  ok = ok && SetBlock(Format(L"CanIncludeInClipboardHistory"), &zero, sizeof(zero));
  ok = ok && SetBlock(Format(L"CanUploadToCloudClipboard"), &zero, sizeof(zero));
  ok = ok && SetBlock(Format(L"ConsoleCrypt.ClipId"), id.data(), id.size());
  if (!ok) EmptyClipboard();  // Never leave the text without its protections.
  CloseClipboard();
  SecureZeroMemory(wide.data(), wide.size() * sizeof(wchar_t));
  if (!ok) return std::string();
  const std::string token = std::to_string(GetClipboardSequenceNumber()) + ":" + id;
  last_secret_ = secret ? token : std::string();
  return token;
}

std::string ClipboardBridge::ClearIfOwned(const std::string& token) {
  const size_t colon = token.find(':');
  if (colon == std::string::npos) return "gone";
  const std::string sequence = token.substr(0, colon), id = token.substr(colon + 1);
  if (std::to_string(GetClipboardSequenceNumber()) != sequence) {
    if (token == last_secret_) last_secret_.clear();
    return CountClipboardFormats() == 0 ? "gone" : "foreign";
  }
  const UINT id_format = Format(L"ConsoleCrypt.ClipId");
  if (!IsClipboardFormatAvailable(id_format)) {
    if (token == last_secret_) last_secret_.clear();
    return "foreign";
  }
  if (!OpenClipboardRetrying(window_)) return "busy";
  if (token == last_secret_) last_secret_.clear();
  // Only our own id format is read, never the text.
  bool owned = false;
  if (HANDLE data = GetClipboardData(id_format)) {
    const SIZE_T size = GlobalSize(data);
    if (const auto* bytes = static_cast<const char*>(GlobalLock(data))) {
      owned = size >= id.size() && std::memcmp(bytes, id.data(), id.size()) == 0;
      GlobalUnlock(data);
    }
  }
  if (owned) EmptyClipboard();
  CloseClipboard();
  return owned ? "cleared" : "foreign";
}

void ClipboardBridge::ClearOnExit() {
  if (!last_secret_.empty()) ClearIfOwned(last_secret_);
  last_secret_.clear();
}
