fn main() {
    println!("cargo:rerun-if-changed=clipboard-avenger.ico");
    winresource::WindowsResource::new()
        .set_icon("clipboard-avenger.ico")
        .set("ProductName", "Clipboard Avenger")
        .set("FileDescription", "Clipboard Avenger")
        .compile()
        .unwrap();
}
