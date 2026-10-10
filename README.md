# Cloud Chamber — Firmware

Firmware embarqué pour une chambre à brouillard, en Rust `no_std`.
Cible : **RP2040** (Pico), support **RP2350** (Pico 2) prévu par construction.

> ### /!\ Projet en cours — ne pas faire tourner sans surveillance
>
> Le firmware démarre, s'affiche et lance un cycle, mais il n'est **ni
> terminé ni validé**, et il pilote une haute tension. Manques les plus
> structurants :
>
> - une panique du cœur de contrôle **laisse les relais en l'état**, et
>   l'écran continue comme si de rien n'était ;
> - la seule sécurité active est la température de sortie compresseur —
>   **aucune surveillance de pression ni de surintensité** ;
> - après un déclenchement, seul un reflash réarme la machine ;
> - plusieurs broches et seuils sont encore `TODO CÂBLAGE` / `TODO CALIBRAGE`.
>
> Détail complet : [État du projet](#état-du-projet).

---

## Vue d'ensemble

Le firmware occupe **les deux cœurs**, avec une frontière nette :

```
Cœur 1 — contrôle                     Cœur 0 — interface
control_loop::run()                   TIMER_IRQ_0 (1 ms) : scrute l'encodeur
  sonde → réconcilie → décide            → empile dans EVENTS
  → applique aux actionneurs           boucle : dépile → UiApp → redessine
  → publie l'état
            ↕                                      ↕
     SHARED_STATE (critical_section::Mutex<RefCell<_>>) + shared::settings
```

Le cœur 1 possède capteurs et actionneurs, le cœur 0 possède l'écran :
rien n'est partagé hors de `SHARED_STATE`. La sécurité n'est pas une boucle
séparée, c'est une transition prioritaire évaluée à chaque tour du contrôle.

> **Piège.** Sur RP2040, `critical-section-impl` est un **spinlock matériel
> partagé par les deux cœurs**, pas un masquage d'interruptions local : une
> section critique tenue longtemps d'un côté met l'autre en attente active.
> D'où la règle — **rien ne dessine ni ne fait d'E/S sous verrou**. La boucle
> d'affichage copie `SharedState` sous verrou court, puis dessine hors
> verrou. Sinon un rendu plein écran bloquerait le cœur 1 en pleine séquence
> 1-Wire, dont le décodage dépend d'un timing à la microseconde.

---

## Matériel

Câblage réel dans [`src/config/wiring.rs`](src/config/wiring.rs), qui
**vérifie à la compilation** qu'aucune broche n'est réclamée deux fois.

| Rôle | Broche | Constante |
|------|--------|-----------|
| Bus 1-Wire, 8× DS18B20 | GP23 — pull-up 4,7 kΩ **obligatoire** | `PIN_ONEWIRE` |
| I²C0 — BME280 pression (`0x76`, ambiant absolu) | SDA GP20 / SCL GP21 | `PIN_I2C_*` |
| Relais compresseur | GP5 | `PIN_COMPRESSOR_RELAY` |
| Relais haute tension | GP14 | `PIN_HV_RELAY` |
| Relais chauffage isopropanol | GP9 | `PIN_ISO_HEATER_RELAY` |
| Relais pompe / éclairage / chauffage vitre | GP7 / GP8 / GP10 | `PIN_PUMP_RELAY`… |
| Encodeur A / B / bouton | GP26 / GP27 / GP28 | `PIN_ENCODER_*` |
| Écran ILI9341 — SCK / MOSI (SPI0) | GP18 / GP19 | `PIN_SCREEN_*` |
| Écran — CS / DC / RESET (GPIO) | GP22 / GP16 / GP17 | `PIN_SCREEN_*` |

**Sorties de puissance à 8 mA** (`RELAY_DRIVE_STRENGTH`), pas les 4 mA par
défaut : le MOC3043 demande jusqu'à 5 mA dans sa LED, et à 4 mA la tension
de sortie s'effondre sous ~10 mA — amorçage aléatoire. La valeur est relue
depuis le registre au démarrage et journalisée. Les broches de l'écran
gardent 4 mA (entrées CMOS, à côté d'un SPI à 32 MHz).

### Drivers

Intégrés : **DS18B20** (1-Wire, accès registre direct, clones supportés) ·
**BME280** (pression I²C) · **ILI9341** (SPI + framebuffer RAM) · **rotary encodeur** (bouton débruité) · relais **compresseur** et
**chauffage** (hystérésis, sens opposés) · relais tout-ou-rien
(**haute tension**, **pompe**, **éclairage**, **chauffage vitre**).

Présents mais non câblés : **ABP2** · **ADC** tension/courant ·
**capteur de fermeture** · **triac zero-cross PIO** · **stockage flash**
(trait `FlashOps` sans implémentation RP2040).

**Index des sondes** : l'ordre SEARCH ROM n'est pas garanti d'un montage à
l'autre — le vérifier avec `identify_temp_sensors` avant de se fier à
`CHAMBER_TEMP_IDX` & co. (`cloud_chamber_hal/config.rs`).

---

## Démarrage

```bash
rustup target add thumbv6m-none-eabi            # RP2040
cargo install flip-link probe-rs-tools

echo rp2040 > .pico-rs   # ou rp2350 / rp2350-riscv ; build.rs en déduit la cible

cargo run --release --target thumbv6m-none-eabi \
    --features bin-cloud-chamber --bin cloud_chamber
```

`flip-link` n'est pas optionnel : il place la pile pour qu'un débordement
tombe en mémoire non mappée (HardFault net) plutôt que d'écraser les
`static`.

> **Toujours flasher en `--release`.** En debug, `embedded-graphics` est
> beaucoup plus lent : un rendu plein écran passe de quelques dizaines de ms à
> plusieurs secondes. Les deux profils fonctionnent, mais l'UI est
> inutilisable en debug.

Le firmware **panique volontairement** si un capteur ne répond pas au
premier sondage ; le message donne le masque des index muets. Il faut donc
les **9 capteurs présents** (8 températures + 1 pression) pour démarrer.

> La pression vient aujourd'hui du **BME280**, qui mesure l'ambiant absolu
> (~1013 hPa) — pas la pression d'un circuit de la chambre.

Sans matériel :

```bash
cargo test-host-linux      # suite complète, aucune cible embarquée requise
cargo test-live-ui-linux   # fenêtre SDL2 sur l'UI réelle (optionnel)
```

Binaires de bring-up, chacun derrière `--features bin-<nom>` : `blinky`,
`identify_temp_sensors`, `relay_test`, `bme_test`, `screen_test`,
`encoder_test`, `ui_test`.

---

## Architecture

```
src/
├── main.rs              — Point de composition : matériel réel, lancement des deux cœurs
├── config/              — Propre à CETTE installation : wiring, operating, settings
│
├── cloud_chamber_hal/   — Traits abstraits, génériques (aucune dépendance vers logic/)
│   ├── sensors.rs       — Sensor, BatchSensor, DeferredBatchSensor, Sensors<Tmp, Prs>
│   ├── actuators.rs     — BinaryActuator, TargetActuator, Actuators<..>, ActuatorPlan
│   └── timer.rs, measurement.rs, ring_buffer.rs, units.rs, config.rs
│
├── drivers/             — Implémentations concrètes (cf. liste ci-dessous)
│
├── logic/               — Machine à états (le cœur du firmware), pur et testé
│   ├── control_loop.rs  — run() + tick() (un tour, testable)
│   ├── cooling.rs       — Séquence de démarrage (6 phases) · stopping.rs (3 phases)
│   ├── phase_clock.rs   — Durées/timeouts par phase, advance()
│   └── security.rs, probing.rs, timing.rs
│
├── shared/              — Frontière entre les deux cœurs : data.rs, settings.rs
├── ui/                  — app.rs (sommet), router.rs, navigator.rs, screens/
└── comm/                — Liaison USB (feature `usb-comm`, cassée)
```

### Inversion de dépendance

`logic/` ne connaît que des traits, jamais un driver concret :
`control_loop::run()` est générique sur `Sensors<Tmp, Prs>`,
`Actuators<Hv, Cool, Iso, Pump, Lights, Glass>` et `Clk`. Toute la logique
se teste donc avec des mocks, sans matériel. `cloud_chamber_hal` ne dépend
jamais de `logic/`, un type qui doit vivre dans le HAL pour l'ergonomie de
l'API (ex. `ActuatorPlan`) y est déplacé, jamais importé à l'envers.

### Décision / application, à tous les étages

Une phase décide sa transition **et** construit son `ActuatorPlan`, sans
toucher au matériel ; `Actuators::apply()` ne fait qu'exécuter. Même
principe côté UI : un écran renvoie une décision (`NavAction`, demande
d'état) et n'agit jamais lui-même. C'est ce qui rend les deux couches
testables sans matériel ni `static`.

---

## La boucle de contrôle

```rust
pub enum SystemTask {
    Idle,
    Cooling(CoolingPhase),   // SensorCheck → PreCooling → IpaCirculation
                             //   → Saturation → HighVoltage → FinalCheck
    Stabilising,             // régime permanent, pas de sortie automatique
    Stopping(StoppingPhase), // CutHighVoltage → CutCompressor → WaitPressure
    Tripped(SafetyCause),    // verrouillé jusqu'à réarmement opérateur
}
```

L'UI écrit dans `SHARED_STATE.task` pour demander un démarrage — pas de
canal séparé. Deux règles protègent contre les races : `tick()` **adopte**
en début de tour toute écriture externe, et **publie** en fin de tour par
compare-and-swap (si l'UI a écrit entre-temps, la décision calculée sur la
base périmée est abandonnée). Conséquence : une transition décidée par le
contrôleur lui-même prime toujours sur une valeur restée en retard, sans
cas particulier par phase.

Garde symétrique côté UI (`ui::router`) : un démarrage n'est accordé que
depuis `Idle` — rappuyer sur le bouton pendant un cycle affiche le suivi
sans relancer la séquence.

---

## Tests

272 tests, tous sur poste (`#![cfg_attr(not(test), no_std)]`) :

- **Logique pure** — chaque transition, chaque timeout, `SafetyMonitor`.
- **`control_loop`** — `run()` boucle indéfiniment, donc son corps est
  extrait dans `tick()`. Une suite d'intégration (mocks + `Harness`)
  enchaîne les tours comme en usage réel : cycle complet Idle →
  Stabilising → Idle, abandons timeout/perte-capteur, priorité sécurité,
  réconciliation dans les deux sens (dont la race publication/écriture UI).
- **UI** — navigation, garde de démarrage, mapping événement → action,
  captures d'écran headless.
- **Drivers** — décodage encodeur (rebond, faux appuis), hystérésis,
  framebuffer, stockage flash, conversions.

---

## État du projet

L'architecture (séparation HAL/logic, décision/application, réconciliation,
répartition bi-cœur) est traitée comme stable. Le reste est du travail
d'intégration et de durcissement.

### Sécurité

- **Une panique coupe tout et le dit** : le gestionnaire de panique
  (`drivers/safe_panic.rs` + `main.rs`) force d'abord les six relais au
  niveau bas par accès registre direct, puis publie un drapeau dans
  `SHARED_STATE` que la boucle du cœur 0 transforme en écran d'erreur —
  plus d'affichage d'une machine saine pendant que le contrôle est mort.
  L'écran d'erreur porte un bouton **REDEMARRER** : le clic coupe le
  chien de garde et force un reset immédiat ; à défaut, le watchdog
  armé réinitialise la machine au bout de 60 s.
- **Pas de sécurité pression, par design** : la chambre n'est pas
  pressurisée — le BME280 mesure la pression atmosphérique ambiante,
  une **donnée d'expérience** (elle entre dans le calcul des conditions
  de nucléation), pas un danger à surveiller. Le jour où un circuit
  serait pressurisé, le `match` exhaustif de `SafetyMonitor::severity_of`
  imposera de dire comment le mesurer.
- **Réarmement opérateur câblé** : un clic sur l'écran de suivi pendant un
  déclenchement demande `Idle`, que `control_loop::tick` lit comme
  acquittement — `Tripped` n'est plus définitif jusqu'au reflash.
- **Broches et seuils provisoires** (`TODO CÂBLAGE`, `IPA_HEATER_TARGET_C`).

### Manquant

- **Écran de suivi à boutons** : l'action principale suit l'état —
  **START** à l'arrêt, **STOP** en course (séquence d'arrêt complète de
  `logic::stopping`, coupure HV d'abord, pas un `Idle` sec),
  **ACQUITTER** en déclenchement — plus un bouton **BACK**. La rotation
  déplace la sélection entre les deux, le clic valide : deux boutons,
  un seul bouton physique, le pattern encodeur.
- **Surintensité** : les drivers `AdcCurrentSensor`/`AdcVoltageSensor`
  existent et sont testés, mais aucune broche n'y est affectée et aucun
  `SafetyCause` ne les consomme — à câbler avec le shunt réel.
- **Écran `ManualControl`** : encore un carton d'attente — le mode manuel
  n'existe pas.
- **`comm/`** : réparé (protocole décorrélé de `logic/`, écrit dans
  `SHARED_STATE` comme l'UI ; commandes `CYCLE 0|1`, `ACK`, `STATE`), mais
  **pas de point d'entrée** : aucun binaire ne lit le port série pour
  l'instant.