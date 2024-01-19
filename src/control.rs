use core::f32::consts::{FRAC_1_PI, PI};
use esp_println::println;
use fugit::{Duration, Rate};

use hal::{
    adc::{AdcPin, ADC},
    analog::ADC1,
    dac::DAC1,
    efuse::{Efuse, ADC_VREF},
    gpio::{Analog, Gpio12, GpioPin, Output, PushPull},
    peripherals::TIMG1,
    prelude::{nb::block, *},
    timer::Timer0,
    Timer,
};
use sogi_pll::{PllConfig, SogiPll};

use crate::{ledc::CURRENT_PHASE, SINE_FREQ};

const IDEAL_VREF: u32 = 1100;

const OMEGA_ZERO: f32 = 2.0 * PI * SINE_FREQ.to_Hz() as f32;

const SAMPLE_RATE: Rate<u64, 1, 1> = Rate::<u64, 1, 1>::Hz(5000);
const SAMPLE_TIME_DURATION: Duration<u64, 1, 1000000> =
    Duration::<u64, 1, 1000000>::from_rate(SAMPLE_RATE);
const SAMPLE_TIME: f32 = 1.0 / SAMPLE_RATE.to_Hz() as f32;
// const RAD_TO_DEG: f32 = 1.0 / (2.0 * PI);
const MV_TO_V: f32 = 1.0 / 1000.0;
const PI2: f32 = PI * 2.0;

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

pub struct AdcTaskResources {
    pub timer: Timer<Timer0<TIMG1>>,
    pub adc: ADC<'static, ADC1>,
    pub dac: DAC1<'static, hal::analog::DAC1>,
    pub test_pin: Gpio12<Output<PushPull>>,
    pub v_grid_adc_pin_pos: AdcPin<GpioPin<Analog, 32>, ADC1>,
    pub v_grid_adc_pin_neg: AdcPin<GpioPin<Analog, 33>, ADC1>,
}

pub fn adc_pll_task(res: &mut AdcTaskResources) {
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

    res.timer.start(SAMPLE_TIME_DURATION);

    loop {
        res.test_pin.set_high().unwrap();

        let v_grid_pos = adc_to_v(
            nb::block!(res.adc.read(&mut res.v_grid_adc_pin_pos)).unwrap(),
            vref,
        );
        let v_grid_neg = adc_to_v(
            nb::block!(res.adc.read(&mut res.v_grid_adc_pin_neg)).unwrap(),
            vref,
        );
        let v_grid = v_grid_pos - v_grid_neg;
        // println!("{}, {}", v_grid_pos, v_grid_neg);

        let pll_result = pll.update(v_grid);

        critical_section::with(|cs| {
            CURRENT_PHASE.replace(cs, f32_to_idsp(pll_result.theta));
        });

        // let freq_hz = pll_result.omega * RAD_TO_DEG;

        // let v_rms = pll_result.v_rms();

        // res.dac.write(((pll_result.theta / (2.0 * PI)) * 254.0) as u8);
        res.dac.write((v_grid * 200.0 + 120.0) as u8);

        res.test_pin.set_low().unwrap();

        block!(res.timer.wait()).unwrap();
    }
}
