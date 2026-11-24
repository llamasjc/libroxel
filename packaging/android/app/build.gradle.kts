plugins {
    id("com.android.application")
}

android {
    namespace = "com.libroxel.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.libroxel.app"
        minSdk = 26
        targetSdk = 36
        versionCode = (System.getenv("VERSION_CODE") ?: "1").toInt()
        versionName = System.getenv("VERSION") ?: "dev"
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a")
            debugSymbolLevel = "SYMBOL_TABLE"
        }
    }

    System.getenv("ANDROID_NDK_LATEST_HOME")?.let { ndkPath = it }

    val keystore = System.getenv("ANDROID_KEYSTORE")

    signingConfigs {
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            if (keystore != null) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(platform("org.jetbrains.kotlin:kotlin-bom:1.8.22"))
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.games:games-activity:4.0.0")
}
