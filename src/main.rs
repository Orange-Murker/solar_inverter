#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![allow(clippy::empty_loop)]

mod control;
mod ledc;
mod mppt;

use core::cell::RefCell;

use control::{adc_pll_task, AdcTaskResources};
use critical_section::Mutex;
use esp_backtrace as _;
use fugit::{HertzU32, Rate};
use hal::cpu_control::{CpuControl, Stack};
use hal::dac::DAC1;
use hal::i2c::I2C;
use hal::{
    adc::{AdcConfig, Attenuation, ADC},
    analog::ADC1,
    clock::ClockControl,
    gpio::IO,
    interrupt::{self, Priority},
    ledc::{
        channel::{self, ChannelIFace},
        timer::{self, TimerIFace},
        HighSpeed, LEDC,
    },
    peripherals::{Interrupt, Peripherals, TIMG1},
    prelude::*,
    timer::{Timer, Timer0, TimerGroup},
};
use mppt::{mppt_task, MpptTaskResources};

// PWM frequency should ideally be a multiple of the sine wave frequency
const SINE_FREQ: HertzU32 = Rate::<u32, 1, 1>::Hz(50);
const PWM_FREQ: HertzU32 = Rate::<u32, 1, 1>::kHz(24);

static mut APP_CORE_STACK: Stack<8192> = Stack::new();

pub static TIMER10: Mutex<RefCell<Option<Timer<Timer0<TIMG1>>>>> = Mutex::new(RefCell::new(None));

#[entry]
fn main() -> ! {
    let peripherals = Peripherals::take();
    let system = peripherals.SYSTEM.split();
    let clocks = ClockControl::boot_defaults(system.clock_control).freeze();

    let timer_group1 = TimerGroup::new(peripherals.TIMG1, &clocks);
    let timer10 = timer_group1.timer0;
    let timer11 = timer_group1.timer1;

    let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);

    let test_pin = io.pins.gpio13.into_push_pull_output();

    // Configure ADC
    let analog = peripherals.SENS.split();
    let v_grid_pin_pos = io.pins.gpio32.into_analog();
    let v_grid_pin_neg = io.pins.gpio33.into_analog();
    let mut adc1_config = AdcConfig::new();

    let v_grid_adc_pin_pos = adc1_config.enable_pin(v_grid_pin_pos, Attenuation::Attenuation0dB);
    let v_grid_adc_pin_neg = adc1_config.enable_pin(v_grid_pin_neg, Attenuation::Attenuation0dB);

    let adc1 = ADC::<ADC1>::adc(analog.adc1, adc1_config).unwrap();

    let dac_pin = io.pins.gpio25.into_analog();
    let dac1 = DAC1::dac(analog.dac1, dac_pin).unwrap();

    {
        let peripherals = unsafe { Peripherals::steal() };

        // Invert data
        peripherals
            .SENS
            .sar_read_ctrl()
            .modify(|_, w| w.sar1_data_inv().set_bit());
    }

    let pwm_high = io.pins.gpio18.into_push_pull_output();
    let pwm_low = io.pins.gpio19.into_push_pull_output();
    let boost = io.pins.gpio21.into_push_pull_output();

    let ledc = LEDC::new(peripherals.LEDC, &clocks);

    let mut hstimer0 = ledc.get_timer::<HighSpeed>(timer::Number::Timer0);

    hstimer0
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty8Bit,
            clock_source: timer::HSClockSource::APBClk,
            frequency: 24u32.kHz(),
        })
        .unwrap();

    // PWM High
    let mut channel0 = ledc.get_channel(channel::Number::Channel0, pwm_high);
    channel0
        .configure(channel::config::Config {
            timer: &hstimer0,
            duty_pct: 50,
            pin_config: channel::config::PinConfig::PushPull,
        })
        .unwrap();

    // PWM Low
    let mut channel1 = ledc.get_channel(channel::Number::Channel1, pwm_low);
    channel1
        .configure(channel::config::Config {
            timer: &hstimer0,
            duty_pct: 50,
            pin_config: channel::config::PinConfig::PushPull,
        })
        .unwrap();

    // Boost Converter
    let mut channel2 = ledc.get_channel(channel::Number::Channel2, boost);
    channel2
        .configure(channel::config::Config {
            timer: &hstimer0,
            duty_pct: 50,
            pin_config: channel::config::PinConfig::PushPull,
        })
        .unwrap();

    {
        // The peripherals do not implement into_inner() so we have to steal it here
        // This is fine because we only have one reference as the old reference is not accessible anymore
        let ledc = unsafe { Peripherals::steal().LEDC };

        // Enable the LEDC interrupt on every timer overflow
        ledc.int_ena()
            .modify(|_, w| w.hstimer0_ovf_int_ena().set_bit());
        interrupt::enable(Interrupt::LEDC, Priority::Priority3)
            .expect("Could not enable the LEDC interrupt");
    }

    let sda = io.pins.gpio16;
    let scl = io.pins.gpio17;
    let i2c0 = I2C::new(peripherals.I2C0, sda, scl, 50u32.kHz(), &clocks);

    let mut adc_task_resources = AdcTaskResources {
        timer: timer10,
        adc: adc1,
        dac: dac1,
        test_pin,
        v_grid_adc_pin_pos,
        v_grid_adc_pin_neg,
    };

    let mut cpu_control = CpuControl::new(system.cpu_control);
    let cpu1_fnctn = || {
        adc_pll_task(&mut adc_task_resources);
    };
    let _guard = cpu_control
        .start_app_core(unsafe { &mut APP_CORE_STACK }, cpu1_fnctn)
        .unwrap();

    let mppt_task_resources = MpptTaskResources {
        timer: timer11,
        i2c: i2c0,
        boost_pwm: channel2,
    };

    mppt_task(mppt_task_resources);
}
