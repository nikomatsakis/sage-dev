fn main() {
    // Embed the sysroot lib path as an rpath so the binary can find
    // librustc_driver and other rustc dylibs at runtime.
    let output = std::process::Command::new("rustc")
        .arg("--print=sysroot")
        .output()
        .expect("rustc not found");
    let sysroot = String::from_utf8(output.stdout).unwrap();
    let sysroot = sysroot.trim();
    println!("cargo::rustc-link-arg=-Wl,-rpath,{sysroot}/lib");
    // Also expose the sysroot as a compile-time env var
    println!("cargo::rustc-env=SAGE_SYSROOT={sysroot}");
}
