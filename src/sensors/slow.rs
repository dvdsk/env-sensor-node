use defmt::unwrap;
use embassy_futures::{join, yield_now};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::priority_channel::{self, PriorityChannel};
use embassy_time::Timer;
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;

use protocol::extended_errors::ConcreteErrorType;
use protocol::large_bedroom::LargeBedroom as LB;

use bosch_bme680::{Bme680, MeasurementData};
use sht31::mode::{Sht31Measure, Sht31Reader, SingleShot};
use sht31::SHT31;

use super::PriorityValue as PV;

pub async fn read<I2C>(
    mut sht: SHT31<SingleShot, I2C>,
    // mut bme: Bme680<I2C, impl DelayNs>,
    publish: &PriorityChannel<NoopRawMutex, PV, priority_channel::Max, 20>,
) where
    I2C: I2c,
    I2C::Error: defmt::Format,
    <I2C as embedded_hal_async::i2c::ErrorType>::Error: Into<protocol::extended_errors::I2cError>,
{
    unwrap!(sht.measure().await);
    Timer::after_secs(1).await;

    loop {
        let sht_read = sht.read();
        yield_now().await;
        // let bme_measure = bme.measure();
        // yield_now().await;
        // let (bme_res, sht_res) = join::join(bme_measure, sht_read).await;
        let sht_res = sht_read.await;
        yield_now().await;

        // match bme_res {
        //     Ok(MeasurementData {
        //         pressure,
        //         gas_resistance,
        //         ..
        //     }) => {
        //         let gas_resistance = unwrap!(gas_resistance); // sensor is on
        //         let _ignore = publish.try_send(PV::p0(LB::GassResistance(gas_resistance)));
        //         let _ignore = publish.try_send(PV::p0(LB::Pressure(pressure)));
        //     }
        //     Err(err) => {
        //         let err = protocol::large_bedroom::SensorError::Bme680(err.strip_generics());
        //         let err = protocol::large_bedroom::Error::Running(err);
        //         let _ignore = publish.try_send(PV::error(err));
        //     }
        // }

        match sht_res {
            Ok(sht31::Reading {
                temperature,
                humidity,
            }) => {
                let _ignore = publish.try_send(PV::p0(LB::Temperature(temperature)));
                let _ignore = publish.try_send(PV::p0(LB::Humidity(humidity)));
            }
            Err(err) => {
                let err = protocol::large_bedroom::SensorError::Sht31(err);
                let err = protocol::large_bedroom::Error::Running(err);
                let _ignore = publish.try_send(PV::error(err));
            }
        }
        if let Err(err) = sht.measure().await {
            let err = protocol::large_bedroom::SensorError::Sht31(err);
            let err = protocol::large_bedroom::Error::Running(err);
            let _ignore = publish.try_send(PV::error(err));
        }
        Timer::after_secs(1).await;
    }
}
