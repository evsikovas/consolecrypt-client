//! Android process initialization before Dart starts the core. Only an
//! application context is retained, never an Activity or user data.

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{Global, JObject};
use jni::refs::Reference;
use std::sync::OnceLock;

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_consolecrypt_consolecrypt_MainActivity_nativeInitialize<'local>(
    mut unowned: jni::EnvUnowned<'local>,
    _this: JObject<'local>,
    context: JObject<'local>,
) {
    unowned
        .with_env(|env| -> jni::errors::Result<()> {
            let global = env.new_global_ref(&context)?;
            let vm = env.get_java_vm()?.get_raw();
            // The verifier must use Android's TrustManager, including its
            // certificate validation. No permissive certificate fallback.
            rustls_platform_verifier::android::init_with_env(env, context)?;
            static CONTEXT: OnceLock<Global<JObject<'static>>> = OnceLock::new();
            CONTEXT.get_or_init(|| {
                // SAFETY: JNI supplied a valid VM; the application Global stays
                // alive for the entire process. OnceLock prevents double init
                // when Android recreates the Activity.
                unsafe {
                    ndk_context::initialize_android_context(vm.cast(), global.as_raw().cast());
                }
                global
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}
