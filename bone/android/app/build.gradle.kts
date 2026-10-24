import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val local = Properties().apply {
    rootProject.file("local.properties").takeIf { it.exists() }?.inputStream()?.use { load(it) }
}
val serverUrl = providers.gradleProperty("bone.url").orNull
    ?: local.getProperty("bone.url")
    ?: error("set bone.url in android/local.properties, e.g. bone.url=https://mail.example.com")

android {
    namespace = "dev.differentname.bone"
    compileSdk = 36
    buildToolsVersion = "36.0.0"

    defaultConfig {
        applicationId = "dev.differentname.bone"
        minSdk = 33
        targetSdk = 36
        versionCode = 3
        versionName = "0.1.0"
        buildConfigField("String", "SERVER_URL", "\"${serverUrl.trimEnd('/')}\"")
    }

    buildFeatures {
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation("org.unifiedpush.android:connector:3.3.5")
}
