import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

// Store builds must use an explicitly supplied private key. Never fall back
// to the runner's historical preview signer for a RuStore release.
val signingPropertiesPath = System.getenv("CC_ANDROID_SIGNING_PROPERTIES")
val releaseSigning = Properties().apply {
    if (!signingPropertiesPath.isNullOrBlank()) {
        file(signingPropertiesPath).inputStream().use { load(it) }
    }
}
val hasReleaseSigning = listOf("storeFile", "storePassword", "keyAlias", "keyPassword")
    .all { !releaseSigning.getProperty(it).isNullOrBlank() }

android {
    namespace = "io.consolecrypt.consolecrypt"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        // flutter_local_notifications schedules with java.time (FEAT-07).
        isCoreLibraryDesugaringEnabled = true
    }

    defaultConfig {
        applicationId = "io.consolecrypt.consolecrypt"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = 30
        targetSdk = flutter.targetSdkVersion
        // Uses the version code from pubspec.yaml. When using split APKs, 1000 * ABI_VERSION
        // is added automatically by Flutter. (https://developer.android.com/studio/build/configure-apk-splits#configure-APK-versions)
        // You can force using the value of versionCode by specifying the `-P force-version-code-ignoring-abi=true`
        // flag during build.
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    buildFeatures { buildConfig = true }
    flavorDimensions += "distribution"
    productFlavors {
        create("direct") { dimension = "distribution" }
        create("rustore") { dimension = "distribution" }
    }
    signingConfigs {
        if (hasReleaseSigning) {
            create("privateRelease") {
                storeFile = file(releaseSigning.getProperty("storeFile"))
                storePassword = releaseSigning.getProperty("storePassword")
                keyAlias = releaseSigning.getProperty("keyAlias")
                keyPassword = releaseSigning.getProperty("keyPassword")
            }
        }
    }

    // The APK is ARM64 only; plugins such as CameraX also ship 32-bit ARM and
    // x86 libraries that verify-android-apk.py refuses.
    packaging {
        jniLibs {
            excludes += listOf("lib/armeabi-v7a/**", "lib/x86/**", "lib/x86_64/**")
        }
    }

    buildTypes {
        release {
            // Preserve direct-preview update compatibility; RuStore is guarded below.
            signingConfig = signingConfigs.getByName(if (hasReleaseSigning) "privateRelease" else "debug")
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
}

gradle.taskGraph.whenReady {
    if (allTasks.any { it.project == project && it.name.contains("rustore", ignoreCase = true) &&
            it.name.contains("release", ignoreCase = true) } && !hasReleaseSigning) {
        throw GradleException("RuStore release requires CC_ANDROID_SIGNING_PROPERTIES with a private release key")
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

dependencies {
    // System-browser OIDC, never an embedded web view (Apache-2.0).
    implementation("androidx.browser:browser:1.10.0")
    // Must match rustls-platform-verifier-android in client/rust/Cargo.lock.
    implementation("org.rustls:rustls-platform-verifier:0.2.0")
    // java.time on older Android for scheduled reminders (GPL-2.0 with Classpath Exception).
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
}

flutter {
    source = "../.."
}
