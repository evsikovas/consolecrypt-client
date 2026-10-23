#ifndef RUNNER_CLIPBOARD_BRIDGE_H_
#define RUNNER_CLIPBOARD_BRIDGE_H_

#include <flutter/binary_messenger.h>
#include <flutter/encodable_value.h>
#include <flutter/method_channel.h>
#include <windows.h>

#include <memory>
#include <string>

// `consolecrypt/clipboard`: protected clipboard for secrets (S9).
//
// * `copySecret` / `copyPrivate` write CF_UNICODETEXT plus the registered
//   formats that keep the data out of clipboard history and the cloud
//   clipboard and away from clipboard monitors
//   (ExcludeClipboardContentFromMonitorProcessing,
//   CanIncludeInClipboardHistory = 0, CanUploadToCloudClipboard = 0), and a
//   random id in a private format;
// * the ownership token is `sequence:id`: the clipboard sequence number after
//   our write, with the id guarding against a sequence repeated after a
//   restart. `clearIfOwned` empties the clipboard only for our own clip and
//   never reads the copied text;
// * when the window is destroyed, the last secret is cleared if still ours.
class ClipboardBridge {
 public:
  ClipboardBridge(flutter::BinaryMessenger* messenger, HWND window);
  ~ClipboardBridge();

  ClipboardBridge(const ClipboardBridge&) = delete;
  ClipboardBridge& operator=(const ClipboardBridge&) = delete;

  // WM_DESTROY: clears the last copied secret if the clipboard still holds it.
  void ClearOnExit();

 private:
  // Returns the token, or an empty string on failure.
  std::string Copy(const std::string& utf8, bool secret);
  // "cleared", "foreign", "gone" or "busy" (clipboard held by another app).
  std::string ClearIfOwned(const std::string& token);

  HWND window_;
  std::string last_secret_;
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> channel_;
};

#endif  // RUNNER_CLIPBOARD_BRIDGE_H_
