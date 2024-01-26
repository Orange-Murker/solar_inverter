#![allow(clippy::comparison_chain)]

use ads1x1x::{
    Ads1x1x, ChannelSelection, DataRate16Bit, DynamicOneShot, FullScaleRange, SlaveAddr,
};
use esp_println::println;
use fugit::{Duration, MicrosDurationU32, Rate};
use hal::{
    gpio::{GpioPin, Output, PushPull},
    i2c::I2C,
    ledc::{channel::Channel, HighSpeed},
    peripherals::I2C0,
    prelude::*,
    Delay,
};
use nb::block;

const MARGIN: i32 = 0;
// 20%
const DUTY_MIN: u32 = 51;
// 80%
const DUTY_MAX: u32 = 204;
const DUTY_STEP: u32 = 1;
const MPPT_TASK_DURATION: MicrosDurationU32 =
    Duration::<u32, 1, 1000000>::from_rate(Rate::<u32, 1, 1>::Hz(200));

fn adc_to_mv(measurement: i16) -> i32 {
    measurement as i32 * 512 / 32768
}

fn adc_to_ma(measurement: i16) -> i32 {
    // TODO: Use the actual formula for the current sensor
    adc_to_mv(measurement)
}

pub struct MpptTaskResources<'a> {
    pub delay: Delay,
    pub i2c: I2C<'a, I2C0>,
    pub boost_pwm: Channel<'a, HighSpeed, GpioPin<Output<PushPull>, 21>>,
}

pub fn mppt_task(res: MpptTaskResources) -> ! {
    let address = SlaveAddr::Default;
    let mut adc = Ads1x1x::new_ads1115(res.i2c, address);
    adc.set_full_scale_range(FullScaleRange::Within0_512V)
        .unwrap();
    // Weird how this is necessary
    hal::xtensa_lx::timer::delay(1);
    adc.set_data_rate(DataRate16Bit::Sps128).unwrap();

    let mut previous_voltage: i32 = 1;
    let mut previous_current: i32 = 1;
    let mut duty: u32 = DUTY_MIN;
    loop {
        let voltage = adc_to_mv(
            block!(DynamicOneShot::read(&mut adc, ChannelSelection::SingleA0)).unwrap_or(0),
        );
        let current = adc_to_ma(
            block!(DynamicOneShot::read(&mut adc, ChannelSelection::SingleA1)).unwrap_or(0),
        );

        let delta_voltage = voltage - previous_voltage;
        let delta_current = current - previous_current;

        let di_dv = delta_current.checked_div(delta_voltage).unwrap_or(0);

        // dV == 0
        if delta_voltage < MARGIN {
            // dI != 0
            if delta_current > MARGIN {
                let inc = duty.saturating_add(DUTY_STEP);
                if inc <= DUTY_MAX {
                    duty = inc;
                }
            } else if delta_current < MARGIN {
                let dec = duty.saturating_sub(DUTY_STEP);
                if dec >= DUTY_MIN {
                    duty = dec;
                }
            }
        } else if delta_voltage > MARGIN {
            // I + dI_dV * V != 0
            if current + di_dv * voltage > MARGIN {
                let dec = duty.saturating_sub(DUTY_STEP);
                if dec >= DUTY_MIN {
                    duty = dec;
                }
            } else if current + di_dv * voltage < MARGIN {
                let inc = duty.saturating_add(DUTY_STEP);
                if inc <= DUTY_MAX {
                    duty = inc;
                }
            }
        }

        println!("voltage: {}, current: {}, duty: {}", voltage, current, duty);

        previous_voltage = voltage;
        previous_current = current;

        res.delay.delay_nanos(MPPT_TASK_DURATION.to_nanos());
    }
}
