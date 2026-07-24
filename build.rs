// build.rs — ajoute le répertoire racine au chemin de recherche du linker
// afin que boot2.x et memory.x soient trouvés par rust-lld.
//
// NOTE convergence : la branche équipe utilisait un build.rs multi-cibles
// (RP2040/RP2350 via .pico-rs) qui réécrivait .cargo/config.toml à chaque
// build. Conservé dans l'historique git — à réintroduire si le support
// RP2350 devient réel. Cette version simple est celle validée sur matériel.
fn main() {
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=boot2.x");
    // Ajoute le répertoire courant (racine du projet) au chemin du linker
    println!("cargo:rustc-link-search={}", std::env::current_dir().unwrap().display());
}
