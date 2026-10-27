// Icône de l'exécutable Windows (celle de l'explorateur et de la barre des tâches).
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("icône Windows");
    }
}
