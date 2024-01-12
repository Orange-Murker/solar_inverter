const VOLTAGE_MARGIN: i32 = 10;

struct MpptState {
    previous_voltage: i32,
    previous_current: i32,
}

// fn approx_eq(a: u32, b: u32) -> bool {
//     let diff = a as i32 - b as i32;
//     diff.abs() < DELTA
// }

fn mppt() {
    let mut state = MpptState {
        previous_voltage: 1,
        previous_current: 1,
    };

    loop {
        let voltage = 1;
        let current = 1;

        let delta_voltage = voltage - state.previous_voltage;
        let delta_current = current - state.previous_current;

        let di_dv = delta_current / delta_voltage;
        let iv = -current / voltage;

        // These if statements will not execute if dIdV is approximately equal to iv
        if di_dv > iv + VOLTAGE_MARGIN {
            // Increase PWM
        } else if di_dv < iv - VOLTAGE_MARGIN {
            // Decrease PWM
        }

        if delta_current > VOLTAGE_MARGIN {
            // Increase PWM
        } else if delta_current < -VOLTAGE_MARGIN {
            // Decrease PWM
        } else {
            return;
        }

        state.previous_voltage = voltage;
        state.previous_current = current;
    }
}
