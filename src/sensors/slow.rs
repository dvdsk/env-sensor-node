use defmt::unwrap;
use embassy_futures::{join, yield_now};
use embassy_time::{with_timeout, Duration, Timer};
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;

use protocol::downcast_err::{ConcreteErrorType, I2cError, UartError};
use protocol::large_bedroom::{Device, LargeBedroom as LB};

use bosch_bme680::{Bme680, MeasurementData};
use sht31::mode::{Sht31Measure, Sht31Reader, SingleShot};
use sht31::SHT31;

use crate::channel::Channel;

pub async fn read<I2C, TX, RX>(
    mut sht: SHT31<SingleShot, I2C>,
    mut bme: Bme680<I2C, impl DelayNs>,
    mut mhz: mhzx::MHZ<TX, RX>,
    publish: &Channel,
) where
    I2C: I2c,
    I2C::Error: defmt::Format,
    <I2C as embedded_hal_async::i2c::ErrorType>::Error: Into<I2cError>,
    TX: embedded_io_async::Write,
    TX::Error: defmt::Format + Into<UartError>,
    RX: embedded_io_async::Read,
    RX::Error: defmt::Format + Into<UartError>,
{
    // sht works in two steps
    //  - send measure command before sleep
    //  - then read
    if let Err(err) = sht.measure().await {
        let err = protocol::large_bedroom::SensorError::Sht31(err);
        let err = protocol::large_bedroom::Error::Running(err);
        publish.send_error(err)
    }
    Timer::after_secs(1).await;

    loop {
        let sht_read = with_timeout(Duration::from_millis(100), sht.read());
        yield_now().await;
        let bme_measure = bme.measure();
        yield_now().await;
        let mhz_measure = with_timeout(Duration::from_millis(100), mhz.read_co2());
        yield_now().await;
        let (bme_res, sht_res, mhz_res) = join::join3(bme_measure, sht_read, mhz_measure).await;
        yield_now().await;

        match bme_res {
            Ok(MeasurementData {
                pressure,
                gas_resistance,
                ..
            }) => {
                let gas_resistance = unwrap!(gas_resistance); // sensor is on
                publish.send_p0(LB::GassResistance(gas_resistance));
                publish.send_p0(LB::Pressure(pressure));
            }
            Err(err) => {
                let err = protocol::large_bedroom::SensorError::Bme680(err.strip_generics());
                let err = protocol::large_bedroom::Error::Running(err);
                publish.send_error(err)
            }
        }

        match sht_res {
            Ok(Ok(sht31::Reading {
                temperature,
                humidity,
            })) => {
                publish.send_p0(LB::Temperature(temperature));
                publish.send_p0(LB::Humidity(humidity));
            }
            Ok(Err(err)) => {
                let err = protocol::large_bedroom::SensorError::Sht31(err);
                let err = protocol::large_bedroom::Error::Running(err);
                publish.send_error(err)
            }
            Err(_timeout) => {
                let err = protocol::large_bedroom::Error::Timeout(Device::Sht31);
                publish.send_error(err)
            }
        }

        match mhz_res {
            Ok(Ok(mhzx::Measurement { co2, .. })) => {
                publish.send_p0(LB::Co2(co2));
            }
            Ok(Err(err)) => {
                let err = err.strip_generics();
                let err = protocol::large_bedroom::SensorError::Mhz14(err);
                let err = protocol::large_bedroom::Error::Running(err);
                publish.send_error(err)
            }
            Err(_timeout) => {
                let err = protocol::large_bedroom::Error::Timeout(Device::Mhz14);
                publish.send_error(err)
            }
        }

        // sht works in two steps
        //  - send measure command before sleep
        //  - then read
        if let Err(err) = sht.measure().await {
            let err = protocol::large_bedroom::SensorError::Sht31(err);
            let err = protocol::large_bedroom::Error::Running(err);
            publish.send_error(err)
        }
        Timer::after_secs(1).await;
    }
}
