use core::f32::consts::{FRAC_1_PI, PI};
use esp_println::println;
use fugit::{Duration, HertzU32, MicrosDurationU32, Rate};

use hal::{
    adc::{AdcPin, ADC},
    analog::ADC1,
    dac::DAC1,
    efuse::{Efuse, ADC_VREF},
    gpio::{Analog, Gpio13, GpioPin, Output, PushPull},
    prelude::*,
    Delay,
};
use sogi_pll::{PllConfig, SogiPll};

use crate::{ledc::CURRENT_PHASE, SINE_FREQ};

const IDEAL_VREF: u32 = 1100;

const OMEGA_ZERO: f32 = 2.0 * PI * SINE_FREQ.to_Hz() as f32;

const SAMPLE_RATE: HertzU32 = Rate::<u32, 1, 1>::Hz(12000);
const SAMPLE_TIME_DURATION: MicrosDurationU32 = Duration::<u32, 1, 1000000>::from_rate(SAMPLE_RATE);
const SAMPLE_TIME: f32 = 1.0 / SAMPLE_RATE.to_Hz() as f32;

// const RAD_TO_DEG: f32 = 1.0 / (2.0 * PI);
const MV_TO_V: f32 = 1.0 / 1000.0;
const PI2: f32 = PI * 2.0;

const PHASE_OFFSET_RAD: f32 = 35.0 * (PI / 180.0);

pub fn f32_to_idsp(x: f32) -> i32 {
    let out = if 0.0 < x && x < PI {
        (x * FRAC_1_PI) * i32::MAX as f32
    } else if (PI..PI2).contains(&x) {
        // x / PI to go from pi..2pi to 1..2
        // - 2.0 to go from 1..2 to -1..0
        (x * FRAC_1_PI - 2.0) * i32::MIN as f32
    } else {
        0.0
    };

    out as i32
}

fn read_actual_vref() -> u32 {
    let vref_fuse: u8 = Efuse::read_field_le(ADC_VREF);

    let mut value = (vref_fuse & 0b1111) as i8;
    // Check the sign
    if vref_fuse >> 4 == 1 {
        value = -value;
    }

    (IDEAL_VREF as i32 + (value as i32 * 7)) as u32
}

/// Correct for 0dB attenuation
fn adc_to_v(measurement: u16, vref: u32) -> f32 {
    let coef_a = (vref * 57431) / 4096;
    let coef_b = 75;

    (((coef_a * measurement as u32 + (65536 / 2)) / 65535) + coef_b) as f32 * MV_TO_V
}

pub struct AdcTaskResources<'a> {
    pub delay: Delay,
    pub adc: ADC<'a, ADC1>,
    pub dac: DAC1<'a, hal::analog::DAC1>,
    pub test_pin: Gpio13<Output<PushPull>>,
    pub v_grid_adc_pin: AdcPin<GpioPin<Analog, 35>, ADC1>,
}

pub fn adc_pll_task(res: &mut AdcTaskResources) -> ! {
    let config = PllConfig {
        sample_time: SAMPLE_TIME,
        sogi_k: 1.0,
        pi_proportional_gain: 178.0,
        pi_integral_gain: 0.0001,
        omega_zero: OMEGA_ZERO,
    };

    let mut pll = SogiPll::new(config);

    let vref = read_actual_vref();
    println!("Vref: {vref}");

    loop {
        res.test_pin.set_output_high(true);

        let v_grid = adc_to_v(
            nb::block!(res.adc.read(&mut res.v_grid_adc_pin)).unwrap(),
            vref,
        );

        let v_grid = (v_grid - 0.512) * 5.0;

        let pll_result = pll.update(v_grid);

        let phase = pll_result.theta + PHASE_OFFSET_RAD;

        let idsp_phase = f32_to_idsp(phase);
        critical_section::with(|cs| {
            CURRENT_PHASE.replace(cs, idsp_phase);
        });

        // let v_rms = pll_result.v_rms();
        // critical_section::with(|cs| {
        //     SYNC_TIMEOUT.replace(cs, v_rms < 0.1);
        // });

        // let freq_hz = pll_result.omega * RAD_TO_DEG;

        // res.dac.write(((pll_result.theta / (2.0 * PI)) * 254.0) as u8);
        // res.dac.write((cossin(idsp_phase).0 / 16777216 + 128) as u8);
        // res.dac.write((v_rms * 200.0) as u8);
        // res.dac.write((v_grid * 500.0 + 100.0 ) as u8);

        res.test_pin.set_output_high(false);

        res.delay.delay_nanos(SAMPLE_TIME_DURATION.to_nanos());
    }
}
