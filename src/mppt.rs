use ads1x1x::{
    Ads1x1x, ChannelSelection, DataRate16Bit, DynamicOneShot, FullScaleRange, SlaveAddr,
};
// use esp_println::println;
use fugit::{Duration, MicrosDurationU64, Rate};
use hal::{
    gpio::{GpioPin, Output, PushPull},
    i2c::I2C,
    ledc::{channel::Channel, HighSpeed},
    peripherals::{I2C0, TIMG1},
    prelude::*,
    timer::Timer1,
    Timer,
};
use nb::block;

const VOLTAGE_MARGIN: i32 = 10;
// 20%
const DUTY_MIN: u32 = 51;
// 80%
const DUTY_MAX: u32 = 204;
const DUTY_STEP: u32 = 1;
const MPPT_TASK_DURATION: MicrosDurationU64 =
    Duration::<u64, 1, 1000000>::from_rate(Rate::<u64, 1, 1>::Hz(100));

// fn approx_eq(a: u32, b: u32) -> bool {
//     let diff = a as i32 - b as i32;
//     diff.abs() < DELTA
// }

fn adc_to_mv(measurement: i16) -> i32 {
    measurement as i32 * 4096 / 32768
}

fn adc_to_ma(measurement: i16) -> i32 {
    let mv = adc_to_mv(measurement);
    (mv - 2440) / 185
}

pub struct MpptTaskResources<'a> {
    pub timer: Timer<Timer1<TIMG1>>,
    pub i2c: I2C<'a, I2C0>,
    pub boost_pwm: Channel<'a, HighSpeed, GpioPin<Output<PushPull>, 21>>,
}

pub fn mppt_task(mut res: MpptTaskResources) -> ! {
    let address = SlaveAddr::Default;
    let mut adc = Ads1x1x::new_ads1115(res.i2c, address);
    adc.set_full_scale_range(FullScaleRange::Within4_096V)
        .unwrap();
    // Weird how this is necessary
    hal::xtensa_lx::timer::delay(1);
    adc.set_data_rate(DataRate16Bit::Sps860).unwrap();

    res.timer.start(MPPT_TASK_DURATION);

    let mut previous_voltage: i32 = 1;
    let mut previous_current: i32 = 1;
    let mut duty: u32 = 0;
    loop {
        let voltage =
            adc_to_mv(block!(DynamicOneShot::read(&mut adc, ChannelSelection::SingleA0)).unwrap());
        let current =
            adc_to_ma(block!(DynamicOneShot::read(&mut adc, ChannelSelection::SingleA1)).unwrap());

        let delta_voltage = voltage - previous_voltage;
        let delta_current = current - previous_current;

        // println!("voltage: {}, current: {}", voltage, current);

        let di_dv = delta_current.checked_div(delta_voltage).unwrap_or(0);

        // i +v * di/dv != 0
        if current + voltage * di_dv > VOLTAGE_MARGIN {
            let i_v = -current.checked_div(voltage).unwrap_or(0);
            // di/dv == -i/v
            if di_dv - i_v < VOLTAGE_MARGIN {
                // Increase duty cycle
                let inc = duty.saturating_add(DUTY_STEP);
                if inc <= DUTY_MAX {
                    duty = inc;
                }
            } else {
                // Decrease duty cycle
                let dec = duty.saturating_sub(DUTY_STEP);
                if dec >= DUTY_MIN {
                    duty = dec;
                }
            }
            res.boost_pwm.set_duty_hw(duty);
        }

        previous_voltage = voltage;
        previous_current = current;

        block!(res.timer.wait()).unwrap();
    }
}
