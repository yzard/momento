plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.ksp)
}

val releaseStoreFile = providers.gradleProperty("momentoReleaseStoreFile")
val releaseStorePassword = providers.gradleProperty("momentoReleaseStorePassword")
val releaseKeyAlias = providers.gradleProperty("momentoReleaseKeyAlias")
val releaseKeyPassword = providers.gradleProperty("momentoReleaseKeyPassword")
val androidVersion = rootProject.file("version.txt").readText().trim()
val androidVersionMatch = requireNotNull(Regex("^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$").matchEntire(androidVersion)) {
    "version.txt must contain a semantic version in major.minor.patch format."
}
val (androidVersionMajor, androidVersionMinor, androidVersionPatch) = androidVersionMatch.destructured
val androidVersionCode = androidVersionMajor.toLong() * 1_000_000L +
    androidVersionMinor.toLong() * 1_000L + androidVersionPatch.toLong()
val androidBuildTimeMillis = System.currentTimeMillis()
require(androidVersionMinor.toLong() <= 999L && androidVersionPatch.toLong() <= 999L) {
    "Android version minor and patch components must not exceed 999."
}
require(androidVersionCode in 1L..2_100_000_000L) {
    "Android version produces an unsupported versionCode."
}

android {
    namespace = "io.github.yzard.momento"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.github.yzard.momento"
        minSdk = 26
        targetSdk = 36
        versionCode = androidVersionCode.toInt()
        versionName = androidVersion
        buildConfigField("long", "BUILD_TIME_MILLIS", "${androidBuildTimeMillis}L")
        manifestPlaceholders["momentoBuildTime"] = "epochMillis:$androidBuildTimeMillis"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    buildFeatures { compose = true; buildConfig = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.15" }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
    sourceSets {
        getByName("test").java.srcDir("../../../tests/android/io")
        getByName("androidTest").java.srcDir("../../../tests/android/instrumented")
    }

    signingConfigs {
        create("release") {
            storeFile = releaseStoreFile.orNull?.let(::file)
            storePassword = releaseStorePassword.orNull
            keyAlias = releaseKeyAlias.orNull
            keyPassword = releaseKeyPassword.orNull
        }
    }

    buildTypes {
        getByName("debug") {
            buildConfigField("boolean", "ALLOW_CLEARTEXT_TRAFFIC", "true")
            manifestPlaceholders["momentoUsesCleartextTraffic"] = "true"
        }
        getByName("release") {
            buildConfigField("boolean", "ALLOW_CLEARTEXT_TRAFFIC", "false")
            manifestPlaceholders["momentoUsesCleartextTraffic"] = "false"
            signingConfig = signingConfigs.getByName("release")
        }
    }
}

ksp { arg("room.schemaLocation", "$projectDir/schemas") }

val validateReleaseSigning by tasks.registering {
    doLast {
        val requiredProperties = mapOf(
            "momentoReleaseStoreFile" to releaseStoreFile,
            "momentoReleaseStorePassword" to releaseStorePassword,
            "momentoReleaseKeyAlias" to releaseKeyAlias,
            "momentoReleaseKeyPassword" to releaseKeyPassword,
        )
        val missingProperties = requiredProperties.filterValues { it.orNull.isNullOrBlank() }.keys
        check(missingProperties.isEmpty()) {
            "Release builds require Gradle properties: ${missingProperties.joinToString(", ")}."
        }
    }
}

tasks.configureEach {
    if (name == "assembleRelease" || name == "bundleRelease") {
        dependsOn(validateReleaseSigning)
    }
}

dependencies {
    implementation(libs.androidx.core)
    implementation(libs.activity.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.icons)
    implementation(libs.navigation.compose)
    implementation(libs.lifecycle.runtime)
    implementation(libs.lifecycle.viewmodel)
    implementation(libs.lifecycle.compose)
    implementation(libs.retrofit)
    implementation(libs.retrofit.serialization)
    implementation(libs.okhttp)
    implementation(libs.serialization.json)
    implementation(libs.room.runtime)
    implementation(libs.room.ktx)
    implementation(libs.work.runtime)
    implementation(libs.datastore)
    implementation(libs.coil)
    implementation(libs.media3.exoplayer)
    implementation(libs.media3.ui)
    implementation(libs.media3.okhttp)
    implementation(libs.osmdroid)
    ksp(libs.room.compiler)
    testImplementation(libs.junit)
    testImplementation(libs.serialization.json)
    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test)
    androidTestImplementation(libs.androidx.test.ext)
    androidTestImplementation(libs.espresso)
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)
}
