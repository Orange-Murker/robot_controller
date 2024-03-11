mod motor;

use std::sync::{Arc, Mutex};

use embedded_svc::{
    http::Method,
    io::Write,
    wifi::{self, AccessPointConfiguration, AuthMethod},
    ws::FrameType,
};

use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::gpio::*,
    http::server::EspHttpServer,
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, EspWifi},
};
use esp_idf_svc::{
    hal::{
        ledc::{config::TimerConfig, LedcDriver, LedcTimerDriver},
        prelude::Peripherals,
        units::FromValueType,
    },
    sys::EspError,
    sys::ESP_ERR_INVALID_SIZE,
};

use log::*;

use crate::motor::{Direction, MotorControl};

const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASS");
static INDEX_HTML: &str = include_str!("page.html");

// Max payload length
const MAX_LEN: usize = 128;

// Need lots of stack to parse JSON
const STACK_SIZE: usize = 10240;

// Wi-Fi channel, between 1 and 11
const CHANNEL: u8 = 11;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;

    let timer_driver = Arc::new(LedcTimerDriver::new(
        peripherals.ledc.timer0,
        &TimerConfig::default().frequency(100u32.Hz()),
    )?);

    let mut left_motor =
        LedcDriver::new(peripherals.ledc.channel0, timer_driver.clone(), pins.gpio5)?;

    let mut right_motor = LedcDriver::new(peripherals.ledc.channel1, timer_driver, pins.gpio6)?;

    left_motor.set_duty(left_motor.get_max_duty() / 2)?;
    right_motor.set_duty(right_motor.get_max_duty() / 2)?;
    left_motor.disable()?;
    right_motor.disable()?;

    let motor_control = Mutex::new(MotorControl {
        left_step: left_motor,
        left_dir: PinDriver::output(pins.gpio20)?,
        right_step: right_motor,
        right_dir: PinDriver::output(pins.gpio21)?,
    });

    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    let wifi_configuration = wifi::Configuration::AccessPoint(AccessPointConfiguration {
        ssid: SSID.try_into().unwrap(),
        ssid_hidden: false,
        auth_method: AuthMethod::WPA2Personal,
        password: PASSWORD.try_into().unwrap(),
        channel: CHANNEL,
        ..Default::default()
    });
    wifi.set_configuration(&wifi_configuration)?;
    wifi.start()?;
    wifi.wait_netif_up()?;

    info!(
        "Created Wi-Fi with WIFI_SSID `{}` and WIFI_PASS `{}`",
        SSID, PASSWORD
    );

    let server_configuration = esp_idf_svc::http::server::Configuration {
        stack_size: STACK_SIZE,
        ..Default::default()
    };

    // Keep wifi running beyond when this function returns (forever)
    // Do not call this if you ever want to stop or access it later.
    // Otherwise it should be returned from this function and kept somewhere
    // so it does not go out of scope.
    // https://doc.rust-lang.org/stable/core/mem/fn.forget.html
    core::mem::forget(wifi);

    let mut server = EspHttpServer::new(&server_configuration)?;

    server.fn_handler("/", Method::Get, |req| {
        req.into_ok_response()?
            .write_all(INDEX_HTML.as_bytes())
            .map(|_| ())
    })?;

    server.ws_handler("/ws/control", move |ws| {
        if ws.is_new() {
            info!("New WebSocket session {}", ws.session());
            return Ok(());
        } else if ws.is_closed() {
            info!("Closed WebSocket session {}", ws.session());
            return Ok(());
        }

        let (_frame_type, len) = match ws.recv(&mut []) {
            Ok(frame) => frame,
            Err(e) => return Err(e),
        };

        if len > MAX_LEN {
            ws.send(FrameType::Close, &[])?;
            return Err(EspError::from_infallible::<ESP_ERR_INVALID_SIZE>());
        }

        let mut buf = [0; MAX_LEN];
        ws.recv(buf.as_mut())?;
        let Ok(command) = std::str::from_utf8(&buf[..len]) else {
            error!("Could not parse request as UTF-8");
            return Ok(());
        };

        let command = command.trim_matches(char::from(0));

        info!("Received command {}", command);

        let mut command = command.split('-');

        let mut motor_control = motor_control.lock().expect("Could not lock motor_control");

        let direction = if let Some(cmd_dir) = command.next() {
            match cmd_dir {
                "fwd" => Some(Direction::Forward),
                "back" => Some(Direction::Back),
                "left" => Some(Direction::Left),
                "right" => Some(Direction::Right),
                _ => {
                    error!("Invalid command received {}", cmd_dir);
                    None
                }
            }
        } else {
            error!("Did not get a direction");
            None
        };

        if let Some(direction) = direction {
            motor_control
                .set_direction(direction)
                .expect("Could not set direction");
        }

        if let Some(state) = command.next() {
            match state {
                "down" => motor_control.set_enable(true).expect("Could not step"),
                "up" => motor_control.set_enable(false).expect("Could not step"),
                _ => {
                    error!("Invalid button state {}", state);
                }
            }
        }

        Ok(())
    })?;

    // Keep server running beyond when main() returns (forever)
    // Do not call this if you ever want to stop or access it later.
    // Otherwise you can either add an infinite loop so the main task
    // never returns, or you can move it to another thread.
    // https://doc.rust-lang.org/stable/core/mem/fn.forget.html
    core::mem::forget(server);

    // Main task no longer needed, free up some memory
    Ok(())
}
