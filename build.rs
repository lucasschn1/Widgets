//! Embute no Widgets.exe o manifesto do Windows (controles modernos, escala por monitor) e,
//! quando existir, o ícone res/widgets.ico.

fn main() {
    println!("cargo:rerun-if-changed=res/widgets.manifest");
    println!("cargo:rerun-if-changed=res/widgets.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_manifest_file("res/widgets.manifest");
    if std::path::Path::new("res/widgets.ico").exists() {
        res.set_icon("res/widgets.ico");
    }
    res.set("FileDescription", "Widgets da área de trabalho");
    res.set("ProductName", "Widgets");
    res.set("CompanyName", "lucasschn1");
    res.set("LegalCopyright", "lucasschn1");
    if let Err(e) = res.compile() {
        println!("cargo:warning=não deu para embutir o manifesto: {e}");
    }
}
