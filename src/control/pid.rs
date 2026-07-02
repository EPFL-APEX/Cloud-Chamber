/// Contrôleur PID discret à temps variable.
///
/// Sortie clampée à [output_min, output_max] avec anti-windup.
/// Le terme dérivé est supprimé au premier appel après `new()` ou `reset()` pour
/// éviter le spike causé par un `prev_error` non initialisé.
#[derive(Clone, Copy, Debug)]
pub struct PidController {
    kp: f32,
    ki: f32,
    kd: f32,
    integral:   f32,
    prev_error: f32,
    output_min: f32,
    output_max: f32,
    /// Vrai après création ou reset — bloque la dérivée au premier appel.
    first_call: bool,
}

impl PidController {
    pub const fn new(
        kp: f32, ki: f32, kd: f32,
        output_min: f32, output_max: f32,
    ) -> Self {
        Self {
            kp, ki, kd,
            integral:   0.0,
            prev_error: 0.0,
            output_min,
            output_max,
            first_call: true,
        }
    }

    /// Met à jour le PID et retourne la commande dans [output_min, output_max].
    ///
    /// `dt_s` doit être > 0 ; si ≤ 0 les termes I et D sont figés.
    pub fn update(&mut self, setpoint: f32, measurement: f32, dt_s: f32) -> f32 {
        let error = setpoint - measurement;

        let derivative = if dt_s > 0.0 && !self.first_call {
            self.integral += error * dt_s;
            // Anti-windup : l'intégrale seule ne peut pas saturer la sortie.
            let ki_safe = if self.ki.abs() > 1e-9 { self.ki } else { 1e-9 };
            self.integral = self.integral.clamp(
                self.output_min / ki_safe,
                self.output_max / ki_safe,
            );
            (error - self.prev_error) / dt_s
        } else if dt_s > 0.0 {
            // Premier appel : on accumule l'intégrale mais on ne dérive pas.
            self.integral += error * dt_s;
            let ki_safe = if self.ki.abs() > 1e-9 { self.ki } else { 1e-9 };
            self.integral = self.integral.clamp(
                self.output_min / ki_safe,
                self.output_max / ki_safe,
            );
            0.0
        } else {
            0.0
        };

        self.first_call = false;
        self.prev_error = error;

        let output = self.kp * error + self.ki * self.integral + self.kd * derivative;
        output.clamp(self.output_min, self.output_max)
    }

    /// Remet l'état interne à zéro. Le prochain appel à `update()` ne produira
    /// pas de spike dérivé (le terme D est supprimé pour ce seul appel).
    pub fn reset(&mut self) {
        self.integral   = 0.0;
        self.prev_error = 0.0;
        self.first_call = true;
    }
}
