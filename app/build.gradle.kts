plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.dymith.browser"
    compileSdk = 35
    buildToolsVersion = "35.0.0"

    defaultConfig {
        applicationId = "dev.dymith.browser"
        minSdk = 26
        targetSdk = 35
        versionCode = (System.getenv("DYMITH_VERSION_CODE") ?: "1").toInt()
        versionName = "0.0.2-phase0"
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    // Phase 0: ký bằng một debug keystore cố định (do CI tạo lần đầu) để bản mới cài đè được bản cũ.
    // Keystore phát hành thật sẽ đưa vào GitHub Secrets ở Phase 1.
    signingConfigs {
        create("dymith") {
            storeFile = file("signing/dymith-debug.p12")
            storePassword = "android"
            keyAlias = "dymith"
            keyPassword = "android"
            storeType = "pkcs12"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            signingConfig = signingConfigs.getByName("dymith")
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    packaging {
        // Thư viện native lưu không nén, căn trang 16 KB, mmap trực tiếp từ APK.
        jniLibs {
            useLegacyPackaging = false
        }
        resources {
            excludes += setOf("DebugProbesKt.bin", "kotlin-tooling-metadata.json")
        }
    }

    lint {
        checkReleaseBuilds = false
        abortOnError = false
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.webkit:webkit:1.12.1")
}
