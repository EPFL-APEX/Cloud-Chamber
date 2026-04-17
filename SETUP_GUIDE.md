# Guide d'installation — Environnement Rust/Embassy sur VS Code

## 1. Installer Rust

```bash
# Installer rustup (gestionnaire de toolchain Rust)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Choisir l'option 1 (installation par défaut)

# Recharger le PATH
source $HOME/.cargo/env

# Passer en toolchain nightly (requis par Embassy)
rustup default nightly
rustup update

# Ajouter la cible ARM Cortex-M0+ (RP2040)
rustup target add thumbv6m-none-eabi

# Installer l'outil de flash USB
cargo install elf2uf2-rs

# Optionnel : installer probe-rs pour debug avancé
# cargo install probe-rs-tools
```

## 2. Configurer VS Code

### Extensions à installer

Ouvrir VS Code → Ctrl+Shift+X (Extensions) :

| Extension | ID | Rôle |
|---|---|---|
| **rust-analyzer** | `rust-lang.rust-analyzer` | Autocomplétion, diagnostics, navigation |
| Even Better TOML | `tamasfe.even-better-toml` | Coloration Cargo.toml |
| Error Lens | `usernamehw.errorlens` | Erreurs affichées inline |
| cortex-debug | `marus25.cortex-debug` | Debug embarqué (optionnel) |

### Settings workspace

Créer `.vscode/settings.json` dans le dossier du projet :

```json
{
    "rust-analyzer.cargo.target": "thumbv6m-none-eabi",
    "rust-analyzer.check.allTargets": false,
    "rust-analyzer.check.noDefaultFeatures": true,
    "rust-analyzer.imports.granularity.group": "module"
}
```

## 3. Compiler le projet

```bash
cd cloud_chamber_firmware_rust

# Compilation (mode debug)
cargo build

# Compilation optimisée (mode release)
cargo build --release
```

## 4. Flasher le Pico W

### Méthode UF2 (sans debug probe)

1. Maintenir le bouton **BOOTSEL** du Pico W enfoncé
2. Brancher le câble USB → le Pico apparaît comme clé USB
3. Lancer :
```bash
cargo run --release
```
`elf2uf2-rs` convertit et copie automatiquement le firmware.

### Méthode probe-rs (avec debug probe)

```bash
cargo run --release
# Le runner dans .cargo/config.toml utilise probe-rs
```

## 5. Voir les logs (defmt)

Les messages `defmt::info!()`, `defmt::warn!()`, `defmt::error!()`
sont transmis via RTT (Real-Time Transfer). Pour les lire :

```bash
# Avec probe-rs
probe-rs attach --chip RP2040
```

Sans debug probe, les logs ne sont pas visibles directement.
Alternative : ajouter un output UART dans le code pour du logging série.

## 6. Structure du projet

```
cloud_chamber_firmware_rust/
├── .cargo/config.toml     ← Target ARM + runner
├── Cargo.toml             ← Dépendances (Embassy, drivers)
├── build.rs               ← Linker script setup
├── memory.x               ← Memory layout RP2040
└── src/
    ├── main.rs            ← Core 0 : Flow Controller
    │                         (boucle critique capteurs + sécurité)
    ├── config.rs          ← Constantes modifiables (WiFi, pins, seuils)
    ├── data.rs            ← Structures partagées inter-cœurs (Mutex)
    ├── sensors/
    │   ├── mod.rs         ← Re-export des drivers
    │   ├── ds18b20.rs     ← Driver 1-Wire multi-capteurs
    │   └── abp2.rs        ← Driver I²C pression Honeywell
    └── network/
        ├── mod.rs         ← Re-export réseau
        ├── wifi.rs        ← Connexion WiFi CYW43
        └── http_server.rs ← Serveur HTTP (JSON API + dashboard)
```

## 7. Comment étendre le projet

### Ajouter un nouveau capteur

1. Créer `src/sensors/mon_capteur.rs` avec les fonctions `init()` et `read()`
2. L'ajouter dans `src/sensors/mod.rs` : `pub mod mon_capteur;`
3. L'utiliser dans `main.rs` dans la phase appropriée du flow controller

### Ajouter un module de contrôle (ex: PID)

1. Créer `src/control/mod.rs` et `src/control/pid.rs`
2. Le module lit les données depuis `SHARED_STATE`
3. Il écrit ses commandes (ex: vitesse ventilateur) dans `SHARED_STATE`
4. L'intégrer dans la phase "Reaction to readings" du flow controller

### Ajouter du logging sur fichier (SD card)

1. Créer `src/logging/mod.rs`
2. Utiliser le SPI du Pico W pour une carte SD
3. Le module tourne sur Core 1, lit `SHARED_STATE` périodiquement
