plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "com.ojbkxc.hyx"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.ojbkxc.hyx"
        minSdk = 26
        targetSdk = 35
        versionCode = 1000002
        versionName = "1.0.2"

        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
        viewBinding = true
    }

    buildTypes {
        debug { isMinifyEnabled = false }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            signingConfig = signingConfigs.getByName("debug")
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    // Path to the Rust shared library produced by cargo-ndk. The library is
    // named libhyx_mobile.so and placed under src/main/jniLibs via the
    // cargoNdk task (see android/buildRustNdk gradle task below).
    sourceSets {
        getByName("main") { jniLibs.srcDirs("src/main/jniLibs") }
    }

    packaging {
        resources { excludes += "/META-INF/{AL2.0,LGPL2.1}" }
    }

    lint { checkReleaseBuilds = false }
}

// Build the Rust core into per-ABI shared libraries with cargo-ndk, then copy
// them into src/main/jniLibs before packaging. NDK must be installed; the
// `cargo-ndk` binary must be on PATH (install via: cargo install cargo-ndk).
val cargoNdk = tasks.register<Exec>("cargoNdk") {
    // rootProject is the `android` gradle root; its projectDir is <repo>/android,
    // so its parentFile is the repo root where mobile/Cargo.toml lives.
    workingDir = rootProject.projectDir.parentFile
    // Call cargo directly (not via `sh -c`) so the task works on Windows too.
    commandLine(
        "cargo", "ndk", "-t", "arm64-v8a", "-o",
        "android/app/src/main/jniLibs", "build", "--release",
        "-p", "hyx-mobile", "--manifest-path", "mobile/Cargo.toml"
    )
}

tasks.matching { it.name.contains("assemble", ignoreCase = true) }
    .configureEach { dependsOn(cargoNdk) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.ui)
    implementation(libs.androidx.ui.graphics)
    implementation(libs.androidx.ui.tooling.preview)
    implementation(libs.androidx.material3)
    implementation(libs.androidx.material.icons)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.documentfile)


    debugImplementation(libs.androidx.ui.tooling)
}
