use core::cell::RefCell;

use critical_section::Mutex;
use hal::{peripherals::Peripherals, prelude::*};

use crate::{PWM_FREQ, SINE_FREQ, TEST_PIN};

const WAVE_UPDATES_PER_SINE_PERIOD: u32 = PWM_FREQ.to_Hz() / SINE_FREQ.to_Hz();

const PHASE_CHANGE_PER_UPDATE: u32 = i32::MAX as u32 / WAVE_UPDATES_PER_SINE_PERIOD;

pub static CURRENT_PHASE: Mutex<RefCell<i32>> = Mutex::new(RefCell::new(0));
pub static SYNC_TIMEOUT: Mutex<RefCell<bool>> = Mutex::new(RefCell::new(true));

#[interrupt]
fn LEDC() {
    // This is fine because we only have one reference as the old reference is not accessible anymore
    // and this is the only interrupt that is using the LEDC peripheral
    let ledc = unsafe { Peripherals::steal().LEDC };
    // Clear the interrupt
    ledc.int_clr.write(|w| w.lstimer0_ovf_int_clr().set_bit());

    // Disable the output on timeout
    critical_section::with(|cs| {
        if *SYNC_TIMEOUT.borrow_ref(cs) {
            ledc.ch0_conf0.modify(|_, w| w.sig_out_en().clear_bit());
        } else {
            ledc.ch0_conf0.modify(|_, w| w.sig_out_en().set_bit());
        }
    });

    critical_section::with(|cs| {
        TEST_PIN
            .borrow_ref_mut(cs)
            .as_mut()
            .unwrap()
            .set_high()
            .unwrap();
    });

    let phase = critical_section::with(|cs| CURRENT_PHASE.take(cs));

    let sin = idsp::cossin(phase).1 as i64;
    let duty = ((sin.abs() * 255) / i32::MAX as i64) as u32;

    ledc.ch0_duty.write(|w| unsafe { w.bits(duty << 4) });
    ledc.ch0_conf1.write(|w| unsafe {
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
    ledc.ch0_conf0.modify(|_, w| w.para_up().set_bit());

    let new_phase = phase.wrapping_add(PHASE_CHANGE_PER_UPDATE as i32);
    critical_section::with(|cs| {
        CURRENT_PHASE.replace(cs, new_phase);
    });

    critical_section::with(|cs| {
        TEST_PIN
            .borrow_ref_mut(cs)
            .as_mut()
            .unwrap()
            .set_low()
            .unwrap();
    });
}
