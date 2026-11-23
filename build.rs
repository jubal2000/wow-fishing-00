fn main() {
    // screencapturekit links the Swift runtime; the final binary needs the rpath.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}
