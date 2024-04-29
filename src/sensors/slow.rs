use defmt::unwrap;
use embassy_futures::{join, yield_now};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::priority_channel::{self, PriorityChannel};
use embassy_time::Timer;
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;

use protocol::large_bedroom::LargeBedroom as LB;

use bosch_bme680::{Bme680, MeasurementData};
use sht31::mode::{Sht31Measure, Sht31Reader, SingleShot};
use sht31::SHT31;

use super::PriorityValue;

pub async fn read<I2C>(
    mut sht: SHT31<SingleShot, I2C>,
    mut bme: Bme680<I2C, impl DelayNs>,
    publish: &PriorityChannel<NoopRawMutex, PriorityValue, priority_channel::Max, 20>,
) where
    I2C: I2c,
    I2C::Error: defmt::Format,
{
    unwrap!(sht.measure().await);
    Timer::after_secs(1).await;

    loop {
        let sht_measure = sht.read();
        yield_now().await;
        let bme_measure = bme.measure();
        yield_now().await;
        let (bme_res, sht_res) = join::join(bme_measure, sht_measure).await;
        yield_now().await;
        let sht31::Reading {
            temperature,
            humidity,
        } = unwrap!(sht_res);
        let MeasurementData {
            pressure,
            gas_resistance,
            ..
        } = unwrap!(bme_res);
        yield_now().await;

        let gas_resistance = unwrap!(gas_resistance); // sensor is on
        let _ignore_full_queue =
            publish.try_send(PriorityValue::p0(LB::GassResistance(gas_resistance)));
        let _ignore_full_queue = publish.try_send(PriorityValue::p0(LB::Pressure(pressure)));
        let _ignore_full_queue = publish.try_send(PriorityValue::p0(LB::Temperature(temperature)));
        let _ignore_full_queue = publish.try_send(PriorityValue::p0(LB::Humidity(humidity)));
        unwrap!(sht.measure().await);
        Timer::after_secs(1).await;
    }
}
