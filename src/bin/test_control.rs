//! Tests unitaires de la logique de contrôle — aucun capteur requis.
//!
//! Flasher : cargo run --bin test_control
//! Lire    : PuTTY / minicom / screen à 115200 baud sur le port USB du Pico.

#![no_std]
#![no_main]

use core::fmt::Write as FmtWrite;

use rp2040_hal as hal;
use hal::pac;

use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::SerialPort;
use heapless::String;

use cloud_chamber_firmware::{
    config::{
        SAFETY_HP_MAX, SAFETY_TEMP_COMPRESSOR_MAX, SAFETY_BP_MIN, TARGET_CHAMBER_TEMP,
        CRITICAL_READ_INTERVAL_MS, SENSOR_FAILURE_RETRY_MS,
    },
    control::{
        controller::Controller,
        pid::PidController,
        scheduler::TempScheduler,
        target::TargetState,
    },
    data::{PressureReading, SystemState, TemperatureReading},
};

use defmt_rtt as _;
use panic_halt as _;

// ── Boot2 ────────────────────────────────────────────────────────────────────

#[link_section = ".boot2"]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

// ── Constructeurs de test ─────────────────────────────────────────────────────

fn temp(value: f32, valid: bool) -> TemperatureReading {
    TemperatureReading { value, valid, critical: false }
}

fn pressure(p: f32, valid: bool) -> PressureReading {
    PressureReading { pressure: p, temperature: 0.0, valid }
}

/// État nominal : tout valide, rien en alarme, chambre à 20 °C.
fn nominal() -> SystemState {
    let mut s = SystemState::new();
    s.pressure_hp = pressure(8.0, true);  // sous SAFETY_HP_MAX = 14 bar
    s.pressure_bp = pressure(0.5, true);  // au-dessus SAFETY_BP_MIN = 0.15 bar
    for i in 0..5 { s.temperatures[i] = temp(20.0, true); }
    s.temperatures[0] = temp(60.0, true); // sortie_compresseur : pas en surchauffe
    s
}

// ── Tests — sécurité ─────────────────────────────────────────────────────────

fn t_safety_hp() -> bool {
    let mut s = nominal();
    s.pressure_hp = pressure(SAFETY_HP_MAX + 1.0, true);
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    out.safety_override && !out.compressor && !out.high_voltage
        && out.isopropanol_heater_duty == 0.0
}

fn t_safety_comp_temp() -> bool {
    let mut s = nominal();
    s.temperatures[0] = temp(SAFETY_TEMP_COMPRESSOR_MAX + 1.0, true);
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    out.safety_override && !out.compressor
}

fn t_safety_bp() -> bool {
    let mut s = nominal();
    s.pressure_bp = pressure(SAFETY_BP_MIN - 0.05, true);
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    out.safety_override && !out.compressor
}

fn t_safety_invalid_ignored() -> bool {
    // Pression hors seuil mais lecture invalide → pas de safety_override
    let mut s = nominal();
    s.pressure_hp = pressure(SAFETY_HP_MAX + 5.0, false);
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    !out.safety_override
}

// ── Tests — compresseur ───────────────────────────────────────────────────────

fn t_compressor_on() -> bool {
    // Chambre bien au-dessus de la cible + hystérésis → compresseur ON
    let mut s = nominal();
    s.temperatures[4] = temp(TARGET_CHAMBER_TEMP + 10.0, true);
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    out.compressor && !out.safety_override
}

fn t_compressor_off() -> bool {
    let mut s = nominal();
    let mut ctrl = Controller::new();
    // D'abord allumer
    s.temperatures[4] = temp(TARGET_CHAMBER_TEMP + 10.0, true);
    ctrl.step(&s, &TargetState::default(), 0.1);
    // Puis chambre bien en dessous → OFF
    s.temperatures[4] = temp(TARGET_CHAMBER_TEMP - 10.0, true);
    let out = ctrl.step(&s, &TargetState::default(), 0.1);
    !out.compressor && !out.safety_override
}

fn t_compressor_hysteresis() -> bool {
    // Le compresseur allumé reste allumé quand on revient dans la bande
    let mut s = nominal();
    let mut ctrl = Controller::new();
    s.temperatures[4] = temp(TARGET_CHAMBER_TEMP + 10.0, true); // ON
    ctrl.step(&s, &TargetState::default(), 0.1);
    s.temperatures[4] = temp(TARGET_CHAMBER_TEMP, true); // dans la bande → reste ON
    let out = ctrl.step(&s, &TargetState::default(), 0.1);
    out.compressor
}

fn t_compressor_allowed_interlock() -> bool {
    // L'interlock compressor_allowed = false doit couper la sortie sans safety_override
    let mut ctrl = Controller::new();
    let mut s = nominal();
    s.temperatures[4] = temp(TARGET_CHAMBER_TEMP + 10.0, true); // hystérésis voudrait ON
    s.compressor_allowed = false;
    let out = ctrl.step(&s, &TargetState::default(), 0.1);
    !out.compressor && !out.safety_override
}

fn t_compressor_invalid_keeps_off() -> bool {
    // Lecture invalide avec état initial OFF → reste OFF
    let mut s = nominal();
    s.temperatures[4] = temp(0.0, false);
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    !out.compressor
}

// ── Tests — haut voltage ──────────────────────────────────────────────────────

fn t_hv_on_when_cold() -> bool {
    let mut s = nominal();
    s.temperatures[4] = temp(TARGET_CHAMBER_TEMP, true); // exactement sur la cible
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    out.high_voltage && !out.safety_override
}

fn t_hv_off_when_warm() -> bool {
    let mut s = nominal();
    s.temperatures[4] = temp(TARGET_CHAMBER_TEMP + 20.0, true); // trop chaud
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    !out.high_voltage
}

fn t_hv_off_by_target() -> bool {
    let mut s = nominal();
    s.temperatures[4] = temp(TARGET_CHAMBER_TEMP, true);
    let mut target = TargetState::default();
    target.high_voltage_enabled = false;
    let out = Controller::new().step(&s, &target, 0.1);
    !out.high_voltage
}

fn t_hv_off_no_valid_reading() -> bool {
    let mut s = nominal();
    s.temperatures[4] = temp(0.0, false); // pas de lecture valide
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    !out.high_voltage
}

// ── Tests — chauffage isopropanol ─────────────────────────────────────────────

fn t_iso_positive_error() -> bool {
    // Temp isopropanol < cible → erreur positive → duty > 0
    let mut s = nominal();
    let target = TargetState::default(); // iso_target = -20 °C
    s.temperatures[3] = temp(-30.0, true); // -10 °C en dessous → duty = 1.0 (clampé)
    let out = Controller::new().step(&s, &target, 1.0);
    out.isopropanol_heater_duty > 0.0 && out.isopropanol_heater_duty <= 1.0
}

fn t_iso_negative_error_is_zero() -> bool {
    // Temp isopropanol > cible → erreur négative → duty = 0 (pas de refroidissement)
    let mut s = nominal();
    let target = TargetState::default(); // iso_target = -20 °C
    s.temperatures[3] = temp(-10.0, true); // au-dessus de la cible
    let out = Controller::new().step(&s, &target, 1.0);
    out.isopropanol_heater_duty == 0.0
}

fn t_iso_invalid_is_zero() -> bool {
    let mut s = nominal();
    s.temperatures[3] = temp(-30.0, false); // invalide
    let out = Controller::new().step(&s, &TargetState::default(), 0.1);
    out.isopropanol_heater_duty == 0.0
}

// ── Tests — planificateur de température ──────────────────────────────────────

fn t_sched_all_due_at_start() -> bool {
    TempScheduler::new().next_to_measure(0).is_some()
}

fn t_sched_critical_first() -> bool {
    // Au démarrage tous dus → capteur critique (idx 0) choisi en premier
    TempScheduler::new().next_to_measure(0) == Some(0)
}

fn t_sched_not_due_after_record() -> bool {
    let mut sched = TempScheduler::new();
    // Changement lent (delta ≈ 0) → intervalle = CRITICAL_READ_INTERVAL_MS * 4 = 2000 ms
    sched.record_measurement(0, 0.0, 0);
    sched.next_to_measure(100) != Some(0) // 100 ms après : pas encore dû
}

fn t_sched_fast_change_short_interval() -> bool {
    let mut sched = TempScheduler::new();
    // Delta initial = |25 - 0| = 25 > 1 °C → fast → next_due = 0 + 500 = 500 ms
    sched.record_measurement(0, 25.0, 0);
    sched.next_to_measure(CRITICAL_READ_INTERVAL_MS) == Some(0)
}

fn t_sched_failure_no_hammer() -> bool {
    // Après un échec, le capteur ne doit pas être re-sélectionné immédiatement
    let mut sched = TempScheduler::new();
    sched.record_failure(0, 0);
    sched.next_to_measure(100) != Some(0) // 100 ms après : pas encore dû
}

fn t_sched_failure_retries() -> bool {
    // Après SENSOR_FAILURE_RETRY_MS, le capteur doit redevenir dû
    let mut sched = TempScheduler::new();
    sched.record_failure(0, 0);
    sched.next_to_measure(SENSOR_FAILURE_RETRY_MS) == Some(0)
}

fn t_sched_slow_change_long_interval() -> bool {
    let mut sched = TempScheduler::new();
    // 1. Mesure rapide (delta grand) → next_due[0] = 500
    sched.record_measurement(0, 25.0, 0);
    // 2. Mesure stable (delta petit) → next_due[0] = 500 + 500*4 = 2500
    sched.record_measurement(0, 25.01, CRITICAL_READ_INTERVAL_MS);

    let not_yet = sched.next_to_measure(CRITICAL_READ_INTERVAL_MS + 500) != Some(0); // à 1000 ms
    let due_now = sched.next_to_measure(CRITICAL_READ_INTERVAL_MS * 5)   == Some(0); // à 2500 ms
    not_yet && due_now
}

// ── Tests — PID ───────────────────────────────────────────────────────────────

fn t_pid_proportional() -> bool {
    let mut pid = PidController::new(2.0, 0.0, 0.0, -100.0, 100.0);
    let out = pid.update(5.0, 3.0, 0.1); // error = 2.0, kp = 2 → 4.0
    (out - 4.0).abs() < 1e-3
}

fn t_pid_output_clamped() -> bool {
    let mut pid = PidController::new(10.0, 0.0, 0.0, 0.0, 1.0);
    let out = pid.update(100.0, 0.0, 0.1); // 10*100 = 1000 → clampé à 1.0
    (out - 1.0).abs() < 1e-3
}

fn t_pid_integral_accumulates() -> bool {
    let mut pid = PidController::new(0.0, 1.0, 0.0, -100.0, 100.0);
    pid.update(5.0, 0.0, 1.0); // intégrale = 5, sortie = 5
    let out = pid.update(5.0, 0.0, 1.0); // intégrale = 10, sortie = 10
    (out - 10.0).abs() < 1e-2
}

fn t_pid_no_derivative_kick() -> bool {
    // kd élevé : sans le fix, le reset causerait un spike à -1000 (clamped)
    let mut pid = PidController::new(0.0, 0.0, 100.0, -1000.0, 1000.0);
    pid.update(0.0, 10.0, 1.0); // prev_error = -10 après ce cycle
    pid.reset();
    let out = pid.update(0.0, 10.0, 1.0); // dérivée doit être 0, pas (-10-0)/1*100
    out == 0.0
}

fn t_pid_reset_clears_integral() -> bool {
    let mut pid = PidController::new(0.0, 1.0, 0.0, -100.0, 100.0);
    pid.update(5.0, 0.0, 1.0); // intégrale = 5
    pid.reset();
    let out = pid.update(5.0, 0.0, 1.0); // intégrale repart de 0 → sortie = 5
    (out - 5.0).abs() < 1e-2
}

// ── USB ───────────────────────────────────────────────────────────────────────

fn usb_write(serial: &mut SerialPort<hal::usb::UsbBus>, data: &[u8]) {
    let mut pos = 0;
    while pos < data.len() {
        match serial.write(&data[pos..]) {
            Ok(n)  => pos += n,
            Err(_) => break,
        }
    }
}

fn wait_ms(
    timer:   &hal::Timer,
    ms:      u64,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial:  &mut SerialPort<hal::usb::UsbBus>,
) {
    let end = timer.get_counter().ticks() + ms * 1_000;
    while timer.get_counter().ticks() < end {
        if usb_dev.poll(&mut [serial]) {
            let mut buf = [0u8; 64];
            serial.read(&mut buf).ok();
        }
    }
}

// ── Entrée ────────────────────────────────────────────────────────────────────

#[hal::entry]
fn main() -> ! {
    let mut pac      = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    let clocks = hal::clocks::init_clocks_and_plls(
        12_000_000u32,
        pac.XOSC, pac.CLOCKS, pac.PLL_SYS, pac.PLL_USB,
        &mut pac.RESETS, &mut watchdog,
    ).ok().unwrap();

    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    static mut USB_BUS: Option<UsbBusAllocator<hal::usb::UsbBus>> = None;
    let usb_alloc = unsafe {
        USB_BUS = Some(UsbBusAllocator::new(hal::usb::UsbBus::new(
            pac.USBCTRL_REGS, pac.USBCTRL_DPRAM,
            clocks.usb_clock, true, &mut pac.RESETS,
        )));
        (*core::ptr::addr_of!(USB_BUS)).as_ref().unwrap()
    };

    let mut serial  = SerialPort::new(usb_alloc);
    let mut usb_dev = UsbDeviceBuilder::new(usb_alloc, UsbVidPid(0x2e8a, 0x0005))
        .device_class(0x02)
        .build();

    // Attendre 5 s pour laisser le temps d'ouvrir le moniteur série.
    wait_ms(&timer, 5_000, &mut usb_dev, &mut serial);

    // ── Runner ────────────────────────────────────────────────────────────────

    type TestFn = fn() -> bool;
    let tests: &[(&str, TestFn)] = &[
        // Sécurité
        ("safety: HP > seuil",                t_safety_hp),
        ("safety: T compresseur > seuil",     t_safety_comp_temp),
        ("safety: BP < seuil",                t_safety_bp),
        ("safety: lecture invalide ignoree",  t_safety_invalid_ignored),
        // Compresseur
        ("compresseur: mise en marche",       t_compressor_on),
        ("compresseur: arret",                t_compressor_off),
        ("compresseur: hysteresis in-band",   t_compressor_hysteresis),
        ("compresseur: interlock allowed",    t_compressor_allowed_interlock),
        ("compresseur: lecture invalide",     t_compressor_invalid_keeps_off),
        // Haut voltage
        ("HV: actif chambre froide",          t_hv_on_when_cold),
        ("HV: inactif chambre chaude",        t_hv_off_when_warm),
        ("HV: desactive par target",          t_hv_off_by_target),
        ("HV: inactif sans lecture",          t_hv_off_no_valid_reading),
        // Isopropanol
        ("iso: erreur positive -> duty > 0",  t_iso_positive_error),
        ("iso: erreur negative -> duty = 0",  t_iso_negative_error_is_zero),
        ("iso: lecture invalide -> duty = 0", t_iso_invalid_is_zero),
        // Planificateur
        ("scheduler: tous dus au demarrage",  t_sched_all_due_at_start),
        ("scheduler: critique en premier",    t_sched_critical_first),
        ("scheduler: pas du apres record",    t_sched_not_due_after_record),
        ("scheduler: changement rapide",      t_sched_fast_change_short_interval),
        ("scheduler: changement lent",        t_sched_slow_change_long_interval),
        ("scheduler: echec pas de hammer",    t_sched_failure_no_hammer),
        ("scheduler: echec retry apres 1s",   t_sched_failure_retries),
        // PID
        ("PID: action proportionnelle",       t_pid_proportional),
        ("PID: sortie clampee",               t_pid_output_clamped),
        ("PID: integrale s accumule",         t_pid_integral_accumulates),
        ("PID: pas de spike apres reset",      t_pid_no_derivative_kick),
        ("PID: reset efface l integrale",     t_pid_reset_clears_integral),
    ];

    let (mut passed, mut failed) = (0u32, 0u32);

    usb_write(&mut serial, b"=== test_control ===\r\n\r\n");

    for (name, f) in tests {
        let ok = f();
        if ok { passed += 1; } else { failed += 1; }

        let mut line: String<72> = String::new();
        let _ = write!(line, "[{}] {}\r\n", if ok { " OK " } else { "FAIL" }, name);
        usb_write(&mut serial, line.as_bytes());
        usb_dev.poll(&mut [&mut serial]);
    }

    let mut summary: String<40> = String::new();
    let _ = write!(summary, "\r\n{}/{} passes\r\n", passed, passed + failed);
    usb_write(&mut serial, summary.as_bytes());

    loop { usb_dev.poll(&mut [&mut serial]); }
}
