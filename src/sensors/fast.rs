use defmt::{unwrap, warn};
use embassy_futures::{
    join::{self, join3},
    yield_now,
};
use embassy_stm32::exti::ExtiInput;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::priority_channel::{self, PriorityChannel};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal_async::i2c::I2c;
use futures::future::join5;
use max44009::Max44009;

use protocol::extended_errors::ConcreteErrorType;
use protocol::large_bedroom::{self, BedButton, LargeBedroom as LB};

use super::PriorityValue as PV;

fn sig_lux_diff(old: f32, new: f32) -> bool {
    let diff = old - new;
    // we do not have f32::abs on embedded
    diff > old / 20.0 || -diff > old / 20.0
}

async fn report_lux<I2C>(
    mut max44: Max44009<I2C>,
    publish: &PriorityChannel<NoopRawMutex, PV, priority_channel::Max, 20>,
) where
    I2C: I2c,
    I2C::Error: defmt::Format,
    <I2C as embedded_hal_async::i2c::ErrorType>::Error: Into<protocol::extended_errors::I2cError>,
{
    let mut prev_lux = f32::MAX;
    let mut last_lux = Instant::now();
    const MIN_INTERVAL: Duration = Duration::from_secs(1);

    loop {
        Timer::after_millis(50).await;
        let lux = match max44.read_lux().await {
            Ok(lux) => lux,
            Err(err) if last_lux.elapsed() > MIN_INTERVAL => {
                let err = large_bedroom::SensorError::Max44(err.strip_generics());
                let err = large_bedroom::Error::Running(err);
                let _ignore = publish.try_send(PV::error(err));
                continue;
            }
            Err(_) => continue,
        };

        let msg = if sig_lux_diff(prev_lux, lux) {
            PV::p2(LB::Brightness(lux))
        } else if last_lux.elapsed() > MIN_INTERVAL {
            PV::p1(LB::Brightness(lux))
        } else {
            yield_now().await;
            continue;
        };

        prev_lux = lux;
        last_lux = Instant::now();
        let _ignore_full_channel = publish.try_send(msg);
    }
}

type Channel = PriorityChannel<NoopRawMutex, PV, priority_channel::Max, 20>;
async fn watch_button(
    mut input: ExtiInput<'static>,
    event: impl Fn(protocol::Press) -> BedButton,
    channel: &Channel,
) {
    let mut went_high_at: Option<Instant> = None;
    loop {
        if let Some(went_high_at) = went_high_at.take() {
            input.wait_for_falling_edge().await;
            let press = went_high_at.elapsed();
            if press > Duration::from_millis(5) {
                let Ok(press) = press.as_millis().try_into() else {
                    warn!("extremely long button press registered, skipping");
                    continue;
                };
                let event = (event)(protocol::Press(press));
                let value = PV::p2(LB::BedButton(event));
                let _ignore_full_channel = channel.try_send(value);
            }
        } else {
            input.wait_for_rising_edge().await;
            went_high_at = Some(Instant::now());
        }
    }
}

pub struct ButtonInputs {
    pub top_left: ExtiInput<'static>,
    pub top_right: ExtiInput<'static>,
    pub middle_inner: ExtiInput<'static>,
    pub middle_center: ExtiInput<'static>,
    pub middle_outer: ExtiInput<'static>,
    pub lower_inner: ExtiInput<'static>,
    pub lower_center: ExtiInput<'static>,
    pub lower_outer: ExtiInput<'static>,
}

pub async fn read<I2C>(
    max44: Max44009<I2C>,
    /*inputs: ButtonInputs,*/
    publish: &PriorityChannel<NoopRawMutex, PV, priority_channel::Max, 20>,
) where
    I2C: I2c,
    I2C::Error: defmt::Format,
    <I2C as embedded_hal_async::i2c::ErrorType>::Error: Into<protocol::extended_errors::I2cError>,
{
    // let watch_buttons_1 = join5(
    //     watch_button(inputs.top_left, BedButton::TopLeft, publish),
    //     watch_button(inputs.top_right, BedButton::TopRight, publish),
    //     watch_button(inputs.middle_inner, BedButton::MiddleInner, publish),
    //     watch_button(inputs.middle_center, BedButton::MiddleCenter, publish),
    //     watch_button(inputs.middle_outer, BedButton::MiddleOuter, publish),
    // );
    //
    // let watch_buttons_2 = join3(
    //     watch_button(inputs.lower_inner, BedButton::LowerInner, publish),
    //     watch_button(inputs.lower_center, BedButton::LowerCenter, publish),
    //     watch_button(inputs.lower_outer, BedButton::LowerOuter, publish),
    // );

    let watch_lux = report_lux(max44, publish);
    watch_lux.await;
    // join::join3(watch_buttons_1, watch_buttons_2, watch_lux).await;
}
