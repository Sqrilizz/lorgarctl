fn main() {
    cc::Build::new()
        .file("src/uhid_battery.c")
        .compile("uhid_battery");
    println!("cargo:rerun-if-changed=src/uhid_battery.c");
}
