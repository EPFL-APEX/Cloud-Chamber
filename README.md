# Cloud Chamber — Firmware

Firmware embarqué pour une chambre à brouillard, écrit en Rust `no_std`.  
Cible principale : **RP2040** (Raspberry Pi Pico) avec support prévu **RP2350** (Pico 2).

---

## Table des matières

1. [Vue d'ensemble](#vue-densemble)
2. [Matériel requis](#matériel-requis)
3. [Prérequis logiciels](#prérequis-logiciels)
4. [Mise en route rapide](#mise-en-route-rapide)
5. [Architecture du projet](#architecture-du-projet)
6. [Drivers disponibles](#drivers-disponibles)
7. [Binaires de test](#binaires-de-test)
8. [Cibler le RP2350](#cibler-le-rp2350)
9. [Tests unitaires desktop](#tests-unitaires-desktop)

---

## Vue d'ensemble

Le firmware gère deux cœurs en parallèle :

```
Core0 — initialisation, UI, logging
Core1 — boucle de sécurité à 100 Hz (seuils, alarmes, disjoncteur)
         ↕ SHARED : Mutex<RefCell<SharedState>>
```

Les capteurs lents (DS18B20 : ~800 ms par mesure) sont lus sur **Core0**. La boucle de sécurité sur Core1 ne fait que lire les dernières valeurs connues via la zone partagée.

---

## Matériel requis

| Composant | Broche |
|-----------|--------|
| DS18B20 (1-Wire, température) | GP15 — pull-up 4,7 kΩ vers 3,3 V obligatoire |
| BME280 (I²C, temp + humidité + pression) | SDA → GP4, SCL → GP5 |
| ABP2 basse pression 0–1 bar (I²C) | SDA → GP4, SCL → GP5, adresse `0x28` |
| ABP2 haute pression 0–12 bar (I²C) | SDA → GP4, SCL → GP5, adresse `0x38` |
| Relais compresseur | GP16 |

> Les broches et adresses sont centralisées dans [`src/config.rs`](src/config.rs).

---

## Prérequis logiciels

### Rust et cibles embarquées

```bash
# Installer rustup si nécessaire : https://rustup.rs
rustup target add thumbv6m-none-eabi     # RP2040
rustup target add thumbv8m.main-none-eabihf  # RP2350 (optionnel)
```

### Outils de flash et de débogage

```bash
# Linker wrapper (protection stack overflow)
cargo install flip-link

# Flasher via sonde SWD (recommandé)
cargo install probe-rs-tools

# OU flasher via USB (glisser-déposer UF2)
# Télécharger picotool : https://github.com/raspberrypi/picotool
```

### Sélectionner la puce cible

Écrire `rp2040` ou `rp2350` dans le fichier `.pico-rs` à la racine :

```bash
echo rp2040 > .pico-rs   # Raspberry Pi Pico
echo rp2350 > .pico-rs   # Raspberry Pi Pico 2
```

Le `build.rs` lit ce fichier et configure automatiquement le linker script et les `cfg` flags (`rp2040` / `rp2350`).

---

## Mise en route rapide

```bash
# 1. Cloner le dépôt
git clone <url-du-repo>
cd Cloud-Chamber

# 2. Sélectionner la cible
echo rp2040 > .pico-rs

# 3. Vérifier que tout compile
cargo check

# 4. Flasher le firmware principal (sonde SWD branchée)
cargo run

# 5. Voir les logs defmt en temps réel
#    probe-rs run (utilisé dans cargo run) affiche les logs automatiquement
```

---

## Architecture du projet

```
src/
├── main.rs                  — Point d'entrée Core0
├── core1.rs                 — Tâche Core1 (boucle de sécurité)
├── config.rs                — Broches GPIO, adresses I²C, seuils de sécurité
├── lib.rs                   — Réexporte tous les modules (accès depuis les binaires de test)
│
├── cloud_chamber_hal/       — Traits abstraits (interfaces matériel)
│   ├── sensors.rs           — TemperatureSensor, PressureSensor, VoltageSensor, CurrentSensor…
│   ├── actuators.rs         — BreakerActuator, VoltageController
│   └── timer.rs             — MonotonicTimer, WatchdogFeed
│
├── drivers/                 — Implémentations concrètes
│   ├── ds18b20.rs           — Capteur température 1-Wire (avec support clones)
│   ├── bme280.rs            — Température + humidité + pression (I²C)
│   ├── abp2.rs              — Capteur pression Honeywell (I²C)
│   ├── adc.rs               — Tension et courant via ADC embarqué
│   ├── breaker.rs           — Disjoncteur via relais GPIO
│   ├── closure.rs           — Détection fermeture de chambre (contact sec)
│   ├── display.rs           — Écran ILI9341
│   └── encoder.rs           — Encodeur rotatif
│
├── security_loop/           — Boucle de sécurité (Core1, 100 Hz)
│   ├── loop_runner.rs       — Boucle principale
│   ├── safety.rs            — Évaluation des seuils
│   ├── states.rs            — Machine à états du système
│   └── error.rs             — Types d'erreurs de la boucle
│
├── shared/                  — Données partagées Core0 ↔ Core1
│   ├── data.rs              — SensorSnapshot, SystemState, SHARED (Mutex global)
│   ├── error.rs             — Types d'erreurs génériques
│   └── ring_buffer.rs       — Buffer circulaire pour l'historique
│
├── ui/                      — Interface graphique (Core0)
│   ├── navigator.rs         — Pile de navigation entre écrans
│   ├── screens/             — Écran de status, menu…
│   ├── theme.rs             — Couleurs et polices
│   └── widgets.rs           — Composants réutilisables
│
└── bin/                     — Binaires de test indépendants
    ├── test_capteurs.rs     — DS18B20 + BME280, sortie USB série
    ├── test_temperature.rs  — DS18B20 seul, sortie defmt/RTT
    ├── test_voltage.rs      — Capteur tension ADC
    ├── test_current.rs      — Capteur courant ADC
    ├── test_breaker.rs      — Disjoncteur GPIO
    ├── test_closure.rs      — Capteur de fermeture
    └── test_encoder.rs      — Encodeur rotatif
```

### Principe d'inversion de dépendance

```
security_loop  →  cloud_chamber_hal (traits)
                         ↑
                    drivers/ (implémentations)
```

La boucle de sécurité dépend uniquement des **traits** (`TemperatureSensor`, etc.), jamais des drivers concrets. On peut substituer n'importe quel driver sans toucher à la logique de sécurité.

---

## Drivers disponibles

### DS18B20 — Température 1-Wire

```rust
use cloud_chamber::drivers::ds18b20::{Ds18b20Bus, Ds18b20Sensor, Resolution};

// Découverte sur le bus (supporte les clones via SKIP ROM)
let mut bus = Ds18b20Bus::new(pin);
let count = bus.discover(&mut delay);

// Lecture directe via le bus
bus.start_conversion(0, &mut delay)?;
delay.delay_ms(Resolution::Bits9.conversion_time_ms());
let temp = bus.read_celsius(0, &mut delay)?;

// OU via le wrapper TemperatureSensor (résolution configurée une seule fois)
let mut sensor = Ds18b20Sensor::new(bus, delay, 0, Resolution::Bits9)?;
sensor.start_measurement()?;  // envoie Convert T + attend
let temp = sensor.read_celsius()?;
```

| Résolution | Précision  | Durée max |
|------------|------------|-----------|
| `Bits9`    | ± 0,5 °C   | 150 ms    |
| `Bits10`   | ± 0,25 °C  | 240 ms    |
| `Bits11`   | ± 0,125 °C | 430 ms    |
| `Bits12`   | ± 0,0625 °C| 800 ms    |

> La résolution est envoyée au capteur **une seule fois** (à la construction ou via `set_resolution()`). Les appels à `start_measurement()` n'envoient jamais WriteScratchpad.

### BME280 — Température + Humidité + Pression (I²C)

```rust
use cloud_chamber::drivers::bme280::{Bme280Driver, Bme280Sensor};

let mut driver = Bme280Driver::new(i2c);
driver.init()?;

let (temp_c, pressure_hpa, humidity_pct) = driver.measure(&mut delay)?;

// OU via le trait TemperatureSensor
let mut sensor = Bme280Sensor::new(driver, delay);
sensor.init()?;
sensor.start_measurement()?;
let temp = sensor.read_celsius()?;
```

### ABP2 — Pression Honeywell (I²C)

```rust
use cloud_chamber::drivers::abp2::Abp2Driver;
use cloud_chamber::config::{ABP2_BP_ADDR, BP_PRESSURE_MIN, BP_PRESSURE_MAX};

let mut bp = Abp2Driver::new(i2c, ABP2_BP_ADDR, BP_PRESSURE_MIN, BP_PRESSURE_MAX);
let reading = bp.read()?;  // Abp2Reading { pressure_bar, temperature_c }
let pa = bp.read_pascal()?;
```

---

## Binaires de test

Chaque binaire teste un driver ou un périphérique en isolation. Utile pour valider le câblage sans avoir à flasher le firmware complet.

```bash
# DS18B20 + BME280 — sortie via USB série (minicom, PuTTY…)
cargo run --bin test_capteurs

# DS18B20 seul — sortie defmt via sonde SWD
cargo run --bin test_temperature

# Autres
cargo run --bin test_voltage
cargo run --bin test_current
cargo run --bin test_breaker
cargo run --bin test_closure
cargo run --bin test_encoder
```

> `test_capteurs` : ouvrir un moniteur série après le flash (baudrate automatique USB CDC).  
> Les autres : les logs apparaissent directement dans le terminal via `probe-rs`.

---

## Cibler le RP2350

```bash
echo rp2350 > .pico-rs
cargo run
```

Le `build.rs` met à jour automatiquement `.cargo/config.toml` avec la bonne cible et le bon chip pour `probe-rs`.

> Pour un Pico 2 en mode RISC-V : `echo rp2350-riscv > .pico-rs`

---

## Tests unitaires desktop

Les modules `shared/` et `drivers/` contiennent des tests qui tournent sur la machine de développement (sans matériel) :

```bash
cargo test-host
# équivalent à : cargo test --target x86_64-pc-windows-msvc --lib
```

> Sur Linux/macOS, remplacer la cible par `x86_64-unknown-linux-gnu` ou `aarch64-apple-darwin` dans `.cargo/config.toml`.
