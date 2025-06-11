fn main() {
    // Make it possible to use the JSON extension in binaries (binary crates or tests).
    // See https://docs.rs/kuzu/latest/kuzu/#using-extensions
    println!("cargo:rustc-link-arg=-rdynamic");
}
