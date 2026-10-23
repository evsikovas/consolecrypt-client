#ifndef RUNNER_CLIPBOARD_BRIDGE_H_
#define RUNNER_CLIPBOARD_BRIDGE_H_

#include <flutter_linux/flutter_linux.h>

// `consolecrypt/clipboard`: protected clipboard for secrets (S9), GTK 3 on
// X11 and Wayland.
//
// * secrets and private text are offered on CLIPBOARD only (never PRIMARY)
//   as UTF8_STRING, text/plain;charset=utf-8, TEXT and STRING, plus
//   `x-kde-passwordManagerHint` = `secret` so clipboard managers do not
//   record them;
// * the content is never handed to a clipboard manager on exit (no
//   gtk_clipboard_set_can_store), so it disappears with the app;
// * the ownership token names our current clip; GTK calls the clear function
//   as soon as another owner takes the clipboard, so `clearIfOwned` clears
//   only while we still own it and never reads the content.
//
// Registers the channel on [messenger]; it lives as long as [owner].
void clipboard_bridge_register(FlBinaryMessenger* messenger, GObject* owner);

#endif  // RUNNER_CLIPBOARD_BRIDGE_H_
