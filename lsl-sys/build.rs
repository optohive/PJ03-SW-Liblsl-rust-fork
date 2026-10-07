use std::env;
use std::path::Path;

fn main() {
    // Cargo only re-runs a build script when something it has been told about
    // changes. Without these, altering either variable leaves a cached
    // build-script output in place: one bad build keeps being replayed after
    // the cause is fixed, which is near-impossible to diagnose from the logs
    // because the script does not run and prints nothing.
    println!("cargo:rerun-if-env-changed=LIBLSL_LIB_DIR");
    println!("cargo:rerun-if-env-changed=HOMEBREW_PREFIX");

    // Check for system liblsl
    if link_to_system_liblsl() {
        return;
    }

    println!("cargo:warning={}", "rebuilding...");
    build_liblsl();
}

// Try to link to a system-installed dynamic liblsl
fn link_to_system_liblsl() -> bool {
    // If lsl path set with environment variable.
    //
    // An empty value is treated as unset: env::var returns Ok("") for a
    // variable that is set but empty, and blanking a variable is the usual way
    // to try to neutralise it (a GitHub Actions `env:` block cannot unset an
    // inherited variable at all, only blank it). Taking this branch with an
    // empty value emitted `rustc-link-search=native=` and failed the build with
    // `error: empty search path given via -L`.
    if let Some(lib_path) = env::var("LIBLSL_LIB_DIR")
        .ok()
        .filter(|p| !p.trim().is_empty())
    {
        if !Path::new(&lib_path).is_dir() {
            panic!("LIBLSL_LIB_DIR is set to {lib_path:?}, which is not a directory");
        }
        println!("cargo:rustc-link-search=native={}", lib_path);
        println!("cargo:rustc-link-lib=dylib=lsl");
        // cargo:warning, not a bare println: cargo only surfaces lines that
        // begin with `cargo:`, so plain output is discarded. These are the
        // diagnostics that say whether linkage ended up static or dynamic.
        println!("cargo:warning=liblsl: linking dynamically against {lib_path}");
        return true;
    }

    // Check homebrew path on macOS
    #[cfg(target_os = "macos")]
    {
        if let Ok(homebrew) = env::var("HOMEBREW_PREFIX") {
            let path = &format!("{}/lib", homebrew);
            let lib_file = Path::new(path).join("liblsl.dylib");
            if lib_file.exists() {
                println!("cargo:rustc-link-search=native={}", path);
                println!("cargo:rustc-link-lib=dylib=lsl");
                // Said out loud because it is easy to miss and the consequence
                // lands on the user, not the developer: the resulting binary
                // needs this dylib at the same absolute path, so an app built
                // on a machine with Homebrew liblsl fails to start elsewhere
                // with a dyld error.
                println!(
                    "cargo:warning=liblsl: linking DYNAMICALLY against {} (HOMEBREW_PREFIX is set).                      The resulting binary is not portable; unset HOMEBREW_PREFIX to build the                      vendored static copy instead.",
                    lib_file.display()
                );
                return true;
            }
        }
    }
    // TODO: automatic finding for Linux and Windows
    // Fallback to building from source
    false
}

// Build the liblsl library from source using cmake
fn build_liblsl() {
    let target = env::var("TARGET").unwrap();
    
    // build with cmake
    let mut cfg = cmake::Config::new("liblsl");
    cfg
        .define("LSL_NO_FANCY_LIBNAME", "ON")
        .define("LSL_BUILD_STATIC", "ON");
    if target.contains("msvc") {
        // override some C/CXX flags that the cmake crate splices in on Windows
        // (these cause the build to fail)...
        // * /EHsc sets the correct exception handling mode
        // * /GR enables RTTI
        // * /MT links the C and C++ runtimes statically
        //
        // /MT rather than /MD: with /MD the consuming executable imports
        // MSVCP140.dll, which ships with the Visual C++ Redistributable and is
        // not present on a clean Windows install, so every downstream
        // application has to bundle the redistributable or fail to start.
        // Consumers linking this crate should set
        // `-C target-feature=+crt-static` so the Rust half matches.
        let cxx_args = " /nologo /EHsc /MT /GR";
        cfg 
            .define("WIN32", "1")
            .define("_WINDOWS", "1")
            .define("CMAKE_C_FLAGS", cxx_args)
            .define("CMAKE_CXX_FLAGS", cxx_args)
            .define("CMAKE_C_FLAGS_DEBUG", cxx_args)
            .define("CMAKE_CXX_FLAGS_DEBUG", cxx_args)
            .define("CMAKE_C_FLAGS_RELEASE", cxx_args)
            .define("CMAKE_CXX_FLAGS_RELEASE", cxx_args);
    }
    let install_dir = cfg.build();

    // emit link directives
    let libdir = install_dir.join("lib");
    let libname = "lsl";
    println!(
        "cargo:rustc-link-search=native={}",
        libdir.to_str().unwrap()
    );
    println!("cargo:rustc-link-lib=static={}", libname);

    // make sure we also link some additional libs
    if target.contains("linux") {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    } else if target.contains("windows") {
        // TODO: this is a shortcoming in the current cmake file, which should be
        //       linking in these libraries (once this is fixed, we should remove this)
        println!("cargo:rustc-link-lib=dylib=bcrypt");
        // timeBeginPeriod / timeEndPeriod, raising the timer resolution.
        println!("cargo:rustc-link-lib=dylib=winmm");
        // GetAdaptersAddresses, enumerating the interfaces to multicast on.
        println!("cargo:rustc-link-lib=dylib=iphlpapi");
        // Without these, anything that links this crate directly fails with
        // `LNK2019: unresolved external symbol __imp_timeBeginPeriod` and
        // `__imp_GetAdaptersAddresses`. A large application binary often pulls
        // both in transitively, so the gap shows up in examples and
        // integration tests rather than in the application itself.
    } else {
        println!("cargo:rustc-link-lib=dylib=c++");
    }
}
