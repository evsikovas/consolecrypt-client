#ifndef RUNNER_FONTS_BRIDGE_H_
#define RUNNER_FONTS_BRIDGE_H_

#include <flutter/binary_messenger.h>
#include <flutter/encodable_value.h>
#include <flutter/method_channel.h>

#include <memory>

// `consolecrypt/fonts`, as on macOS:
//
// * `monospaceFamilies` lists the installed fixed-pitch families so the
//   terminal font picker offers fonts that actually render;
// * `installedFamilies` lists every installed family name, so Dart can tell a
//   missing font from one that is merely not fixed-pitch.
//
// Both come from the DirectWrite system collection that Flutter resolves
// `fontFamily` against, including fonts installed only for the current user.
class FontsBridge {
 public:
  explicit FontsBridge(flutter::BinaryMessenger* messenger);
  ~FontsBridge();

  FontsBridge(const FontsBridge&) = delete;
  FontsBridge& operator=(const FontsBridge&) = delete;

 private:
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> channel_;
};

#endif  // RUNNER_FONTS_BRIDGE_H_
