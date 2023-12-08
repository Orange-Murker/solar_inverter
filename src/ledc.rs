use hal::{peripherals::Peripherals, prelude::*};

use crate::{PWM_FREQ, SINE_FREQ};

const WAVE_UPDATES_PER_SINE_PERIOD: u32 = PWM_FREQ.to_Hz() / SINE_FREQ.to_Hz();

const PHASE_CHANGE_PER_UPDATE: u32 = i32::MAX as u32 / WAVE_UPDATES_PER_SINE_PERIOD;

static mut CURRENT_STEP: usize = 0;
static mut CURRENT_PHASE: i32 = 0;

// These functions are here for debugging purposes.
// This is faster than using a critical section.
fn set_pin_unsafe(pin: u32) {
    let gpio = unsafe { Peripherals::steal().GPIO };
    gpio.out_w1ts.write(|w| unsafe { w.bits(1 << pin) });
}
fn unset_pin_unsafe(pin: u32) {
    let gpio = unsafe { Peripherals::steal().GPIO };
    gpio.out_w1tc.write(|w| unsafe { w.bits(1 << pin) });
}

#[interrupt]
fn LEDC() {
    // This is fine because we only have one reference as the old reference is not accessible anymore
    // and this is the only interrupt that is using the LEDC peripheral
    let ledc = unsafe { Peripherals::steal().LEDC };
    // Clear the interrupt
    ledc.int_clr.write(|w| w.lstimer0_ovf_int_clr().set_bit());
    set_pin_unsafe(8);

    let i = unsafe { &mut CURRENT_STEP };
    // This is fine because no other interrupt is using this and this interrupt has the highest
    // or the same priority as other interrupts
    let phase = unsafe { &mut CURRENT_PHASE };

    // Technically we could keep incrementing the step counter forever but we don't want to
    // overflow the u32 so we reset it to 0 after one cycle
    if *i >= WAVE_UPDATES_PER_SINE_PERIOD as usize {
        *i = 0;
    }

    let sin = idsp::cossin(*phase).1 as i64;
    let duty = ((sin.abs() * 255) / i32::MAX as i64) as u32;

    // let duty = if *i % 2 == 0 {
    //     192
    // } else {
    //     64
    // };

    // ledc.ch0_conf0.modify(|_, w| w.sig_out_en().set_bit());

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

    *phase = phase.wrapping_add(PHASE_CHANGE_PER_UPDATE as i32);
    *i += 1;

    unset_pin_unsafe(8);
}
