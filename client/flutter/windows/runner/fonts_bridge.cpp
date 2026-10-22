#include "fonts_bridge.h"

#include <flutter/standard_method_codec.h>

#include <string>
#include <vector>

#include "font_families.h"

namespace {

constexpr char kFontsChannel[] = "consolecrypt/fonts";

flutter::EncodableValue ToList(const std::vector<std::string>& names) {
  flutter::EncodableList list;
  list.reserve(names.size());
  for (const auto& name : names) {
    list.emplace_back(name);
  }
  return flutter::EncodableValue(std::move(list));
}

}  // namespace

FontsBridge::FontsBridge(flutter::BinaryMessenger* messenger) {
  channel_ = std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
      messenger, kFontsChannel, &flutter::StandardMethodCodec::GetInstance());
  channel_->SetMethodCallHandler(
      [](const flutter::MethodCall<flutter::EncodableValue>& call,
         std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
             result) {
        if (call.method_name() == "monospaceFamilies") {
          result->Success(ToList(MonospaceFontFamilies()));
        } else if (call.method_name() == "installedFamilies") {
          result->Success(ToList(InstalledFontFamilies()));
        } else {
          result->NotImplemented();
        }
      });
}

FontsBridge::~FontsBridge() {
  if (channel_) {
    channel_->SetMethodCallHandler(nullptr);
  }
}
