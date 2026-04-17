#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

mod config;
mod data;
mod sensors;
mod network;

use core::default::Default;

use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Flex, Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::I2C0;
use embassy_time::{Duration, Instant, Timer};
use defmt_rtt as _;
use panic_probe as _;

use crate::config::*;
use crate::data::*;
use crate::sensors::abp2::{self, Abp2Config};
use crate::sensors::ds18b20::Ds18b20Bus;

// Déclare le gestionnaire d'interruption I2C0
bind_interrupts!(struct Irqs {
    I2C0_IRQ => InterruptHandler<I2C0>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    defmt::info!("Cloud Chamber Cooling System starting...");

    let p = embassy_rp::init(Default::default());

    // GPIO relais compresseur
    let mut compressor_relay = Output::new(p.PIN_16, Level::High);

    // Bus 1-Wire DS18B20
    let onewire_pin = Flex::new(p.PIN_15);
    let mut temp_bus = Ds18b20Bus::new(onewire_pin);
    let sensor_count = temp_bus.discover_sensors();
    defmt::info!("Temperature bus: {} sensor(s) found", sensor_count);

    // I2C avec interrupt handler correct
    let i2c_config = i2c::Config::default();
    let mut i2c_bus = I2c::new_async(p.I2C0, p.PIN_5, p.PIN_4, Irqs, i2c_config);
    defmt::info!("I2C bus initialized on GP4/GP5");

    let bp_config = Abp2Config {
        address: ABP2_BP_ADDR,
        p_min: BP_PRESSURE_MIN,
        p_max: BP_PRESSURE_MAX,
        label: "BP",
    };
    let hp_config = Abp2Config {
        address: ABP2_HP_ADDR,
        p_min: HP_PRESSURE_MIN,
        p_max: HP_PRESSURE_MAX,
        label: "HP",
    };

    defmt::info!("Flow controller entering main loop");
    let boot_instant = Instant::now();
    let mut cycle_count: u64 = 0;

    loop {
        cycle_count += 1;
        let uptime_s = boot_instant.elapsed().as_secs();

        // Phase 1 : lecture capteurs critiques
        let critical_temps = temp_bus.read_critical().await;
        let pressure_hp = abp2::read_abp2(&mut i2c_bus, &hp_config)
            .await.unwrap_or_default();
        let pressure_bp = abp2::read_abp2(&mut i2c_bus, &bp_config)
            .await.unwrap_or_default();

        // Phase 2 : réaction sécurité
        let mut compressor_ok = true;

        if pressure_hp.valid && pressure_hp.pressure > SAFETY_HP_MAX {
            defmt::error!("ALARM: HP pressure too high!");
            compressor_ok = false;
            let mut state = SHARED_STATE.lock().await;
            state.push_alarm(AlarmLevel::Critical, "pressure_hp", "Surpression HP", uptime_s);
        }
        if pressure_bp.valid && pressure_bp.pressure < SAFETY_BP_MIN {
            defmt::warn!("ALARM: BP pressure too low!");
            let mut state = SHARED_STATE.lock().await;
            state.push_alarm(AlarmLevel::Warning, "pressure_bp", "Vide trop profond", uptime_s);
        }
        if critical_temps[0].valid && critical_temps[0].value > SAFETY_TEMP_COMPRESSOR_MAX {
            defmt::error!("ALARM: Compressor overheating!");
            compressor_ok = false;
            let mut state = SHARED_STATE.lock().await;
            state.push_alarm(AlarmLevel::Critical, "temp_compressor", "Surchauffe compresseur", uptime_s);
        }

        if compressor_ok {
            compressor_relay.set_high();
        } else {
            compressor_relay.set_low();
            defmt::error!("COMPRESSOR CUT OFF");
        }

        // Phase 3 : capteurs non-critiques
        let non_critical_temps = temp_bus.read_non_critical().await;

        // Phase 4 : mise à jour état partagé
        {
            let mut state = SHARED_STATE.lock().await;
            for i in 0..5 {
                if critical_temps[i].valid || critical_temps[i].critical {
                    state.temperatures[i] = critical_temps[i];
                }
                if non_critical_temps[i].valid {
                    state.temperatures[i] = non_critical_temps[i];
                }
            }
            state.pressure_bp = pressure_bp;
            state.pressure_hp = pressure_hp;
            state.compressor_allowed = compressor_ok;
            state.cycle_count = cycle_count;
            state.uptime_s = uptime_s;
        }

        if cycle_count % 10 == 0 {
            defmt::info!("Cycle {} | uptime {}s | comp:{}", cycle_count, uptime_s, compressor_ok);
        }

        Timer::after(Duration::from_millis(CRITICAL_READ_INTERVAL_MS)).await;
    }
}