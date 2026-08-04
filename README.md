# Cloud Chamber — Firmware

Firmware embarqué pour une chambre à brouillard, écrit en Rust `no_std`.
Cible principale : **RP2040** (Raspberry Pi Pico), support **RP2350** (Pico 2) prévu par construction.

---

## Table des matières

1. [Vue d'ensemble](#vue-densemble)
2. [Matériel requis](#matériel-requis)
3. [Prérequis logiciels](#prérequis-logiciels)
4. [Mise en route](#mise-en-route)
5. [Architecture du projet](#architecture-du-projet)
6. [La boucle de contrôle](#la-boucle-de-contrôle)
7. [`cloud_chamber_hal` — traits et inversion de dépendance](#cloud_chamber_hal--traits-et-inversion-de-dépendance)
8. [Drivers disponibles](#drivers-disponibles)
9. [Interface utilisateur](#interface-utilisateur)
10. [Tests](#tests)
11. [État du projet / limites connues](#état-du-projet--limites-connues)

---

## Vue d'ensemble

Le firmware tourne sur un **seul cœur**. Il n'y a pas de boucle de sécurité
séparée à 100 Hz sur un second cœur : la sécurité est une source de
transition prioritaire évaluée à chaque tour de la boucle de contrôle
principale, au même titre que la logique de phase.

```
logic::control_loop::run()
  └─ boucle : sonde les capteurs
              → réconcilie l'état avec SHARED_STATE (écritures externes de l'UI)
              → sécurité (priorité absolue) ou logique de phase
              → applique le plan aux actionneurs
              → publie l'état (si rien n'a changé entre-temps côté SHARED_STATE)
                    ↕ SHARED_STATE : Mutex<RefCell<SharedState>>
                                     lu par l'UI (et tout autre lecteur)
```

`SHARED_STATE` reste un `static` partagé façon multi-cœur (`critical_section::Mutex<RefCell<_>>`)
même si tout tourne aujourd'hui sur un seul cœur — ça garde la porte ouverte
à un futur retour à un second cœur sans revoir le modèle de données, et ça
donne déjà une frontière claire entre « ce qui contrôle » (`control_loop`)
et « ce qui lit » (l'UI, ou tout autre consommateur).

---

## Matériel requis

| Composant | Broche | Constante (`config.rs`) |
|-----------|--------|--------------------------|
| 5× DS18B20 (1-Wire, température) | GP15 — pull-up 4,7 kΩ vers 3,3 V obligatoire | `PIN_ONEWIRE` |
| ABP2 basse pression 0–1 bar (I²C) | SDA → GP4, SCL → GP5, adresse `0x28` | `ABP2_BP_ADDR` |
| ABP2 haute pression 0–12 bar (I²C) | SDA → GP4, SCL → GP5, adresse `0x38` | `ABP2_HP_ADDR` |
| Relais compresseur | GP16 | `PIN_COMPRESSOR_RELAY` |
| Relais haute tension | GP17 *provisoire, jamais vérifié sur le montage réel* | `PIN_HV_RELAY` |
| Relais chauffage isopropanol | GP18 *provisoire, jamais vérifié sur le montage réel* | `PIN_ISO_HEATER_RELAY` |

L'ordre de découverte des 5 sondes DS18B20 sur le bus 1-Wire (SEARCH ROM)
n'est pas garanti fixe d'un montage à l'autre — vérifier au boot (logs
`INFO ds{i}`) avant de faire confiance aux index (`CHAMBER_TEMP_IDX`,
`COMPRESSOR_OUT_IDX`, `ISO_TEMP_IDX` dans `cloud_chamber_hal/config.rs`).

Un ampèremètre (`NUMBER_OF_AMPMETER`) est prévu dans la forme du HAL mais
n'est pas encore câblé dans `Sensors`/`SensorSnapshot` — pas de protection
surintensité logicielle pour l'instant.

---

## Prérequis logiciels

### Rust et cibles embarquées

```bash
rustup target add thumbv6m-none-eabi          # RP2040
rustup target add thumbv8m.main-none-eabihf   # RP2350 (Cortex-M33)
rustup target add riscv32imac-unknown-none-elf # RP2350 (mode RISC-V)
```

### Outils de flash et de débogage

```bash
cargo install flip-link       # linker wrapper (protection stack overflow)
cargo install probe-rs-tools  # flash + logs via sonde SWD
```

### Sélectionner la puce cible

```bash
echo rp2040 > .pico-rs        # Raspberry Pi Pico
echo rp2350 > .pico-rs        # Raspberry Pi Pico 2 (Cortex-M33)
echo rp2350-riscv > .pico-rs  # Raspberry Pi Pico 2 (RISC-V)
```

`build.rs` lit ce fichier à chaque build et met à jour `.cargo/config.toml`
(cible, linker script, `cfg(rp2040)`/`cfg(rp2350)`) en conséquence.

---

## Mise en route

```bash
git clone <url-du-repo>
cd Cloud-Chamber
echo rp2040 > .pico-rs
cargo check
```

> **Pas encore de firmware flashable.** Il n'existe aujourd'hui aucun
> `src/main.rs` ni binaire enregistré (`autobins = false`, aucun `[[bin]]`
> dans `Cargo.toml`) : rien ne construit encore les `Pins`/`I2c`/ADC réels
> ni n'assemble `Sensors`/`Actuators` pour appeler `logic::control_loop::run()`.
> Toute la logique de contrôle est écrite et testée (voir
> [État du projet](#état-du-projet--limites-connues)), mais le bring-up
> matériel reste à faire.

En attendant, la boucle de contrôle se vérifie desktop (sans matériel) :

```bash
cargo test-host-linux   # cargo test --target x86_64-unknown-linux-gnu --lib
```

---

## Architecture du projet

```
src/
├── lib.rs                     — Racine de la lib, réexporte les modules publics
├── config.rs                  — Broches GPIO, adresses I²C, seuils, timings de phase
├── comm/                      — Liaison série USB (feature "usb-comm", cf. limites connues)
│
├── cloud_chamber_hal/         — Traits abstraits (interfaces matériel), génériques
│   ├── sensors.rs             — Sensor, DeferredSensor, BatchSensor, DeferredBatchSensor, Sensors<..>
│   ├── actuators.rs           — BinaryActuator, AnalogActuator<Unit>, Actuators<..>, ActuatorPlan
│   ├── timer.rs                — MonotonicTimer, WatchdogFeed, Instant, Duration
│   ├── measurement.rs         — Measurement<Unit> (valeur + horodatage)
│   ├── units.rs                — Celsius, HectoPascal, Volt...
│   └── config.rs               — Forme des tableaux Sensors/SensorSnapshot (NUMBER_OF_*, index par rôle)
│
├── drivers/                   — Implémentations concrètes des traits ci-dessus
│   ├── ds18b20.rs             — Température 1-Wire (accès registre direct, clones supportés)
│   ├── bme280.rs               — Température/humidité/pression I²C (non intégré à Sensors, cf. limites)
│   ├── abp2.rs                  — Pression Honeywell I²C
│   ├── adc.rs                  — Tension/courant via l'ADC embarqué
│   ├── breaker.rs               — Relais GPIO (implémente BinaryActuator)
│   ├── closure.rs               — Détection fermeture de chambre (contact sec)
│   ├── encoder.rs               — Encodeur rotatif quadrature + bouton
│   └── mock.rs                  — Mocks (capteurs, actionneur, horloge) — `#[cfg(test)]` uniquement
│
├── logic/                     — Machine à états et boucle de contrôle (le cœur du firmware)
│   ├── control_loop.rs        — Point d'entrée `run()` + `tick()` (un tour de boucle, testable)
│   ├── cooling.rs              — Séquence de démarrage (6 phases), pur
│   ├── stopping.rs             — Séquence d'arrêt (3 phases), pur
│   ├── phase_clock.rs           — Durées/timeouts par phase, `PhaseClock<Clk>`, `advance()`
│   ├── security.rs              — Seuils de sécurité, `SafetyMonitor` (anti-rebond, verrouillage)
│   └── probing.rs               — Plan de sondage, `MeasurementHistory` (ring buffers horodatés)
│
├── shared/                    — État partagé entre le contrôle et ses lecteurs (UI...)
│   ├── data.rs                 — SharedState, SHARED_STATE (static), SensorSnapshot, SystemTask
│   └── ring_buffer.rs           — Buffer circulaire générique horodaté
│
└── ui/                         — Interface graphique (écran ILI9341 320×240, encodeur rotatif)
    ├── navigator.rs             — Pile de navigation entre écrans (Screen, const generic DEPTH)
    ├── interactions.rs           — Traits Rotary / Click
    ├── theme.rs                  — Palette de couleurs et styles
    └── screens/                  — Écrans concrets (menu principal, statistiques, graphe température)
```

### Principe d'inversion de dépendance

```
logic/  ──depends on──>  cloud_chamber_hal (traits, génériques)
                                 ↑
                          drivers/ (implémentations concrètes)
```

`logic/` ne connaît que des traits (`Sensor`, `BinaryActuator`,
`MonotonicTimer`...), jamais un driver concret — `control_loop::run()` est
générique sur `Sensors<Ts,Ps,Vs>`, `Actuators<Hv,Comp,Iso>` et `Clk`. On
peut donc tester toute la logique avec des mocks (`drivers::mock`) sans
aucun accès matériel, et substituer un driver réel sans toucher à `logic/`.

`cloud_chamber_hal` ne dépend jamais de `logic/` : quand un type a besoin de
vivre dans le HAL pour des raisons d'ergonomie d'API (ex. `ActuatorPlan`,
pour que `Actuators::apply()` le prenne directement), il est déplacé *dans*
le HAL plutôt qu'importé depuis `logic/` — la dépendance reste à sens
unique, jamais l'inverse.

---

## La boucle de contrôle

`SystemTask` (`shared/data.rs`) est la machine à états de haut niveau :

```rust
pub enum SystemTask {
    Idle,
    Cooling(CoolingPhase),   // SensorCheck → PreCoolingThePlate → StartingIpaCirculation
                              //   → SaturatingAirWithIpa → HighVoltage → FinalCheckBeforeStabilising
    Stabilising,             // régime permanent, pas de sortie automatique
    Stopping(StoppingPhase), // CutHighVoltage → CutCompressor → WaitPressureEquilibrium
    Tripped(SafetyCause),    // coupure de sécurité, verrouillé jusqu'à réarmement opérateur
}
```

### Décision / application, séparées partout

Chaque phase de `cooling.rs`/`stopping.rs` a une seule responsabilité :
décider la transition **et** construire son propre `ActuatorPlan` (quels
actionneurs allumer), sans jamais toucher au matériel. `Actuators::apply()`
(dans le HAL) ne fait qu'exécuter ce plan — aucune logique dedans. Ce même
principe décision/application se retrouve à chaque étage :

- `react_to(task, history) -> (SystemTask, ActuatorPlan)` : décision pure,
  basée uniquement sur les mesures.
- `advance(task, history, elapsed_ms, chamber_stale_ms) -> (SystemTask, ActuatorPlan)`
  (`phase_clock.rs`) : ajoute la priorité mesure > abandon perte-capteur >
  délai/timeout — toujours pure, ne possède rien.
- `PhaseClock<Clk>` : possède l'horloge de l'appareil (`Clk: MonotonicTimer`)
  et sait uniquement « quelle phase, depuis quand » — pas de logique de
  décision.
- `control_loop::tick()` : orchestre explicitement l'enchaînement (sécurité
  en priorité absolue, sinon `advance()`, puis `Actuators::apply()`, puis
  publication) — l'orchestration reste visible dans ce fichier, jamais
  cachée dans une méthode d'un autre type.

### Réconciliation avec `SHARED_STATE`

L'UI peut écrire directement dans `SHARED_STATE.task` pour demander un
démarrage, un arrêt, ou acquitter un `Tripped` — pas de canal séparé. Deux
règles, dans les deux sens, protègent contre les races :

1. **En début de tour**, `tick()` relit `SHARED_STATE.task` et l'adopte
   (`PhaseClock::set`, no-op si la valeur n'a pas changé) — donc une
   écriture externe survenue depuis le tour précédent est prise en compte
   avant de décider quoi que ce soit.
2. **En fin de tour**, la publication de la nouvelle tâche est un
   *compare-and-swap* atomique (une seule section critique) : elle
   n'écrit `next` que si `SHARED_STATE` vaut encore exactement ce qui a
   été lu en début de tour. Si l'UI a écrit entre-temps, `next` (calculé
   sur une base désormais périmée) est abandonné, et le tour suivant
   repart de la vraie valeur de `SHARED_STATE`.

Conséquence : **toute transition décidée par le contrôleur lui-même**
(avancement normal, abandon perte-capteur, timeout, fin de cycle) est
toujours prioritaire sur une valeur de `SHARED_STATE` restée en retard —
aucun cas particulier à coder phase par phase, la règle générale suffit.

---

## `cloud_chamber_hal` — traits et inversion de dépendance

| Trait | Rôle |
|-------|------|
| `Sensor<T>` / `DeferredSensor<T>` | Lecture simple / lecture différée (conversion à attendre, ex. DS18B20) |
| `BatchSensor<Unit, N>` / `DeferredBatchSensor<Unit, N>` | Lecture groupée de `N` capteurs de la même catégorie |
| `IndependentSensors<S, N>` | Enrobe `N` capteurs indépendants derrière l'interface `BatchSensor` |
| `BinaryActuator` | Sortie tout-ou-rien (`turn_on`/`turn_off`) — relais HV/compresseur/iso |
| `AnalogActuator<Unit>` | Sortie continue dans une unité physique (réutilisable si l'iso passe en PWM) |
| `MonotonicTimer` | Horloge monotone (`now() -> Instant`, `elapsed_since()` par défaut) |

`Instant`/`Duration` (`cloud_chamber_hal::timer`) sont en microsecondes sur
`u64` — pas de débordement avant des centaines de milliers d'années, utile
puisque `Stabilising` (régime permanent) peut durer des jours en usage réel.

`Sensors<Tmp, Prs, Vlt>` et `Actuators<Hv, Comp, Iso>` regroupent les trois
sources/sorties de la chambre ; tous deux génériques, tous deux traités de
façon symétrique (pas de logique dans `Actuators`, juste l'exécution du
plan).

---

## Drivers disponibles

### DS18B20 — Température 1-Wire

```rust
use cloud_chamber_firmware::drivers::ds18b20::{Ds18b20Sensors, Resolution};
```

Protocole 1-Wire implémenté directement (sans crate externe), accès
registre SIO pour tenir la fenêtre de lecture (~10µs) sur les clones
DS18B20 testés sur ce montage — une version générique `embedded-hal`
`OutputPin` ne suffisait pas (pousse activement un niveau haut au lieu de
relâcher la ligne en open-drain). Découverte SEARCH ROM au boot.

| Résolution | Précision  | Durée de conversion max |
|------------|------------|--------------------------|
| `Bits9`    | ± 0,5 °C   | 150 ms |
| `Bits10`   | ± 0,25 °C  | 240 ms |
| `Bits11`   | ± 0,125 °C | 430 ms |
| `Bits12`   | ± 0,0625 °C| 800 ms |

### ABP2 — Pression Honeywell (I²C)

```rust
use cloud_chamber_firmware::drivers::abp2::Abp2Sensor;
use cloud_chamber_firmware::config::{ABP2_BP_ADDR, BP_PRESSURE_MIN, BP_PRESSURE_MAX};
```

Conversion selon l'Application Note Honeywell AN-1728. Deux capteurs
distincts (basse/haute pression), pas de partage de bus particulier au-delà
de l'I²C standard.

### ADC — Tension / courant

```rust
use cloud_chamber_firmware::drivers::adc::{AdcVoltageSensor, AdcCurrentSensor};
```

Couche `AdcChannel` séparée de la conversion (tension/courant), pour
pouvoir tester la logique de conversion avec `MockChannel` sans ADC réel.

### Relais (`BinaryActuator`)

```rust
use cloud_chamber_firmware::drivers::breaker::GpioBreaker;
```

Un seul driver générique pour les trois relais (HV, compresseur, iso) —
`active_high` gère les deux sens de câblage sans dupliquer la logique.

### BME280 — Non intégré

Le driver existe (`drivers/bme280.rs`, température/humidité/pression I²C)
mais n'est pas câblé dans `Sensors`/`SensorSnapshot` : cette structure ne
modélise aujourd'hui aucune catégorie de mesure ambiante. À intégrer si
besoin d'afficher/utiliser une mesure BME280 (ambiance, sursaturation IPA).

---

## Interface utilisateur

Écran ILI9341 320×240, navigation par encodeur rotatif quadrature
(`drivers::encoder`) — pas de tactile.

- `ui::navigator::Navigator<DEPTH>` : pile de navigation générique (`Screen`
  comme simple étiquette), testée en isolation.
- `ui::interactions::{Rotary, Click}` : traits que chaque écran implémente
  pour réagir à la rotation/au clic.
- `ui::screens::menu` : menu principal.
- `ui::screens::stats` : écran de statistiques en direct (phase courante,
  cause de trip, températures, pressions, sorties actionneurs attendues).
- `ui::screens::temp` : graphe de température (ring buffer horodaté).

---

## Tests

```bash
cargo test-host-linux   # cargo test --target x86_64-unknown-linux-gnu --lib
```

`#![cfg_attr(not(test), no_std)]` : en mode test, la lib compile avec `std`
disponible (nécessaire pour `critical_section` sur desktop et pour les
tests interactifs SDL2). Aucune cible embarquée n'est requise pour lancer
la suite de tests.

### Ce qui est testé, et où

- **Logique pure** (`cooling.rs`, `stopping.rs`, `phase_clock.rs`,
  `security.rs`, `probing.rs`) : chaque condition de transition, chaque
  timeout, `SafetyMonitor` (anti-rebond, verrouillage, réarmement) —
  testés isolément, sans mock, ce sont des fonctions/structures pures.
- **`logic::control_loop`** : `run()` boucle indéfiniment (`-> !`), donc
  non testable directement — son corps de boucle est extrait dans
  `tick()`, testable. Une suite d'intégration (`drivers::mock` +
  `Harness` de test) enchaîne `tick()` sur plusieurs tours comme en usage
  réel : cycle complet Idle → Stabilising → Idle, abandons par
  timeout/perte-capteur, priorité sécurité, réconciliation `SHARED_STATE`
  dans les deux sens (y compris un test dédié à la race condition entre
  publication et écriture externe), robustesse aux échecs de sondage.
- **UI** : captures d'écran headless (`embedded-graphics-simulator`) pour
  chaque écran.

### Test interactif SDL2 (optionnel)

```bash
cargo test-live-ui-linux   # nécessite SDL2 installé sur la machine
```

Ouvre une vraie fenêtre et permet de naviguer le menu au clavier
(flèches, Entrée/Espace) — utile pour visualiser un écran sans repasser
par une capture PNG à chaque fois. Feature `live-menu-test`, désactivée
par défaut (dépendances SDL2 non nécessaires au reste du projet).

---

## État du projet / limites connues

La logique de contrôle (démarrage, sécurité, arrêt, réconciliation UI) est
considérée fonctionnellement terminée et testée. Ce qui reste, avant un
firmware réellement flashable :

- **Aucun bring-up matériel** : pas de `src/main.rs`, aucun `[[bin]]`
  enregistré. Rien ne construit encore les `Pins`/`I2c`/ADC réels ni
  n'assemble `Sensors::new(...)` + `Actuators { ... }` pour appeler
  `control_loop::run(...)`.
- **Signal UI → contrôle pas câblé** : `SHARED_STATE.task` peut recevoir
  une écriture externe (le mécanisme de réconciliation est prêt et testé),
  mais aucun écran n'écrit encore dedans — pas d'écran "Contrôle"
  (démarrage/arrêt) construit sur cette branche.
- **`SafetyMonitor::reset()` jamais appelé** : une fois `Tripped`, le
  système y reste verrouillé indéfiniment — le réarmement opérateur
  (bouton, écran dédié) reste à concevoir et câbler. Comportement actuel
  volontairement testé et documenté (`control_loop::tests::safety_trip_*`),
  pas un oubli silencieux.
- **`comm/` (liaison USB) désactivé et cassé** : la feature `usb-comm`
  n'est pas déclarée dans `Cargo.toml` (le module ne compile donc jamais
  actuellement), et son code référence des fonctions de `phase_clock.rs`
  qui ont été retirées pendant la refonte de `PhaseClock`. Portée
  initiale (commande `CYCLE` uniquement) toujours valable si repris,
  mais nécessite une remise à niveau avant de rendre la feature à nouveau
  compilable.
- **`ui/screens/status.rs`** : fichier présent mais non déclaré dans
  `screens/mod.rs` (orphelin) — écran remplacé par `ui/screens/stats.rs`.
- **`examples/ui_simulator.rs`** : obsolète, référence des types qui
  n'existent plus (`SystemState`, `StatusScreen`...) — pas mis à jour.
- **3 tests connus en échec**, sans rapport avec le contrôle :
  `ui::screens::menu::tests::{select_next_at_top_stays,
  select_previous_at_bottom_stays, select_previous_increments}` —
  incohérence entre les assertions et l'implémentation de la navigation,
  pas encore corrigée.
- **Ampèremètre non intégré** : pas de protection surintensité logicielle.
- **`SAFETY_HP_MAX` (14.0 bar)** au-dessus de la plage physique du capteur
  ABP2 HP (0–12 bar) — ce seuil ne peut donc jamais se déclencher tel
  quel. Valeur volontairement pas corrigée : il faut la vraie limite
  mécanique du circuit, pas un chiffre deviné sur un seuil de sécurité.
- **Broches `PIN_HV_RELAY`/`PIN_ISO_HEATER_RELAY`** provisoires (GP17/18),
  jamais vérifiées sur le montage réel.

L'architecture elle-même (séparation HAL/logic, décision/application,
`PhaseClock` propriétaire de son horloge, réconciliation `SHARED_STATE`)
est traitée comme stable — les points ci-dessus sont des travaux
d'intégration/bring-up restants, pas des refontes prévues.
