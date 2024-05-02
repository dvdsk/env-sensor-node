use defmt::info;
use embassy_embedded_hal::shared_bus;
use embassy_futures::join;
use embassy_stm32::i2c::I2c;
use embassy_stm32::mode::Async;
use embassy_stm32::peripherals::I2C1;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::priority_channel::PriorityChannel;
use protocol::large_bedroom::{self, LargeBedroom};
use protocol::Sensor;

use embassy_time::{with_timeout, Delay, Duration};
use max44009::{Max44009, SlaveAddr};
use protocol::extended_errors::ConcreteErrorType;

pub mod fast;
pub mod slow;

/// Higher prio will be send earlier
pub struct PriorityValue {
    priority: u8,
    pub value: Sensor,
}

impl PriorityValue {
    pub fn low_priority(&self) -> bool {
        self.priority < 2
    }
    pub fn error(error: large_bedroom::Error) -> Self {
        Self {
            priority: 0,
            value: Sensor::LargeBedroomError(error),
        }
    }

    fn p0(value: LargeBedroom) -> Self {
        Self {
            priority: 0,
            value: Sensor::LargeBedroom(value),
        }
    }
    fn p1(value: LargeBedroom) -> Self {
        Self {
            priority: 1,
            value: Sensor::LargeBedroom(value),
        }
    }

    fn p2(value: LargeBedroom) -> PriorityValue {
        Self {
            priority: 2,
            value: Sensor::LargeBedroom(value),
        }
    }

    pub fn critical_error(error: large_bedroom::Error) -> Self {
        Self {
            priority: 10,
            value: Sensor::LargeBedroomError(error),
        }
    }
}

impl Eq for PriorityValue {}
impl PartialEq for PriorityValue {
    fn eq(&self, other: &Self) -> bool {
        self.priority.eq(&other.priority)
    }
}

impl PartialOrd for PriorityValue {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.priority.cmp(&other.priority))
    }
}

impl Ord for PriorityValue {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

pub async fn init_then_measure(
    publish: &PriorityChannel<NoopRawMutex, PriorityValue, embassy_sync::priority_channel::Max, 20>,
    i2c: Mutex<NoopRawMutex, I2c<'static, I2C1, Async>>,
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

    let sensors_fast = fast::read(max44009, /*buttons,*/ &publish);
    let sensors_slow = slow::read(sht, bme, &publish);
    join::join(sensors_fast, sensors_slow).await;

    defmt::unreachable!();
}
