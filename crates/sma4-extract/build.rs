// Unicorn's bundled QEMU uses 128-bit atomics, which GCC/Clang lower to libatomic calls.
fn main() {
    println!("cargo:rustc-link-lib=atomic");
}
