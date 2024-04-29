use crate::sensors::PriorityValue;
use defmt::{info, unwrap, warn};
use embassy_net::driver::Driver;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Ipv4Address, Stack};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::priority_channel::{self, PriorityChannel};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embedded_io_async::Write;
use protocol::SensorMessage;

type Msg = SensorMessage<6>;

async fn get_messages(
    publish: &PriorityChannel<NoopRawMutex, PriorityValue, priority_channel::Max, 20>,
    msg: &mut Msg,
) {
    use protocol::Sensor::LargeBedroom as LB;

    msg.values.clear();
    let next = publish.receive().await;
    unwrap!(msg.values.push(LB(next.value)));

    if next.low_priority() {
        let deadline = Instant::now() + Duration::from_millis(200);
        while msg.space_left() {
            let until = deadline.saturating_duration_since(Instant::now());
            match with_timeout(until, publish.receive()).await {
                Ok(low_prio) if next.low_priority() => {
                    unwrap!(msg.values.push(LB(low_prio.value)));
                }
                Ok(high_prio) => {
                    unwrap!(msg.values.push(LB(high_prio.value)));
                    break;
                }
                Err(_timeout) => break,
            }
        }
    } else {
        while msg.space_left() {
            let Ok(next) = publish.try_receive() else {
                break;
            };
            unwrap!(msg.values.push(LB(next.value)));
        }
    }
}

pub async fn send_published(
    stack: &Stack<impl Driver>,
    publish: &PriorityChannel<NoopRawMutex, PriorityValue, priority_channel::Max, 20>,
) {
    let mut rx_buffer = [0; 100];
    let mut tx_buffer = [0; Msg::ENCODED_SIZE * 4];

    let mut msg = Msg::new();
    let mut encoded_msg_buffer = [0; Msg::ENCODED_SIZE];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    socket.set_timeout(Some(Duration::from_secs(10)));
    let host_addr = Ipv4Address::new(192, 168, 1, 46);
    let host_port = 1234;

    loop {
        let connected = socket.remote_endpoint().is_some();
        if !connected {
            if let Err(e) = socket.connect((host_addr, host_port)).await {
                warn!("connect error: {:?}", e);
                Timer::after_secs(1).await;
                continue;
            }
        }

        get_messages(publish, &mut msg).await;
        let to_send = msg.encode_slice(&mut encoded_msg_buffer);

        info!("Connected to {:?}", socket.remote_endpoint());
        if let Err(e) = socket.write_all(to_send).await {
            warn!("write error: {:?}", e);
        }
    }
}
