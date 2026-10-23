#include "clipboard_bridge.h"

#include <gtk/gtk.h>

#include <cstring>

namespace {

enum Target : guint { kText = 1, kPasswordHint = 2 };

// One clip we own. Freed (and its text wiped) by GTK's clear function.
struct SecretClip {
  gchar* text;
  gsize length;
  guint64 id;
};

// Process-wide: the id of the clip we currently own (0: none), the next id,
// and a random per-run nonce so a token from an earlier run is never ours.
guint64 owned_id = 0;
guint64 next_id = 1;
gchar* run_nonce = nullptr;

GtkClipboard* clipboard() { return gtk_clipboard_get(GDK_SELECTION_CLIPBOARD); }

gchar* token_for(guint64 id) { return g_strdup_printf("%s:%" G_GUINT64_FORMAT, run_nonce, id); }

void get_func(GtkClipboard*, GtkSelectionData* data, guint info, gpointer user_data) {
  auto* clip = static_cast<SecretClip*>(user_data);
  if (info == kPasswordHint) {
    static const guchar kSecret[] = "secret";
    gtk_selection_data_set(data, gtk_selection_data_get_target(data), 8, kSecret, 6);
    return;
  }
  gtk_selection_data_set_text(data, clip->text, static_cast<gint>(clip->length));
}

void clear_func(GtkClipboard*, gpointer user_data) {
  auto* clip = static_cast<SecretClip*>(user_data);
  if (clip->id == owned_id) owned_id = 0;
  memset(clip->text, 0, clip->length);
  g_free(clip->text);
  delete clip;
}

// Takes CLIPBOARD with [text]; returns the new clip's id, or 0.
guint64 copy(const gchar* text, gsize length) {
  static const GtkTargetEntry kTargets[] = {
      {const_cast<gchar*>("UTF8_STRING"), 0, kText},
      {const_cast<gchar*>("text/plain;charset=utf-8"), 0, kText},
      {const_cast<gchar*>("TEXT"), 0, kText},
      {const_cast<gchar*>("STRING"), 0, kText},
      {const_cast<gchar*>("x-kde-passwordManagerHint"), 0, kPasswordHint},
  };
  auto* clip = new SecretClip{static_cast<gchar*>(g_malloc(length + 1)), length, next_id++};
  memcpy(clip->text, text, length);
  clip->text[length] = '\0';
  // Taking the clipboard again runs the previous clip's clear_func first.
  if (!gtk_clipboard_set_with_data(clipboard(), kTargets, G_N_ELEMENTS(kTargets), get_func, clear_func,
                                   clip)) {
    memset(clip->text, 0, length);
    g_free(clip->text);
    delete clip;
    return 0;
  }
  owned_id = clip->id;
  return clip->id;
}

const gchar* clear_if_owned(const gchar* token) {
  if (owned_id != 0) {
    g_autofree gchar* current = token_for(owned_id);
    if (g_strcmp0(current, token) == 0) {
      gtk_clipboard_clear(clipboard());  // Runs clear_func.
      owned_id = 0;
      return "cleared";
    }
  }
  // One of ours from this run, since replaced: foreign. Unknown: gone.
  const gchar* separator = strchr(token, ':');
  if (separator != nullptr && strncmp(token, run_nonce, separator - token) == 0 &&
      strlen(run_nonce) == static_cast<gsize>(separator - token)) {
    return "foreign";
  }
  return "gone";
}

FlMethodResponse* error(const gchar* code, const gchar* message) {
  return FL_METHOD_RESPONSE(fl_method_error_response_new(code, message, nullptr));
}

void method_call(FlMethodChannel*, FlMethodCall* call, gpointer) {
  const gchar* method = fl_method_call_get_name(call);
  FlValue* args = fl_method_call_get_args(call);
  g_autoptr(FlMethodResponse) response = nullptr;
  auto string_arg = [args](const gchar* key) -> FlValue* {
    if (args == nullptr || fl_value_get_type(args) != FL_VALUE_TYPE_MAP) return nullptr;
    FlValue* value = fl_value_lookup_string(args, key);
    return value != nullptr && fl_value_get_type(value) == FL_VALUE_TYPE_STRING ? value : nullptr;
  };
  if (g_strcmp0(method, "copySecret") == 0 || g_strcmp0(method, "copyPrivate") == 0) {
    FlValue* text = string_arg("text");
    if (text == nullptr) {
      response = error("bad_args", "Invalid clipboard request");
    } else {
      const gchar* value = fl_value_get_string(text);
      const guint64 id = copy(value, strlen(value));
      if (id == 0) {
        response = error("clipboard", "Secret clipboard unavailable");
      } else if (g_strcmp0(method, "copySecret") == 0) {
        g_autofree gchar* token = token_for(id);
        g_autoptr(FlValue) result = fl_value_new_string(token);
        response = FL_METHOD_RESPONSE(fl_method_success_response_new(result));
      } else {
        response = FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
      }
    }
  } else if (g_strcmp0(method, "clearIfOwned") == 0) {
    FlValue* token = string_arg("token");
    if (token == nullptr) {
      response = error("bad_args", "Invalid clipboard token");
    } else {
      g_autoptr(FlValue) result = fl_value_new_string(clear_if_owned(fl_value_get_string(token)));
      response = FL_METHOD_RESPONSE(fl_method_success_response_new(result));
    }
  } else if (g_strcmp0(method, "forget") == 0) {
    response = FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
  } else {
    response = FL_METHOD_RESPONSE(fl_method_not_implemented_response_new());
  }
  fl_method_call_respond(call, response, nullptr);
}

}  // namespace

void clipboard_bridge_register(FlBinaryMessenger* messenger, GObject* owner) {
  if (run_nonce == nullptr) {
    guint32 words[4];
    for (auto& word : words) word = g_random_int();
    run_nonce = g_strdup_printf("%08x%08x%08x%08x", words[0], words[1], words[2], words[3]);
  }
  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
  FlMethodChannel* channel =
      fl_method_channel_new(messenger, "consolecrypt/clipboard", FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(channel, method_call, nullptr, nullptr);
  g_object_set_data_full(owner, "consolecrypt-clipboard-channel", channel, g_object_unref);
}
