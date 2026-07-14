//! Traits pour les capteurs de mesure.
//!
//! # Pourquoi un seul trait générique `Sensor<T>` ?
//!
//! Toutes les mesures partagent la même forme : une valeur horodatée
//! (`Measurement<Unit>`). Un seul trait générique évite de dupliquer un
//! trait par type physique (température, pression, tension...) — c'est
//! le paramètre `Unit` de `Measurement` qui distingue les grandeurs.
//!
//! # Lecture en une phase vs deux phases
//!
//! `Sensor<T>` convient aux capteurs dont la lecture est immédiate ou
//! auto-suffisante (ADC, GPIO, capteurs à conversion continue en arrière-plan).
//! `DeferredSensor<T>` s'ajoute pour les capteurs dont la conversion prend un
//! temps notable (DS18B20, BME280) : `start_conversion()` déclenche la mesure
//! sans bloquer, ce qui permet de démarrer plusieurs conversions en parallèle
//! avant de venir chercher les résultats avec `read_result()`.
//!
//! # Capteurs groupés (`BatchSensor`)
//!
//! Certains capteurs partagent un bus physique unique (ex: plusieurs DS18B20
//! sur un même fil 1-Wire) : on ne peut pas les représenter comme `N`
//! instances indépendantes de `Sensor<T>`, puisque la conversion peut être
//! déclenchée simultanément pour tous les appareils du bus. `BatchSensor`
//! représente ce cas : une lecture retourne `N` résultats indépendants
//! (`Result` par case, pour qu'un capteur en défaut n'empêche pas la lecture
//! des autres). `IndependentSensors<S, N>` fournit l'implémentation inverse :
//! elle regroupe `N` capteurs réellement indépendants (ex: puces I2C
//! séparées) derrière la même interface `BatchSensor`, ce qui permet à
//! `Sensors` de traiter les deux cas de façon uniforme.

use core::fmt::Debug;

use crate::{
    cloud_chamber_hal::{timer::{Instant, Duration}, units::{Celsius, HectoPascal, Volt}},
    config::{NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR, NUMBER_OF_VOLTMETER}
};

/// Mesure horodatée dans l'unité physique `Unit`.
#[derive(Clone, Copy, Debug)]
pub struct Measurement<Unit> {
    pub time: Instant,
    pub value: Unit,
}

impl<Unit> Measurement<Unit> {
    pub fn new(time: Instant, value: Unit) -> Self {
        Self { time, value }
    }

    /// `true` si cette mesure est plus récente que `other`.
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self.time.is_newer_than(&other.time)
    }

    /// `true` si cette mesure est plus ancienne que `other`.
    pub fn is_older_than(&self, other: &Self) -> bool {
        other.time.is_newer_than(&self.time)
    }
}

/// Capteur retournant une mesure unique de type `T` (ex: `Measurement<Celsius>`).
pub trait Sensor<T> {
    type Error: Debug;
    fn read(&mut self) -> Result<T, Self::Error>;
}

/// Capteur dont la conversion prend un temps notable : lecture en deux phases
/// pour permettre le chevauchement de plusieurs conversions simultanées.
///
/// `read()` (hérité de `Sensor<T>`) reste utilisable seul : il déclenche la
/// conversion, attend `conversion_time_ms()`, puis lit le résultat.
pub trait DeferredSensor<T>: Sensor<T> {
    /// Déclenche la conversion sans attendre qu'elle soit terminée.
    fn start_conversion(&mut self) -> Result<(), Self::Error>;
    /// Durée à attendre après `start_conversion()` avant que `read_result()` soit valide.
    fn conversion_time_ms(&self) -> Duration;
    /// Récupère le résultat d'une conversion déjà déclenchée.
    fn read_result(&mut self) -> Result<T, Self::Error>;
}

/// Source produisant `N` mesures horodatées de type `Unit` en un seul appel.
///
/// Un échec individuel (capteur en défaut) n'interrompt pas la lecture des
/// autres : il est reporté dans la case correspondante via `Result::Err`,
/// plutôt que d'être silencieusement transformé en `None`.
pub trait BatchSensor<Unit, const N: usize> {
    type Error: Debug;
    fn read(&mut self) -> [Result<Measurement<Unit>, Self::Error>; N];
}

/// Variante deux-phases de `BatchSensor`, pour les bus partagés (ex: 1-Wire)
/// où toutes les conversions peuvent être déclenchées simultanément.
pub trait DeferredBatchSensor<Unit, const N: usize>: BatchSensor<Unit, N> {
    fn start_conversion(&mut self) -> Result<(), Self::Error>;
    fn conversion_time_ms(&self) -> Duration;
    fn read_result(&mut self) -> [Result<Measurement<Unit>, Self::Error>; N];
}

/// Regroupe `N` capteurs réellement indépendants (ex: puces I2C séparées)
/// derrière l'interface `BatchSensor`.
pub struct IndependentSensors<S, const N: usize>(pub [S; N]);

impl<S, Unit, const N: usize> BatchSensor<Unit, N> for IndependentSensors<S, N>
where
    S: Sensor<Measurement<Unit>>,
{
    type Error = S::Error;

    fn read(&mut self) -> [Result<Measurement<Unit>, Self::Error>; N] {
        core::array::from_fn(|i| self.0[i].read())
    }
}

impl<S, Unit, const N: usize> DeferredBatchSensor<Unit, N> for IndependentSensors<S, N>
where
    S: DeferredSensor<Measurement<Unit>>,
{
    fn start_conversion(&mut self) -> Result<(), Self::Error> {
        let mut result = Ok(());
        for sensor in self.0.iter_mut() {
            if let Err(e) = sensor.start_conversion() {
                result = Err(e);
            }
        }
        result
    }

    fn conversion_time_ms(&self) -> Duration {
        self.0.iter().map(S::conversion_time_ms).max().unwrap()
    }

    fn read_result(&mut self) -> [Result<Measurement<Unit>, Self::Error>; N] {
        core::array::from_fn(|i| self.0[i].read_result())
    }
}

/// Regroupe les trois sources de mesure (température, pression, tension).
///
/// Chaque source produit un batch complet en un seul appel — que ce soit un
/// bus partagé natif (ex: `Ds18b20Sensors`) ou `N` capteurs indépendants
/// enveloppés dans `IndependentSensors`.
pub struct Sensors<Tmp, Prs, Vlt>
where
    Tmp: BatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Prs: BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
    Vlt: BatchSensor<Volt, NUMBER_OF_VOLTMETER>,
{
    pub temperature_source: Tmp,
    pub pressure_source: Prs,
    pub voltage_source: Vlt,
}

impl<Tmp, Prs, Vlt> Sensors<Tmp, Prs, Vlt>
where
    Tmp: BatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Prs: BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
    Vlt: BatchSensor<Volt, NUMBER_OF_VOLTMETER>,
{
    /// Construit `Sensors` à partir de sources déjà initialisées.
    ///
    /// La construction matérielle (bus I2C, broches GPIO...) est de la
    /// responsabilité du code d'initialisation propre à la carte, pas de
    /// cette fonction : `Sensors` ne fait qu'agréger des sources prêtes.
    pub fn new(temperature_source: Tmp, pressure_source: Prs, voltage_source: Vlt) -> Self {
        Self { temperature_source, pressure_source, voltage_source }
    }
}
