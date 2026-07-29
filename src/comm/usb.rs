//! Transport série USB — écriture bornée et maintien de l'énumération.

use rp2040_hal as hal;
use usb_device::{class_prelude::UsbError, prelude::UsbDevice};
use usbd_serial::SerialPort;

/// Port série USB avec buffers dédiés — RX 128 o, TX 512 o
/// (cf. `SerialPort::new_with_store`).
pub type Serial<'a> = SerialPort<'a, hal::usb::UsbBus, [u8; 128], [u8; 512]>;

/// Écriture bloquante bornée.
///
/// Le buffer TX de `usbd-serial` (128 o) peut être plus petit qu'une ligne
/// de télémétrie : il faut poller pour le drainer entre deux écritures
/// partielles. Le deadline de 10 ms évite de bloquer la boucle de contrôle
/// si l'hôte ne lit pas (terminal fermé).
///
/// Le deadline est vérifié à chaque tour de boucle, pas seulement dans la
/// branche `WouldBlock` : `serial.write()` peut aussi renvoyer `Ok(0)` en
/// boucle (aucune erreur, aucun progrès) sans qu'on ne s'en aperçoive sinon
/// — bug trouvé pendant l'audit initial de ce module (branche équipe).
pub fn usb_write(
    timer: &hal::Timer,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial: &mut Serial<'_>,
    data: &[u8],
) {
    let deadline = timer.get_counter().ticks() + 10_000;
    let mut pos = 0;
    while pos < data.len() {
        if timer.get_counter().ticks() >= deadline {
            break;
        }
        match serial.write(&data[pos..]) {
            Ok(0) => usb_dev.poll(&mut [serial]),
            Ok(n) => pos += n,
            Err(UsbError::WouldBlock) => usb_dev.poll(&mut [serial]),
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
    serial: &mut Serial<'_>,
) {
    let end = timer.get_counter().ticks() + ms * 1_000;
    while timer.get_counter().ticks() < end {
        usb_dev.poll(&mut [serial]);
    }
}
