
pub struct SafetyLoop<T, C, L>
where
    T: TemperatureSensor,
    C: CurrentSensor,
    L: ClosureSensor,
{
    temp:    T,
    current: C,
    closure: L,
    state:   SystemState,
}

impl<T, C, L> SafetyLoop<T, C, L>
where
    T: TemperatureSensor,
    C: CurrentSensor,
    L: ClosureSensor,
{
    pub fn run(&mut self) -> ! {
        loop {
            let start = timer.get_counter();

            self.read_critical_sensors();
            self.evaluate_safety();
            self.update_actuators();
            self.push_to_core1();
            watchdog.feed();

            sleep_until(start + LOOP_PERIOD_US);
        }
    }
}