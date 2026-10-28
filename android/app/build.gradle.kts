import java.util.Properties

plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

val signingProperties = Properties()
val signingFile = rootProject.file("key.properties")
if (signingFile.exists()) {
    signingFile.inputStream().use { signingProperties.load(it) }
}
fun signingValue(environmentName: String, propertyName: String): String? =
    System.getenv(environmentName)?.takeIf { it.isNotBlank() }
        ?: signingProperties.getProperty(propertyName)?.takeIf { it.isNotBlank() }

val releaseStoreFile = signingValue("MASHIN_ANDROID_KEYSTORE", "storeFile")
val releaseStorePassword = signingValue("MASHIN_ANDROID_KEYSTORE_PASSWORD", "storePassword")
val releaseKeyAlias = signingValue("MASHIN_ANDROID_KEY_ALIAS", "keyAlias")
val releaseKeyPassword = signingValue("MASHIN_ANDROID_KEY_PASSWORD", "keyPassword")
val hasReleaseSigning = listOf(releaseStoreFile, releaseStorePassword, releaseKeyAlias, releaseKeyPassword)
    .all { !it.isNullOrBlank() }

android {
    namespace = "com.joie96.mashin"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = "30.0.16248370"

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        unitTests.isIncludeAndroidResources = true
    }

    defaultConfig {
        applicationId = "com.joie96.mashin"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        // cpal on Android links against AAudio (libaaudio), available from API 26.
        minSdk = maxOf(flutter.minSdkVersion, 26)
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    signingConfigs {
        create("release") {
            if (hasReleaseSigning) {
                storeFile = rootProject.file(releaseStoreFile!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("release")
            proguardFiles("proguard-rules.pro")
        }
    }
}

dependencies {
    implementation("androidx.media3:media3-common:1.11.1")
    implementation("androidx.media3:media3-session:1.11.1")
    implementation("androidx.security:security-crypto:1.1.0-alpha06")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.16.1")
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}

val validateReleaseSigning = tasks.register("validateReleaseSigning") {
    doLast {
        check(hasReleaseSigning) {
            "Android release signing is missing. Configure key.properties or MASHIN_ANDROID_KEY*; see docs/releases.md."
        }
        check(rootProject.file(releaseStoreFile!!).isFile) {
            "Android release keystore does not exist."
        }
    }
}

tasks.configureEach {
    if (name == "preReleaseBuild") {
        dependsOn(validateReleaseSigning)
    }
    // Resource-backed host tests consume the Flutter asset-copy task's output.
    if (name.startsWith("package") && name.endsWith("UnitTestForUnitTest")) {
        val variant = name.removePrefix("package").removeSuffix("UnitTestForUnitTest")
        dependsOn("copyFlutterAssets$variant")
    }
}
