import javax.inject.Inject

plugins {
    id("com.android.application")
}

val repositoryRoot: File = rootDir.parentFile.parentFile
val astralVersion = "0.0.1"

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
            commandLine("go", "run", "./packages/tools/cmd/astral", "stage", target.absolutePath)
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

androidComponents {
    onVariants { variant ->
        variant.sources.assets?.addGeneratedSourceDirectory(stageResources, StageResources::output)
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

        externalNativeBuild {
            cmake {
                arguments +=
                    listOf(
                        "-DANDROID_STL=c++_static",
                        "-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON",
                        "-DASTRAL_ROOT=${repositoryRoot.absolutePath}",
                        "-DASTRAL_VERSION=$astralVersion",
                    )
                cppFlags += listOf("-std=c++20", "-fexceptions", "-frtti")
            }
        }

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

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "3.22.1"
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
}
