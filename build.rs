use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=ts/lib.ts");
    match Command::new("tsc")
        .status()
    {
        Err(err) => {
            println!("cargo:warning=Failed to call tsc: {}", err);
            std::process::exit(1);
        }
        Ok(status) => {
            if !status.success() {
                match status.code() {
                    Some(code) => println!("cargo:warning=tsc failed with exitcode: {}", code),
                    None => println!("cargo:warning=tsc terminated by signal."),
                };
                std::process::exit(2);
            }
        }
    }

    println!("cargo:rerun-if-changed=lib/linux/error.h");
    println!("cargo:rerun-if-changed=lib/linux/error.c");
    println!("cargo:rerun-if-changed=lib/linux/uniput.c");
    println!("cargo:rerun-if-changed=lib/linux/capture.c");
    cc::Build::new()
        .file("lib/linux/error.c")
        .file("lib/linux/uinput.c")
        .file("lib/linux/capture.c")
        .compile("linux");
    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=Xext");
}
