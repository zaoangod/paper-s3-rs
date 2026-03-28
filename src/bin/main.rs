#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::InputConfig;
use esp_hal::gpio::Pull;
use esp_hal::gpio::{Input, Pin};
use esp_hal::i2c::master;
use esp_hal::main;
use esp_hal::time::{Duration, Instant, Rate};
use log::info;
use paper_s3::driver::gt911::gt911;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

    // -----------------GT911 Touch-----------------
    // Configure the touch INT line as an input early.
    // This avoids the pin floating in a potentially problematic state, and matches M5Unified's
    // usage of GPIO48 as the touch wakeup pin.
    let _touch_int = Input::new(peripherals.GPIO48, InputConfig::default().with_pull(Pull::Up));
    // Touch (GT911) on I2C1: SDA=GPIO41, SCL=GPIO42 (from M5GFX autodetect).
    info!("initialize I2C1 for GT911 (SDA=GPIO41, SCL=GPIO42)");
    let i2c_config = master::Config::default()
        .with_frequency(Rate::from_khz(400))
        .with_software_timeout(master::SoftwareTimeout::Transaction(Duration::from_millis(25)));
    let mut i2c = master::I2c::new(peripherals.I2C1, i2c_config)
        .expect("GT911 I2C Initialization Failure")
        .with_sda(peripherals.GPIO41.degrade())
        .with_scl(peripherals.GPIO42.degrade());
    info!("probing GT911 on I2C1");

    // 尝试检测 GT911 的 I2C 地址，最长等待0.5秒
    let start_time = Instant::now();
    let mut touch_address: u8 = 0;
    while touch_address == 0 && start_time.elapsed() < Duration::from_millis(500) {
        touch_address = detect_gt911_address(&mut i2c);
        if touch_address == 0 {
            info!("GT911 not detected, retrying in 100ms...");
            // 延时 100ms
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_millis(100) {}
        }
    }

    let mut gt911 = gt911::GT911::new(touch_address);
    if let Err(e) = gt911.init(&mut i2c) {
        info!("GT911 init failed: {:?}, entering idle loop", e);
        loop {
            // 初始化失败时的空循环
        }
    }
    // .address(touch_address)
    // .size(ed047tc1::WIDTH, ed047tc1::HEIGHT)
    /*
    M5GFX config for PaperS3 uses x_max=539 y_max=959 offset_rotation=1, meaning the raw touch coordinates are rotated relative to our 960x540 framebuffer.
    This mapping yields x in 0..960 and y in 0..540.
    */
    // .orientation(gt911::Orientation::Portrait)
    // .build();

    // Best-effort read to confirm bus is alive.
    // if let Ok(pid) = gt911.read_product_id() {
    //     info!("GT911 product id: {:#x?}", pid);
    // }
    //
    //
    //
    //
    //
    //
    //
    let mut shutdown_armed_until: Option<Instant> = None;
    let mut refresh_armed_until: Option<Instant> = None;
    let mut last_touch_error_log: Option<Instant> = None;

    loop {
        /*if let Ok(point) = gt911.get_single_touch(&mut i2c) {
            // point can be Some (pressed or moved) or None (released)
            info!("{:?}", point)
        } else {
            // ignore because nothing has happened since last poll => Error::NotReady
        }*/
        if let Ok(point) = gt911.get_multi_touch(&mut i2c) {
            // point can be Some (pressed or moved) or None (released)
            info!("{:?}", point)
        } else {
            // ignore because nothing has happened since last poll => Error::NotReady
        }
    }
}
fn detect_gt911_address(i2c: &mut master::I2c<'_, esp_hal::Blocking>) -> u8 {
    for addr in [gt911::GT911_I2C_ADDRESS_5D, gt911::GT911_I2C_ADDRESS_14] {
        match read_gt911_product_id(i2c, addr) {
            Ok(pid) => {
                info!(
                    "GT911 product id @0x{:02X}: {:02x} {:02x} {:02x} {:02x}",
                    addr, pid[0], pid[1], pid[2], pid[3]
                );
                return addr;
            }
            Err(_) => {
                // 静默失败，不在每次探测时打印错误
            }
        }
    }
    0
}

fn read_gt911_product_id(i2c: &mut master::I2c<'_, esp_hal::Blocking>, addr: u8) -> Result<[u8; 4], master::Error> {
    let product_id_reg: u16 = 0x8140;
    let tx_buf: [u8; 2] = [(product_id_reg >> 8) as u8, (product_id_reg & 0xFF) as u8];
    let mut rx_buf: [u8; 4] = [0; 4];
    i2c.write_read(addr, &tx_buf, &mut rx_buf)?;
    Ok(rx_buf)
}
