use core::cell::RefCell;

use critical_section::Mutex;
use hal::{
    peripherals::{Peripherals, LEDC},
    prelude::*,
};

use crate::{PWM_FREQ, SINE_FREQ};

const WAVE_UPDATES_PER_SINE_PERIOD: u32 = PWM_FREQ.to_Hz() / SINE_FREQ.to_Hz();

const PHASE_CHANGE_PER_UPDATE: u32 = u32::MAX / WAVE_UPDATES_PER_SINE_PERIOD;

pub static CURRENT_PHASE: Mutex<RefCell<i32>> = Mutex::new(RefCell::new(0));
pub static SYNC_TIMEOUT: Mutex<RefCell<bool>> = Mutex::new(RefCell::new(false));

fn set_high_side(ledc: &LEDC) {
    ledc.hsch1_conf0().modify(|_, w| w.sig_out_en().clear_bit());
    ledc.hsch0_conf0().modify(|_, w| w.sig_out_en().set_bit());
}

fn set_low_side(ledc: &LEDC) {
    ledc.hsch0_conf0().modify(|_, w| w.sig_out_en().clear_bit());
    ledc.hsch1_conf0().modify(|_, w| w.sig_out_en().set_bit());
}

fn disable_output(ledc: &LEDC) {
    ledc.hsch0_conf0().modify(|_, w| w.sig_out_en().clear_bit());
    ledc.hsch1_conf0().modify(|_, w| w.sig_out_en().clear_bit());
}

#[interrupt]
fn LEDC() {
    // This is fine because we only have one reference as the old reference is not accessible anymore
    // and this is the only interrupt that is using the LEDC peripheral
    let ledc = unsafe { Peripherals::steal().LEDC };
    // Clear the interrupt
    ledc.int_clr().write(|w| w.hstimer0_ovf_int_clr().set_bit());

    let phase = critical_section::with(|cs| CURRENT_PHASE.take(cs));

    let sin = idsp::cossin(phase).0 as i64;
    // 95% duty cycle max
    let duty = ((sin.abs() * 242) / i32::MAX as i64) as u32;
    let high_side = sin > 0;

    // Disable the output on timeout
    // Set duty cycle per channel
    critical_section::with(|cs| {
        if *SYNC_TIMEOUT.borrow_ref(cs) {
            disable_output(&ledc);
        } else if high_side {
            set_high_side(&ledc);
            ledc.hsch0_duty().write(|w| unsafe { w.bits(duty << 4) });
            ledc.hsch0_conf1().write(|w| unsafe {
                w.duty_start()
                    .set_bit()
                    .duty_inc()
                    .set_bit()
                    .duty_num()
                    .bits(0x1)
                    .duty_cycle()
                    .bits(0x1)
                    .duty_scale()
                    .bits(0x0)
            });
        } else {
            set_low_side(&ledc);
            ledc.hsch1_duty().write(|w| unsafe { w.bits(duty << 4) });
            ledc.hsch1_conf1().write(|w| unsafe {
                w.duty_start()
                    .set_bit()
                    .duty_inc()
                    .set_bit()
                    .duty_num()
                    .bits(0x1)
                    .duty_cycle()
                    .bits(0x1)
                    .duty_scale()
                    .bits(0x0)
            });
        }
    });

    let new_phase = phase.wrapping_add(PHASE_CHANGE_PER_UPDATE as i32);
    critical_section::with(|cs| {
        CURRENT_PHASE.replace(cs, new_phase);
    });
}
