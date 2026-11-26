import javax.inject.Inject

plugins {
    id("com.android.application")
}

val repositoryRoot: File = rootDir.parentFile.parentFile
val astralVersion = repositoryRoot.resolve("version.txt").readText().trim()

abstract class StageResources : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sources: ConfigurableFileCollection

    @get:Internal
    abstract val repository: DirectoryProperty

    @get:OutputDirectory
    abstract val output: DirectoryProperty

    @get:Inject
    abstract val commands: ExecOperations

    @TaskAction
    fun stage() {
        val target = output.get().dir("astral").asFile
        target.deleteRecursively()
        commands.exec {
            workingDir = repository.get().asFile
            commandLine("cargo", "run", "--quiet", "--release", "--manifest-path", "packages/tools/Cargo.toml", "--", "stage", target.absolutePath)
        }
    }
}

abstract class BuildNativeLibraries : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sources: ConfigurableFileCollection

    @get:Internal
    abstract val repository: DirectoryProperty

    @get:Input
    abstract val ndk: Property<String>

    @get:OutputDirectory
    abstract val output: DirectoryProperty

    @get:Inject
    abstract val commands: ExecOperations

    @TaskAction
    fun build() {
        commands.exec {
            workingDir = repository.get().asFile
            commandLine(
                "cargo", "run", "--quiet", "--release", "--manifest-path", "packages/tools/Cargo.toml", "--",
                "android-core", "--ndk", ndk.get(), "--output", output.get().asFile.absolutePath,
            )
        }
    }
}

val stageResources = tasks.register<StageResources>("stageResources") {
    sources.from(
        repositoryRoot.resolve("packages/addons"),
        repositoryRoot.resolve("packages/catalog"),
        repositoryRoot.resolve("packages/assets"),
    )
    repository.set(repositoryRoot)
}

val buildNativeLibraries = tasks.register<BuildNativeLibraries>("buildNativeLibraries") {
    sources.from(
        fileTree(repositoryRoot.resolve("packages/core")) { exclude("target/**", "build/**") },
        fileTree(repositoryRoot.resolve("packages/android/native")) { exclude("target/**") },
        repositoryRoot.resolve("keys"),
        repositoryRoot.resolve("version.txt"),
    )
    repository.set(repositoryRoot)
    ndk.set(androidComponents.sdkComponents.ndkDirectory.map { it.asFile.absolutePath })
}

androidComponents {
    onVariants { variant ->
        variant.sources.assets?.addGeneratedSourceDirectory(stageResources, StageResources::output)
        variant.sources.jniLibs?.addGeneratedSourceDirectory(buildNativeLibraries, BuildNativeLibraries::output)
    }
}


android {
    namespace = "app.astral.android"
    compileSdk = 37
    ndkVersion = "27.3.13750724"

    defaultConfig {
        applicationId = "app.astral.android"
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = astralVersion


        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
        resources {
            merges += "META-INF/xposed/*"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    compileOnly("io.github.libxposed:api:102.0.0")
    implementation("com.squareup.okhttp3:okhttp:4.12.1")
}
