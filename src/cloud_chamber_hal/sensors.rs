//! Traits pour les capteurs de mesure.
//!
//! # Pourquoi un seul trait générique `Sensor<T>` ?
//!
//! Toutes les mesures partagent la même forme : une valeur horodatée
//! (`Measurement<Unit>`). Un seul trait générique évite de dupliquer un
//! trait par type physique (température, pression, tension...) — c'est
//! le paramètre `Unit` de `Measurement` qui distingue les grandeurs.
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
//!
//! `DeferredBatchSensor` ajoute la lecture en deux phases pour les bus
//! partagés : `start_conversion()` déclenche la mesure sans bloquer,
//! `read_result()` vient chercher les résultats.

use core::fmt::Debug;

use crate::cloud_chamber_hal::{
    config::{NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR},
    measurement::Measurement,
    timer::Duration,
    units::{Celsius, HectoPascal},
};

/// Capteur retournant une mesure unique de type `T` (ex: `Measurement<Celsius>`).
pub trait Sensor<T> {
    type Error: Debug;
    fn read(&mut self) -> Result<T, Self::Error>;
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

/// Regroupe les deux sources de mesure (température, pression).
///
/// Chaque source produit un batch complet en un seul appel — que ce soit un
/// bus partagé natif (ex: `Ds18b20Sensors`) ou `N` capteurs indépendants
/// enveloppés dans `IndependentSensors`.
pub struct Sensors<Tmp, Prs>
where
    Tmp: DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Prs: BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
{
    pub temperature_source: Tmp,
    pub pressure_source: Prs,
}

impl<Tmp, Prs> Sensors<Tmp, Prs>
where
    Tmp: DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Prs: BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
{
    /// Construit `Sensors` à partir de sources déjà initialisées.
    ///
    /// La construction matérielle (bus I2C, broches GPIO...) est de la
    /// responsabilité du code d'initialisation propre à la carte, pas de
    /// cette fonction : `Sensors` ne fait qu'agréger des sources prêtes.
    pub fn new(temperature_source: Tmp, pressure_source: Prs) -> Self {
        Self { temperature_source, pressure_source }
    }
}
