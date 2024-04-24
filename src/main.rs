#![no_std]
#![no_main]

use defmt::*;
use embassy_embedded_hal::shared_bus;
use embassy_executor::Spawner;
use embassy_futures::join;
use embassy_net::{Ipv4Address, Ipv4Cidr, Stack, StackResources};
use embassy_net_wiznet::chip::W5500;
use embassy_net_wiznet::*;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::i2c::I2c;
use embassy_stm32::mode::Async;
use embassy_stm32::peripherals::SPI1;
use embassy_stm32::spi::{Config as SpiConfig, Spi};
use embassy_stm32::time::Hertz;
use embassy_stm32::Config;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Delay, Duration, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_io_async::Write;
use heapless::Vec;
use max44009::{Max44009, SlaveAddr};
use rand::{rngs::SmallRng, Rng, SeedableRng};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

embassy_stm32::bind_interrupts!(struct Irqs {
    I2C3_EV => embassy_stm32::i2c::EventInterruptHandler<embassy_stm32::peripherals::I2C3>;
    I2C3_ER => embassy_stm32::i2c::ErrorInterruptHandler<embassy_stm32::peripherals::I2C3>;
});

#[embassy_executor::task]
async fn ethernet_task(
    runner: Runner<
        'static,
        W5500,
        ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
        ExtiInput<'static>,
        Output<'static>,
    >,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<Device<'static>>) -> ! {
    stack.run().await
}

async fn gen_random_number() -> u64 {
    let seed = 0u64;
    let mut rng = SmallRng::seed_from_u64(seed);
    let seed = rng.gen();
    info!("Seed: {}", seed);
    seed
}

// 84 Mhz clock stm32f401
fn config() -> Config {
    use embassy_stm32::rcc::{
        AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllSource,
        Sysclk,
    };

    let mut config = Config::default();
    config.rcc.hse = Some(Hse {
        freq: Hertz(25_000_000),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV25,
        mul: PllMul::MUL336,
        divp: Some(PllPDiv::DIV4),
        divq: None,
        divr: None,
    });
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV2;
    config.rcc.apb2_pre = APBPrescaler::DIV1;
    config.rcc.sys = Sysclk::PLL1_P;
    config
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(config());
    let mut led = Output::new(p.PC13, Level::Low, Speed::Low);
    let seed = gen_random_number().await;

    let i2c = I2c::new(
        p.I2C3,
        p.PA8,
        p.PB4,
        Irqs,
        p.DMA1_CH4,
        p.DMA1_CH2,
        Hertz(50_000),
        Default::default(),
    );
    let i2c: Mutex<NoopRawMutex, _> = Mutex::new(i2c);

    let bme_config = bosch_bme680::Configuration::default();
    let mut bme = unwrap!(
        bosch_bme680::Bme680::new(
            shared_bus::asynch::i2c::I2cDevice::new(&i2c),
            bosch_bme680::DeviceAddress::Secondary,
            Delay,
            &bme_config,
            20
        )
        .await
    );

    let mut max44009 = Max44009::new(
        shared_bus::asynch::i2c::I2cDevice::new(&i2c),
        SlaveAddr::default(),
    );
    unwrap!(
        max44009
            .set_measurement_mode(max44009::MeasurementMode::Continuous)
            .await
    );

    let mut spi_cfg = SpiConfig::default();
    spi_cfg.frequency = Hertz(50_000_000); // todo increase
    let (miso, mosi, clk) = (p.PA6, p.PA7, p.PA5);
    let spi = Spi::new(p.SPI1, clk, mosi, miso, p.DMA2_CH3, p.DMA2_CH0, spi_cfg);
    let cs = Output::new(p.PA4, Level::High, Speed::VeryHigh);
    let w5500_int = ExtiInput::new(p.PA1, p.EXTI1, Pull::Up);
    let w5500_reset = Output::new(p.PA2, Level::High, Speed::Medium);

    let mac_addr = [0x02, 234, 3, 4, 82, 231];
    static STATE: StaticCell<State<8, 8>> = StaticCell::new();
    let state = STATE.init(State::<8, 8>::new());
    let (device, runner) = embassy_net_wiznet::new(
        mac_addr,
        state,
        ExclusiveDevice::new(spi, cs, Delay),
        w5500_int,
        w5500_reset,
    )
    .await;
    unwrap!(spawner.spawn(ethernet_task(runner)));

    // Init network stack
    let mut dns_servers: Vec<_, 3> = Vec::new();
    unwrap!(dns_servers.push(Ipv4Address([192, 168, 1, 1])));
    unwrap!(dns_servers.push(Ipv4Address([192, 168, 1, 1])));
    unwrap!(dns_servers.push(Ipv4Address([192, 168, 1, 1])));
    static STACK: StaticCell<Stack<Device<'static>>> = StaticCell::new();
    static RESOURCES: StaticCell<StackResources<2>> = StaticCell::new();
    let stack = &*STACK.init(Stack::new(
        device,
        embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
            address: Ipv4Cidr::new(Ipv4Address([192, 168, 1, 6]), 24),
            gateway: Some(Ipv4Address([192, 168, 1, 1])),
            dns_servers,
        }),
        RESOURCES.init(StackResources::<2>::new()),
        seed,
    ));

    // Launch network task
    unwrap!(spawner.spawn(net_task(&stack)));

    let mut rx_buffer = [0; 800];
    let mut tx_buffer = [0; 800];
    loop {
        Timer::after_secs(1).await;
        let mut socket = embassy_net::tcp::TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));

        led.set_low();
        info!("Connecting...");
        let host_addr = Ipv4Address::new(192, 168, 1, 46);
        if let Err(e) = socket.connect((host_addr, 1234)).await {
            warn!("connect error: {:?}", e);
            continue;
        }
        info!("Connected to {:?}", socket.remote_endpoint());
        // led.set_high();

        let msg = b"Hello world!\n";
        loop {
            if let Err(e) = socket.write_all(msg).await {
                warn!("write error: {:?}", e);
                break;
            }
            let bme_measure = bme.measure();
            let max_measure = max44009.read_lux();
            let (value, lux) = join::join(bme_measure, max_measure).await;
            info!("bme measured: {}", value);
            info!("lux measured: {}", lux);
            Timer::after_millis(1000).await;
        }
    }
}
