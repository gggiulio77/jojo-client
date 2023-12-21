use std::time::Duration;

use anyhow::{bail, Ok, Result};
use esp_idf_hal::{
    gpio,
    rmt::{config::TransmitConfig, FixedLengthSignal, PinState, Pulse, TxRmtDriver, CHANNEL0},
};
use rgb::RGB8;

pub struct Neopixel<'a> {
    tx_rtm_driver: TxRmtDriver<'a>,
}

impl Neopixel<'_> {
    pub fn new(led: gpio::Gpio48, channel: CHANNEL0) -> Result<Self> {
        let config = TransmitConfig::new().clock_divider(1);
        let tx = TxRmtDriver::new(channel, led, &config)?;
        Ok(Self { tx_rtm_driver: tx })
    }

    pub fn set(&mut self, rgb: RGB8) -> Result<()> {
        // e.g. rgb: (1,2,4)
        // G        R        B
        // 7      0 7      0 7      0
        // 00000010 00000001 00000100
        let color: u32 = ((rgb.g as u32) << 16) | ((rgb.r as u32) << 8) | rgb.b as u32;
        let ticks_hz = self.tx_rtm_driver.counter_clock()?;
        let t0h = Pulse::new_with_duration(ticks_hz, PinState::High, &Duration::from_nanos(350))?;
        let t0l = Pulse::new_with_duration(ticks_hz, PinState::Low, &Duration::from_nanos(800))?;
        let t1h = Pulse::new_with_duration(ticks_hz, PinState::High, &Duration::from_nanos(700))?;
        let t1l = Pulse::new_with_duration(ticks_hz, PinState::Low, &Duration::from_nanos(600))?;
        let mut signal = FixedLengthSignal::<24>::new();

        for i in (0..24).rev() {
            let p = 2_u32.pow(i);
            let bit = p & color != 0;
            let (high_pulse, low_pulse) = if bit { (t1h, t1l) } else { (t0h, t0l) };
            signal.set(23 - i as usize, &(high_pulse, low_pulse))?;
        }
        self.tx_rtm_driver.start(signal)?;

        Ok(())
    }
}

pub fn rainbow(mut led: Neopixel) -> Result<()> {
    let mut i: u32 = 0;
    loop {
        let rgb = hsv2rgb(i, 100, 20)?;
        led.set(rgb)?;
        if i == 360 {
            i = 0;
        }
        i += 1;

        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn hsv2rgb(h: u32, s: u32, v: u32) -> Result<RGB8> {
    if h > 360 || s > 100 || v > 100 {
        bail!("The given HSV values are not in valid range");
    }
    let s = s as f64 / 100.0;
    let v = v as f64 / 100.0;
    let c = s * v;
    let x = c * (1.0 - (((h as f64 / 60.0) % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b);
    if h < 60 {
        r = c;
        g = x;
        b = 0.0;
    } else if (60..120).contains(&h) {
        r = x;
        g = c;
        b = 0.0;
    } else if (120..180).contains(&h) {
        r = 0.0;
        g = c;
        b = x;
    } else if (180..240).contains(&h) {
        r = 0.0;
        g = x;
        b = c;
    } else if (240..300).contains(&h) {
        r = x;
        g = 0.0;
        b = c;
    } else {
        r = c;
        g = 0.0;
        b = x;
    }

    Ok(RGB8 {
        r: ((r + m) * 255.0) as u8,
        g: ((g + m) * 255.0) as u8,
        b: ((b + m) * 255.0) as u8,
    })
}
