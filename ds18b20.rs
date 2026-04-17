/// Driver DS18B20 pour bus 1-Wire sur RP2040.
///
/// Gère la découverte automatique des capteurs et la lecture
/// sélective (critiques vs non-critiques) pour le flow controller.
///
/// # Utilisation
///
/// ```rust
/// let mut bus = Ds18b20Bus::new(pin);
/// bus.discover_sensors();
/// let readings = bus.read_all().await;
/// let critical_only = bus.read_critical().await;
/// ```

use embassy_rp::gpio::{Flex, Pull};
use embassy_time::{Duration, Timer};
use defmt;

use one_wire_bus::OneWire;
use ds18b20::{Ds18b20, Resolution};

use crate::data::TemperatureReading;
use crate::config::{CRITICAL_TEMP_INDICES, NON_CRITICAL_TEMP_INDICES};

/// Nombre maximum de capteurs supportés sur le bus
const MAX_SENSORS: usize = 5;

/// Gère un bus 1-Wire avec plusieurs DS18B20.
pub struct Ds18b20Bus<'a> {
    /// Bus 1-Wire (utilise un GPIO flexible pour le open-drain)
    one_wire: OneWire<Flex<'a>>,
    /// Adresses ROM des capteurs découverts
    addresses: [Option<u64>; MAX_SENSORS],
    /// Nombre de capteurs détectés
    count: usize,
}

impl<'a> Ds18b20Bus<'a> {
    /// Crée un nouveau bus 1-Wire sur le GPIO donné.
    pub fn new(pin: Flex<'a>) -> Self {
        let one_wire = OneWire::new(pin).expect("Failed to init 1-Wire bus");
        Self {
            one_wire,
            addresses: [None; MAX_SENSORS],
            count: 0,
        }
    }

    /// Découvre tous les DS18B20 sur le bus.
    /// Doit être appelé une fois au démarrage.
    /// Retourne le nombre de capteurs trouvés.
    pub fn discover_sensors(&mut self) -> usize {
        self.count = 0;
        let mut search_state = None;

        loop {
            match self.one_wire.device_search(search_state.as_ref(), false, &mut embassy_time::Delay) {
                Ok(Some((address, state))) => {
                    // Vérifier que c'est bien un DS18B20 (family code 0x28)
                    if address.family_code() == ds18b20::FAMILY_CODE {
                        if self.count < MAX_SENSORS {
                            self.addresses[self.count] = Some(address.0);
                            defmt::info!(
                                "DS18B20 #{} found: ROM = {:#018X}",
                                self.count,
                                address.0
                            );
                            self.count += 1;
                        }
                    }
                    search_state = Some(state);
                }
                Ok(None) => break, // Plus de capteurs
                Err(_) => {
                    defmt::error!("1-Wire search error");
                    break;
                }
            }
        }

        defmt::info!("Discovered {} DS18B20 sensor(s)", self.count);
        self.count
    }

    /// Lit la température d'un capteur spécifique par son index.
    /// Retourne `None` en cas d'erreur de communication.
    pub async fn read_sensor(&mut self, index: usize) -> Option<f32> {
        if index >= self.count {
            return None;
        }

        let address = match self.addresses[index] {
            Some(addr) => one_wire_bus::Address(addr),
            None => return None,
        };

        let sensor = Ds18b20::new(address).ok()?;

        // Lancer la conversion (12-bit = 750ms max)
        sensor
            .start_temp_measurement(&mut self.one_wire, &mut embassy_time::Delay)
            .ok()?;

        // Attendre la fin de conversion
        // En 12-bit, max 750ms. On attend 800ms par sécurité.
        Timer::after(Duration::from_millis(800)).await;

        // Lire le résultat
        let data = sensor
            .read_data(&mut self.one_wire, &mut embassy_time::Delay)
            .ok()?;

        Some(data.temperature)
    }

    /// Lit uniquement les capteurs critiques (définis dans config.rs).
    /// Retourne un tableau de TemperatureReading.
    pub async fn read_critical(&mut self) -> [TemperatureReading; MAX_SENSORS] {
        let mut readings = [TemperatureReading::default(); MAX_SENSORS];

        for &idx in CRITICAL_TEMP_INDICES.iter() {
            if let Some(temp) = self.read_sensor(idx).await {
                readings[idx] = TemperatureReading {
                    value: temp,
                    valid: true,
                    critical: true,
                };
            } else {
                readings[idx] = TemperatureReading {
                    value: f32::NAN,
                    valid: false,
                    critical: true,
                };
            }
        }

        readings
    }

    /// Lit uniquement les capteurs non-critiques.
    pub async fn read_non_critical(&mut self) -> [TemperatureReading; MAX_SENSORS] {
        let mut readings = [TemperatureReading::default(); MAX_SENSORS];

        for &idx in NON_CRITICAL_TEMP_INDICES.iter() {
            if let Some(temp) = self.read_sensor(idx).await {
                readings[idx] = TemperatureReading {
                    value: temp,
                    valid: true,
                    critical: false,
                };
            } else {
                readings[idx] = TemperatureReading {
                    value: f32::NAN,
                    valid: false,
                    critical: false,
                };
            }
        }

        readings
    }

    /// Lit tous les capteurs (critiques + non-critiques).
    pub async fn read_all(&mut self) -> [TemperatureReading; MAX_SENSORS] {
        let mut readings = [TemperatureReading::default(); MAX_SENSORS];

        for idx in 0..self.count {
            let is_critical = CRITICAL_TEMP_INDICES.contains(&idx);
            if let Some(temp) = self.read_sensor(idx).await {
                readings[idx] = TemperatureReading {
                    value: temp,
                    valid: true,
                    critical: is_critical,
                };
            } else {
                readings[idx] = TemperatureReading {
                    value: f32::NAN,
                    valid: false,
                    critical: is_critical,
                };
            }
        }

        readings
    }

    /// Nombre de capteurs découverts.
    pub fn sensor_count(&self) -> usize {
        self.count
    }
}
