use embassy_embedded_hal::shared_bus;
use embassy_futures::join;
use embassy_stm32::i2c::I2c;
use embassy_stm32::mode::Async;
use embassy_stm32::peripherals::{I2C1, USART1};
use embassy_stm32::usart::Uart;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{with_timeout, Delay, Duration};
use max44009::{Max44009, SlaveAddr};
use protocol::downcast_err::ConcreteErrorType;

use crate::channel::Channel;

pub mod fast;
pub mod slow;

pub async fn init_then_measure(
    publish: &Channel,
    i2c: Mutex<NoopRawMutex, I2c<'static, I2C1, Async>>,
    usart: Uart<'static, USART1, Async>,
) -> Result<(), protocol::large_bedroom::Error> {
    use protocol::large_bedroom::Device;
    use protocol::large_bedroom::Error;
    use protocol::large_bedroom::SensorError;

    let bme_config = bosch_bme680::Configuration::default();
    let bme = with_timeout(
        Duration::from_secs(12),
        bosch_bme680::Bme680::new(
            shared_bus::asynch::i2c::I2cDevice::new(&i2c),
            bosch_bme680::DeviceAddress::Secondary,
            Delay,
            &bme_config,
            20,
        ),
    )
    .await
    .map_err(|_| Error::SetupTimedOut(Device::Bme680))?
    .map_err(|err| err.strip_generics())
    .map_err(SensorError::Bme680)
    .map_err(Error::Setup)?;

    let mut max44009 = Max44009::new(
        shared_bus::asynch::i2c::I2cDevice::new(&i2c),
        SlaveAddr::default(),
    );
    with_timeout(
        Duration::from_millis(250),
        max44009.set_measurement_mode(max44009::MeasurementMode::Continuous),
    )
    .await
    .map_err(|_| Error::SetupTimedOut(Device::Max44))?
    .map_err(|err| err.strip_generics())
    .map_err(SensorError::Max44)
    .map_err(Error::Setup)?;

    let sht = sht31::SHT31::new(shared_bus::asynch::i2c::I2cDevice::new(&i2c), Delay)
        .with_mode(sht31::mode::SingleShot)
        .with_unit(sht31::TemperatureUnit::Celsius)
        .with_accuracy(sht31::Accuracy::High);

    let (tx, rx) = usart.split();
    let mut usart_buf = [0u8; 9*10]; // 9 byte messages
    let rx = rx.into_ring_buffered(&mut usart_buf);
    let mhz = mhzx::MHZ::from_tx_rx(tx, rx);

    let sensors_fast = fast::read(max44009, /*buttons,*/ &publish);
    let sensors_slow = slow::read(sht, bme, mhz, &publish);
    join::join(sensors_fast, sensors_slow).await;

    defmt::unreachable!();
}
