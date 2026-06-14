fn main() {
    // Tell Cargo to re-run this build script if any src file changes
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=build.rs");

    // On Windows, link against kernel32 for shared memory support
    // On Linux/macOS, link against standard POSIX libraries
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-lib=kernel32");
    }

    #[cfg(target_os = "linux")]
    {
        println!("cargo:rustc-link-lib=rt"); // POSIX real-time extensions (shm_open)
    }
}