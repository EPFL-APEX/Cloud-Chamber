pub fn main(){
    loop {
        Time start.now();
        State s;
        critical_sensor_read(mut& s);
        reaction_to_readings(mut& s);
        if (start - Time.now() < max_time_step) {
            non_critical_sesnor_read(mut& s);
            data_communication(mut& s);
        };
        if (start - Time.now() < max_time_step) {
            sleep(max_time_step - (start - Time.now()));
        }
    }
}