// build.rs — ajoute le répertoire racine au chemin de recherche du linker
// afin que boot2.x et memory.x soient trouvés par rust-lld.
fn main() {
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=boot2.x");
    // Ajoute le répertoire courant (racine du projet) au chemin du linker
    println!("cargo:rustc-link-search={}", std::env::current_dir().unwrap().display());
}
