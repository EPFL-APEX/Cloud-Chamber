//! Driver Triac : PWM secteur synchronisé sur le passage par zéro.
//!
//! Le programme PIO ([`zero_cross.asm`](./zero_cross.asm)) attend les
//! transitions sur la broche de détection de zéro-cross et active la
//! broche de gate du triac pendant un nombre entier de demi-alternances.
//! C'est un "burst-fire" (cycles entiers activés/désactivés en rafale),
//! pas un gradateur à angle de phase : la résolution du rapport cyclique
//! est `1 / period` (`period` = nombre de demi-alternances par période,
//! passé à [`TriacDriver::new`]).
//!
//! Toutes les définitions dépendant du HAL embarqué sont regroupées dans
//! le sous-module [`imp`], compilé uniquement sur cible ARM/RISC-V — même
//! pattern que [`crate::drivers::adc`], pour ne pas casser les tests depuis host.

#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
mod imp {
    #[cfg(all(rp2040, target_arch = "arm"))]
    use rp2040_hal as hal;
    #[cfg(all(rp2350, any(target_arch = "arm", target_arch = "riscv32")))]
    use rp235x_hal as hal;

    use pio::pio_file;
    use hal::gpio::{Pin, PinId, PullType};
    use hal::pio::{
        InstallError, PIOBuilder, PIOExt, PinDir, PinState, Rx, ShiftDirection, StateMachine,
        StateMachineIndex, Tx, UninitStateMachine, PIO,
    };

    use crate::cloud_chamber_hal::actuators::{AnalogActuator, TargetActuator};
    use crate::cloud_chamber_hal::measurement::Measurement;
    use crate::cloud_chamber_hal::ring_buffer::RingBuffer;
    use crate::cloud_chamber_hal::units::{Celsius, Percentage};
    use crate::drivers::regulate_method::{self, PidGains};

    /// Erreur du driver Triac.
    #[derive(Debug)]
    pub enum TriacError {
        /// Pas assez d'espace en mémoire d'instructions PIO pour le programme.
        Install(InstallError),
        /// FIFO TX pleine — la state machine n'a pas consommé le mot précédent.
        /// Ne devrait arriver qu'à une fréquence d'appel de `set_output`
        /// largement supérieure à la fréquence secteur.
        TxFifoFull,
        /// Appel après [`TriacDriver::uninstall`] (ou pendant `drop`).
        Uninstalled,
        /// [`TargetActuator::regulate`] appelé avant que l'historique ne soit
        /// plein — pas assez de points pour un PID fiable. La sortie n'est
        /// *pas* modifiée (elle garde le dernier setpoint appliqué) ; c'est à
        /// l'appelant de décider quoi faire d'un historique encore incomplet.
        HistoryNotFull,
    }

    /// Tout ce qui est libéré ensemble par [`TriacDriver::teardown`] en une seule
    /// `Option` à `take()` plutôt qu'un par ressource. Posséder `gate_pin`/
    /// `zero_cross_pin` (et pas seulement leur index) empêche à la compilation
    /// qu'une même broche soit réutilisée ailleurs pendant que ce driver tourne.
    struct Resources<P, Sm, GateId, GatePull, ZcId, ZcPull>
    where
        P: PIOExt,
        Sm: StateMachineIndex,
        GateId: PinId,
        GatePull: PullType,
        ZcId: PinId,
        ZcPull: PullType,
    {
        pio: PIO<P>,
        sm: StateMachine<(P, Sm), hal::pio::Running>,
        rx: Rx<(P, Sm)>,
        tx: Tx<(P, Sm)>,
        gate_pin: Pin<GateId, P::PinFunction, GatePull>,
        zero_cross_pin: Pin<ZcId, P::PinFunction, ZcPull>,
    }

    /// Driver PWM secteur pour triac, piloté par une state machine PIO.
    ///
    /// `P` est le bloc PIO (`PIO0`/`PIO1`/…), `Sm` le slot de state machine
    /// (`SM0`..`SM3`) — ce driver n'en consomme qu'un seul, le reste du bloc
    /// PIO reste disponible pour d'autres périphériques. `GateId`/`ZcId`
    /// identifient les deux broches consommées par [`new`](Self::new).
    pub struct TriacDriver<P, Sm, GateId, GatePull, ZcId, ZcPull>
    where
        P: PIOExt,
        Sm: StateMachineIndex,
        GateId: PinId,
        GatePull: PullType,
        ZcId: PinId,
        ZcPull: PullType,
    {
        // `Option` : `Drop` interdit de déplacer un champ hors de `self`
        // (E0509), mais `.take()` reste autorisé — un seul point de sortie
        // pour `uninstall` (explicite) et `drop` (implicite).
        resources: Option<Resources<P, Sm, GateId, GatePull, ZcId, ZcPull>>,
        gate_pin_num: u8,
        /// Nombre de demi-alternances par période, moins un (convention de
        /// [`zero_cross.asm`] : `y` compte jusqu'à 0 inclus).
        period_minus_one: u16,
        setpoint: Percentage,
        pid_gains: PidGains,
    }

    impl<P, Sm, GateId, GatePull, ZcId, ZcPull> TriacDriver<P, Sm, GateId, GatePull, ZcId, ZcPull>
    where
        P: PIOExt,
        Sm: StateMachineIndex,
        GateId: PinId,
        GatePull: PullType,
        ZcId: PinId,
        ZcPull: PullType,
    {
        /// Installe le programme PIO et démarre la state machine.
        ///
        /// `mains_half_cycles_per_period` fixe la résolution du rapport
        /// cyclique (résolution = `1 / valeur`) ; ex. `100` → pas de 1 %,
        /// pour un secteur 50 Hz cela étale une période de duty sur 1 seconde.
        pub fn new(
            mut pio: PIO<P>,
            sm: UninitStateMachine<(P, Sm)>,
            gate_pin: Pin<GateId, P::PinFunction, GatePull>,
            zero_cross_pin: Pin<ZcId, P::PinFunction, ZcPull>,
            mains_half_cycles_per_period: u16,
            pid_gains: PidGains,
        ) -> Result<Self, TriacError> {
            let gate_pin_num = gate_pin.id().num;
            let zero_cross_pin_num = zero_cross_pin.id().num;

            let program =
                pio_file!("./src/drivers/zero_cross.asm", select_program("zero_cross")).program;
            let installed = pio.install(&program).map_err(TriacError::Install)?;

            let (mut stopped, rx, tx) = PIOBuilder::from_installed_program(installed)
                .set_pins(gate_pin_num, 1)
                .in_pin_base(zero_cross_pin_num)
                // `set_output`/`packed_word` pack
                // `period << 16 | duty` en supposant que le premier `out`
                // (`out x, 16`) lit les bits de poids faible.
                .out_shift_direction(ShiftDirection::Right)
                .clock_divisor_fixed_point(1, 0)
                .build(sm);

            stopped.set_pindirs([(gate_pin_num, PinDir::Output)]);
            let sm = stopped.start();

            let mut driver = Self {
                resources: Some(Resources { pio, sm, rx, tx, gate_pin, zero_cross_pin }),
                gate_pin_num,
                period_minus_one: mains_half_cycles_per_period.saturating_sub(1),
                setpoint: Percentage(0.0),
                pid_gains,
            };
            // x/y valent 0 avant la première écriture FIFO : pousser un
            // setpoint connu plutôt que tourner sur un mot indéfini au démarrage.
            driver.set_output(Percentage(0.0))?;
            Ok(driver)
        }

        /// Arrête la state machine, force la gate à 0, désinstalle le
        /// programme. `None` si déjà fait (appels suivants, ou après
        /// [`uninstall`](Self::uninstall)).
        #[allow(clippy::type_complexity)]
        fn teardown(
            &mut self,
        ) -> Option<(
            PIO<P>,
            UninitStateMachine<(P, Sm)>,
            Pin<GateId, P::PinFunction, GatePull>,
            Pin<ZcId, P::PinFunction, ZcPull>,
        )> {
            let res = self.resources.take()?;
            let mut stopped = res.sm.stop();
            // La SM désactivée garde le dernier état écrit sur la gate —
            // la forcer à 0 plutôt que laisser le triac indéterminé.
            stopped.set_pins([(self.gate_pin_num, PinState::Low)]);
            let (uninit_sm, installed) = stopped.uninit(res.rx, res.tx);
            let mut pio = res.pio;
            pio.uninstall(installed);
            Some((pio, uninit_sm, res.gate_pin, res.zero_cross_pin))
        }

        /// Désinstalle le programme et rend le bloc PIO, le slot de state
        /// machine et les deux broches, pour être réutilisés ailleurs.
        #[allow(clippy::type_complexity)]
        pub fn uninstall(
            mut self,
        ) -> (
            PIO<P>,
            UninitStateMachine<(P, Sm)>,
            Pin<GateId, P::PinFunction, GatePull>,
            Pin<ZcId, P::PinFunction, ZcPull>,
        ) {
            self.teardown().expect("TriacDriver déjà désinstallé")
        }

        /// Empaquette `duty` en mot 32 bits `period_minus_one << 16 | duty_minus_one`
        /// consommé par le programme PIO (`out x, 16` / `out y, 16`).
        fn packed_word(&self, duty: Percentage) -> u32 {
            let period = self.period_minus_one as u32 + 1;
            let ratio = (duty.0.clamp(0.0, 100.0) / 100.0) * period as f32;
            let on_cycles = (libm::roundf(ratio) as u32).min(period);
            let duty_minus_one = on_cycles.saturating_sub(1).min(self.period_minus_one as u32);
            (self.period_minus_one as u32) << 16 | duty_minus_one
        }
    }

    impl<P, Sm, GateId, GatePull, ZcId, ZcPull> AnalogActuator<Percentage>
        for TriacDriver<P, Sm, GateId, GatePull, ZcId, ZcPull>
    where
        P: PIOExt,
        Sm: StateMachineIndex,
        GateId: PinId,
        GatePull: PullType,
        ZcId: PinId,
        ZcPull: PullType,
    {
        type Error = TriacError;

        fn set_output(&mut self, value: Percentage) -> Result<(), Self::Error> {
            let duty = Percentage(value.0.clamp(0.0, 100.0));
            let word = self.packed_word(duty);
            let res = self.resources.as_mut().ok_or(TriacError::Uninstalled)?;
            if res.tx.write(word) {
                self.setpoint = duty;
                Ok(())
            } else {
                Err(TriacError::TxFifoFull)
            }
        }

        fn get_setpoint(&self) -> Result<Percentage, Self::Error> {
            Ok(self.setpoint)
        }
    }

    impl<P, Sm, GateId, GatePull, ZcId, ZcPull, const N: usize> TargetActuator<Celsius, N>
        for TriacDriver<P, Sm, GateId, GatePull, ZcId, ZcPull>
    where
        P: PIOExt,
        Sm: StateMachineIndex,
        GateId: PinId,
        GatePull: PullType,
        ZcId: PinId,
        ZcPull: PullType,
    {
        type Error = TriacError;

        /// `target: None` coupe la sortie sans passer par le PID (contrat de
        /// [`TargetActuator`]). Retourne `Err(HistoryNotFull)` sans toucher à
        /// la sortie tant que `hist` n'est pas plein : `regulate_method::pid`
        /// panique sinon en indexant l'historique — l'appelant décide comment
        /// réagir à un historique incomplet.
        fn regulate(
            &mut self,
            hist: &RingBuffer<Measurement<Celsius>, N>,
            target: Option<Celsius>,
        ) -> Result<(), Self::Error> {
            let Some(target) = target else {
                return self.set_output(Percentage(0.0));
            };
            if N == 0 || hist.get(N - 1).is_err() {
                return Err(TriacError::HistoryNotFull);
            }

            let correction = regulate_method::pid(target, hist, self.pid_gains);
            self.set_output(Percentage(correction.0))
        }
    }

    impl<P, Sm, GateId, GatePull, ZcId, ZcPull> Drop
        for TriacDriver<P, Sm, GateId, GatePull, ZcId, ZcPull>
    where
        P: PIOExt,
        Sm: StateMachineIndex,
        GateId: PinId,
        GatePull: PullType,
        ZcId: PinId,
        ZcPull: PullType,
    {
        fn drop(&mut self) {
            self.teardown();
        }
    }
}

#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
pub use imp::{TriacDriver, TriacError};
