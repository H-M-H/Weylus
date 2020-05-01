fn main() {
    println!("cargo:rerun-if-changed=src/get_sizeof_window.c");
    cc::Build::new()
        .file("src/get_sizeof_window.c")
        .compile("size");
    println!("cargo:rustc-link-lib=X11");
}
