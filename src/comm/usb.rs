//! Transport série USB — écriture bornée et maintien de l'énumération.

use rp2040_hal as hal;
use usb_device::{class_prelude::UsbError, prelude::UsbDevice};
use usbd_serial::SerialPort;

/// Port série USB avec buffers dédiés — RX 128 o, TX 512 o
/// (cf. `SerialPort::new_with_store`).
pub type Serial<'a> = SerialPort<'a, hal::usb::UsbBus, [u8; 128], [u8; 512]>;

/// Écriture bloquante bornée.
///
/// Le buffer TX de `usbd-serial` (128 o) est plus petit qu'une ligne `STATE`
/// (~140 o) : il faut poller pour le drainer entre deux écritures partielles.
/// Le deadline de 10 ms évite de bloquer la boucle de contrôle si l'hôte ne
/// lit pas (terminal fermé).
pub fn usb_write(
    timer:   &hal::Timer,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial:  &mut Serial<'_>,
    data:    &[u8],
) {
    let deadline = timer.get_counter().ticks() + 10_000;
    let mut pos = 0;
    while pos < data.len() {
        match serial.write(&data[pos..]) {
            Ok(n) => pos += n,
            Err(UsbError::WouldBlock) => {
                usb_dev.poll(&mut [serial]);
                if timer.get_counter().ticks() >= deadline { break; }
            }
            Err(_) => break,
        }
    }
}

/// Poll l'USB pendant `ms` millisecondes — maintient l'énumération vivante
/// pendant les phases d'attente.
pub fn keepalive(
    timer: &hal::Timer,
    ms: u64,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial:  &mut Serial<'_>,
) {
    let end = timer.get_counter().ticks() + ms * 1_000;
    while timer.get_counter().ticks() < end { usb_dev.poll(&mut [serial]); }
}
