#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![allow(clippy::empty_loop)]

mod ledc;

use core::cell::RefCell;

use crate::ledc::{CURRENT_PHASE, SYNC_TIMEOUT};
use critical_section::Mutex;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};
use embedded_hal_async::digital::Wait;
use esp_backtrace as _;
use esp_println::println;
use fugit::{HertzU32, Rate};
use hal::{
    clock::ClockControl,
    embassy,
    gpio::{Gpio8, Output, PushPull, IO},
    interrupt::{self, Priority},
    ledc::{
        channel::{self, ChannelIFace},
        timer::{self, TimerIFace},
        LSGlobalClkSource, LowSpeed, LEDC,
    },
    peripherals::{Interrupt, Peripherals},
    prelude::*,
    systimer::SystemTimer,
};

// PWM frequency should ideally be a multiple of the sine wave frequency
const SINE_FREQ: HertzU32 = Rate::<u32, 1, 1>::Hz(1);
const SINE_PERIOD: Duration = Duration::from_micros(1_000_000 / SINE_FREQ.to_Hz() as u64);
// 5% margin
const PERIOD_MARGIN: Duration = Duration::from_micros(SINE_PERIOD.as_micros() / 20);
const PWM_FREQ: HertzU32 = Rate::<u32, 1, 1>::kHz(18);

static TEST_PIN: Mutex<RefCell<Option<Gpio8<Output<PushPull>>>>> = Mutex::new(RefCell::new(None));

#[main]
async fn main(_spawner: Spawner) -> ! {
    // #[entry]
    // fn main() -> ! {
    let peripherals = Peripherals::take();
    let system = peripherals.SYSTEM.split();
    let clocks = ClockControl::boot_defaults(system.clock_control).freeze();

    embassy::init(&clocks, SystemTimer::new(peripherals.SYSTIMER));

    let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);
    let pwm = io.pins.gpio6.into_push_pull_output();

    let test_pin = io.pins.gpio8.into_push_pull_output();
    critical_section::with(|cs| {
        TEST_PIN.replace(cs, Some(test_pin));
    });

    let mut zero_cross_pin = io.pins.gpio10.into_floating_input();
    interrupt::enable(Interrupt::GPIO, Priority::Priority2)
        .expect("Could not enable the GPIO interrupt");

    // Using a scope to make sure that the LEDC struct cannot be used after setting up the peripheral
    {
        let mut ledc = LEDC::new(peripherals.LEDC, &clocks);

        ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);

        let mut lstimer0 = ledc.get_timer::<LowSpeed>(timer::Number::Timer0);

        lstimer0
            .configure(timer::config::Config {
                duty: timer::config::Duty::Duty8Bit,
                clock_source: timer::LSClockSource::APBClk,
                frequency: PWM_FREQ,
            })
            .unwrap();

        let mut channel0 = ledc.get_channel(channel::Number::Channel0, pwm);
        channel0
            .configure(channel::config::Config {
                timer: &lstimer0,
                duty_pct: 75,
                pin_config: channel::config::PinConfig::PushPull,
            })
            .unwrap();
    }

    {
        // The peripherals doe not implement into_inner() so we have to steal it here
        // This is fine because we only have one reference as the old reference is not accessible anymore
        let ledc = unsafe { Peripherals::steal().LEDC };

        // Enable the LEDC interrupt on every timer overflow
        ledc.int_ena
            .modify(|_, w| w.lstimer0_ovf_int_ena().set_bit());
        interrupt::enable(Interrupt::LEDC, Priority::Priority5)
            .expect("Could not enable the LEDC interrupt");
    }

    loop {
        let zero_cross_future = zero_cross_pin.wait_for_any_edge();
        let timeout_future = Timer::after(SINE_PERIOD.checked_add(PERIOD_MARGIN).unwrap());
        let select_result = select(zero_cross_future, timeout_future).await;
        match select_result {
            Either::First(zero_cross_future) => {
                zero_cross_future.unwrap();

                critical_section::with(|cs| {
                    SYNC_TIMEOUT.replace(cs, false);
                });
                if zero_cross_pin.is_input_high() {
                    critical_section::with(|cs| {
                        CURRENT_PHASE.replace(cs, 0);
                    });
                    println!("HIGH");
                } else {
                    println!("LOW");
                }
            }
            Either::Second(_) => {
                critical_section::with(|cs| {
                    SYNC_TIMEOUT.replace(cs, true);
                });
                println!("Timeout");
            }
        }
    }
}
