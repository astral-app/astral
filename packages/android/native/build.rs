use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const VENDOR: &str = "../../../vendor";

fn tool(name: &str, fallback: &str) -> String {
    env::var(name).unwrap_or_else(|_| fallback.to_owned())
}

fn compile(compiler: &str, source: &Path, object: &Path, flags: &[&str]) {
    println!("cargo:rerun-if-changed={}", source.display());
    let status = Command::new(compiler)
        .args(["-c", "-O2", "-fPIC", "-w", "-fvisibility=hidden"])
        .args(flags)
        .arg(source)
        .arg("-o")
        .arg(object)
        .status()
        .unwrap_or_else(|error| panic!("{compiler} for {}: {error}", source.display()));
    assert!(status.success(), "could not compile {}", source.display());
}

fn main() {
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rerun-if-env-changed=ASTRAL_VERSION");
    println!("cargo:rerun-if-env-changed=ASTRAL_COMMIT");
    let output = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let compiler = tool("CC", "cc");
    let archiver = tool("AR", "ar");
    let lua = output.join("onelua.o");
    let sqlite = output.join("sqlite3.o");
    compile(
        &compiler,
        &Path::new(VENDOR).join("lua/onelua.c"),
        &lua,
        &["-DMAKE_LIB", "-DLUA_USE_POSIX"],
    );
    compile(
        &compiler,
        &Path::new(VENDOR).join("sqlite/sqlite3.c"),
        &sqlite,
        &["-DSQLITE_THREADSAFE=2", "-DSQLITE_OMIT_LOAD_EXTENSION"],
    );
    let archive = output.join("libastral_vendor.a");
    let _ = std::fs::remove_file(&archive);
    let status = Command::new(&archiver)
        .arg("rcs")
        .arg(&archive)
        .args([&lua, &sqlite])
        .status()
        .unwrap_or_else(|error| panic!("{archiver}: {error}"));
    assert!(status.success(), "could not archive the vendored C sources");
    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-lib=static=astral_vendor");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=log");
        println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
    }
    println!(
        "cargo:rustc-env=ASTRAL_VERSION={}",
        tool("ASTRAL_VERSION", env!("CARGO_PKG_VERSION"))
    );
    println!(
        "cargo:rustc-env=ASTRAL_COMMIT={}",
        tool("ASTRAL_COMMIT", "unknown")
    );
}
