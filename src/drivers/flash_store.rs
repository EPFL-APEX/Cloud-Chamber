//! Stockage des réglages dans la flash interne, sans système de fichiers.
//!
//! # Pourquoi pas un JSON
//!
//! Un fichier suppose un système de fichiers, donc une couche (LittleFS,
//! FAT) et un parseur JSON, pour ranger quelques octets. On écrit
//! directement la struct sérialisée par [`Settings::to_bytes`], protégée
//! par un magic, une version et un CRC. Lire les réglages devient une
//! comparaison de quatre octets et un CRC, pas une machine à états.
//!
//! # Découpage
//!
//! Ce module ne sait pas parler au contrôleur de flash. Il connaît la
//! *disposition* : un secteur, seize emplacements, écriture en avançant.
//! Les trois opérations matérielles (lire, effacer, programmer) passent par
//! [`FlashOps`], implémenté d'un côté par le vrai driver ROM, de l'autre
//! par un tableau en RAM dans les tests. C'est ce qui rend la logique
//! d'usure testable sur PC, là où elle est invérifiable sur cible.
//!
//! # Écriture en avançant plutôt qu'en place
//!
//! La flash s'efface par secteur de 4 Ko et supporte de l'ordre de 100 000
//! effacements. Réécrire toujours au même endroit coûterait un effacement
//! par sauvegarde. On garde donc seize emplacements de 256 octets dans le
//! secteur et on programme le premier resté vierge ; le secteur n'est
//! effacé que lorsqu'ils sont tous pris, soit une fois toutes les seize
//! sauvegardes. Un emplacement neuf n'a pas besoin d'être effacé : une
//! flash vierge est à 0xFF et programmer ne fait que descendre des bits
//! à 0.
//!
//! # Ce qui reste à écrire : l'implémentation `FlashOps` sur cible
//!
//! Sur RP2040 comme sur RP2350, effacer et programmer se font par les
//! routines de la ROM (`connect_internal_flash`, `flash_exit_xip`,
//! `flash_range_erase`, `flash_range_program`, `flash_flush_cache`,
//! `flash_enter_cmd_xip`). Deux contraintes non négociables :
//!
//! - la séquence coupe le XIP, donc le code qui l'exécute ne peut pas être
//!   *dans* la flash : la fonction doit être en RAM
//!   (`#[link_section = ".data.ram_func"]` + `#[inline(never)]`) ;
//! - aucune interruption ne doit toucher la flash pendant ce temps, d'où
//!   un `critical_section::with` autour, et rien qui puisse s'exécuter sur
//!   le second cœur.
//!
//! Cette partie n'est pas écrite ici tant qu'elle n'a pas été compilée et
//! passée sur la vraie carte : c'est une vingtaine de lignes d'`unsafe` qui
//! briquent la carte si elles sont fausses, et elles ne se relisent pas,
//! elles se testent.

use crate::config::settings::{RECORD_LEN, Settings, SettingsStore, StoreError};

// Ces deux constantes n'ont pas de source amont à laquelle se rattacher, et
// ce n'est pas un oubli : elles ne décrivent pas le microcontrôleur mais la
// puce QSPI soudée à côté. Le RP2040 n'a pas de flash interne du tout, et le
// Pico 2 utilise aussi une flash externe. `rp2040_hal::rom_data` expose bien
// les six fonctions ROM (`connect_internal_flash`, `flash_exit_xip`,
// `flash_range_erase`, `flash_range_program`, `flash_flush_cache`,
// `flash_enter_cmd_xip`) mais aucune constante de géométrie, et les scripts
// de link (`rp2040.x`, `rp2350.x`) ne déclarent que l'origine et la longueur
// de la région FLASH. Le HAL ne peut pas les connaître : il ne sait pas quel
// boîtier est monté.
//
// Ce qui, lui, devrait venir du script de link, c'est l'*adresse* du secteur
// des réglages — cf. `FlashSettingsStore::new`.

/// Taille du secteur d'effacement de la flash QSPI (famille W25Q, 4 Ko).
pub const SECTOR_SIZE: usize = 4096;

/// Taille de la page de programmation. C'est aussi la taille d'un
/// emplacement : programmer moins qu'une page entière est possible, mais
/// aligner les emplacements sur les pages évite qu'une sauvegarde touche
/// deux pages à la fois.
pub const PAGE_SIZE: usize = 256;

/// Nombre de sauvegardes tenables avant qu'un effacement soit nécessaire.
pub const SLOTS: usize = SECTOR_SIZE / PAGE_SIZE;

// Un enregistrement doit tenir dans une page, sinon `save` en écrirait deux
// et la coupure de courant entre les deux laisserait un état à moitié écrit
// que la relecture ne saurait pas détecter. Vérifié à la compilation plutôt
// que par un test : ça vaut pour toutes les cibles, pas seulement l'hôte.
const _: () = assert!(RECORD_LEN <= PAGE_SIZE);

/// Les trois seules choses qu'on demande au matériel.
///
/// Les décalages sont comptés depuis le début de la flash, pas depuis la
/// fenêtre XIP : c'est ce qu'attendent les routines ROM.
pub trait FlashOps {
    /// Lit `buf.len()` octets. La lecture passe par le XIP, elle n'a besoin
    /// d'aucune précaution.
    fn read(&self, offset: u32, buf: &mut [u8]);

    /// Efface un secteur entier — `offset` doit être aligné sur
    /// [`SECTOR_SIZE`].
    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError>;

    /// Programme une page — `offset` doit être aligné sur [`PAGE_SIZE`].
    fn program_page(&mut self, offset: u32, page: &[u8; PAGE_SIZE]) -> Result<(), StoreError>;
}

/// Implémente [`SettingsStore`] au-dessus d'un secteur de flash.
pub struct FlashSettingsStore<F> {
    flash: F,
    /// Décalage du secteur réservé aux réglages. Il doit tomber hors du
    /// binaire : le script de link réserve le dernier secteur.
    base: u32,
}

impl<F: FlashOps> FlashSettingsStore<F> {
    /// `base` doit être aligné sur [`SECTOR_SIZE`], sinon l'effacement
    /// emporterait le secteur voisin — c'est-à-dire, ici, du code.
    ///
    /// À terme cette valeur ne devrait pas être écrite en dur par
    /// l'appelant mais venir du script de link, qui est le seul endroit à
    /// connaître la taille réelle de la flash de la carte : un symbole
    /// placé en fin de région FLASH, réservé pour qu'aucune section n'y
    /// atterrisse. C'est la seule façon d'être sûr que le secteur ne
    /// recouvre pas le binaire.
    pub fn new(flash: F, base: u32) -> Self {
        debug_assert!(
            base as usize % SECTOR_SIZE == 0,
            "le secteur des reglages doit etre aligne"
        );
        Self { flash, base }
    }

    fn slot_offset(&self, slot: usize) -> u32 {
        self.base + (slot * PAGE_SIZE) as u32
    }

    /// Lit l'enregistrement d'un emplacement. `None` si l'emplacement est
    /// vierge — inutile de vérifier un CRC sur du 0xFF.
    fn read_slot(&self, slot: usize) -> Option<[u8; RECORD_LEN]> {
        let mut raw = [0u8; RECORD_LEN];
        self.flash.read(self.slot_offset(slot), &mut raw);
        if raw.iter().all(|&b| b == 0xFF) {
            return None;
        }
        Some(raw)
    }

    /// Rang du premier emplacement vierge, `None` si le secteur est plein.
    ///
    /// On s'arrête au premier vierge sans regarder la suite : l'écriture
    /// avance toujours dans le même sens, donc tout ce qui suit l'est aussi.
    fn first_blank_slot(&self) -> Option<usize> {
        (0..SLOTS).find(|&slot| self.read_slot(slot).is_none())
    }
}

impl<F: FlashOps> SettingsStore for FlashSettingsStore<F> {
    /// Remonte les emplacements du plus récent au plus ancien et renvoie le
    /// premier qui se relit correctement.
    ///
    /// Descendre plutôt que s'arrêter au dernier écrit couvre le cas d'une
    /// coupure de courant en pleine programmation : l'emplacement en cours
    /// est à moitié écrit et son CRC tombe faux, mais le précédent est
    /// intact, et c'est celui-là qu'on veut.
    fn load(&mut self) -> Option<Settings> {
        (0..SLOTS)
            .rev()
            .filter_map(|slot| self.read_slot(slot))
            .find_map(|raw| Settings::from_bytes(&raw))
    }

    fn save(&mut self, settings: &Settings) -> Result<(), StoreError> {
        // Le reste de la page reste à 0xFF, ce qui est justement ce qui
        // signale « emplacement vierge » aux relectures suivantes.
        let mut page = [0xFFu8; PAGE_SIZE];
        page[..RECORD_LEN].copy_from_slice(&settings.to_bytes());

        let slot = match self.first_blank_slot() {
            Some(slot) => slot,
            None => {
                self.flash.erase_sector(self.base)?;
                0
            }
        };

        let offset = self.slot_offset(slot);
        self.flash.program_page(offset, &page)?;

        // La ROM ne dit pas si la cellule a pris. On relit : un secteur en
        // fin de vie accepte l'ordre d'écriture sans changer d'état, et on
        // préfère le savoir maintenant que sous forme de réglages disparus
        // au prochain démarrage.
        let mut back = [0u8; PAGE_SIZE];
        self.flash.read(offset, &mut back);
        if back != page {
            return Err(StoreError::Verify);
        }

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::units::Celsius;

    /// Flash simulée : un secteur en RAM, avec la sémantique qui compte —
    /// programmer ne fait que descendre des bits, seul l'effacement remonte.
    struct MockFlash {
        data: [u8; SECTOR_SIZE],
        erases: usize,
        /// Nombre de programmations avant de refuser la suivante.
        fail_after: Option<usize>,
        programs: usize,
        /// Simule une cellule morte : le bit de poids fort du premier octet
        /// programmé ne descend jamais.
        stuck_bit: bool,
    }

    impl MockFlash {
        fn new() -> Self {
            Self {
                data: [0xFF; SECTOR_SIZE],
                erases: 0,
                fail_after: None,
                programs: 0,
                stuck_bit: false,
            }
        }
    }

    impl FlashOps for MockFlash {
        fn read(&self, offset: u32, buf: &mut [u8]) {
            let start = offset as usize;
            buf.copy_from_slice(&self.data[start..start + buf.len()]);
        }

        fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError> {
            assert_eq!(offset as usize % SECTOR_SIZE, 0);
            self.data = [0xFF; SECTOR_SIZE];
            self.erases += 1;
            Ok(())
        }

        fn program_page(&mut self, offset: u32, page: &[u8; PAGE_SIZE]) -> Result<(), StoreError> {
            assert_eq!(offset as usize % PAGE_SIZE, 0);
            if let Some(limit) = self.fail_after {
                if self.programs >= limit {
                    return Err(StoreError::Write);
                }
            }
            let start = offset as usize;
            for (cell, &wanted) in self.data[start..start + PAGE_SIZE].iter_mut().zip(page) {
                // Programmer ne peut que mettre des bits à 0.
                *cell &= wanted;
            }
            if self.stuck_bit {
                self.data[start] |= 0x80;
            }
            self.programs += 1;
            Ok(())
        }
    }

    fn store() -> FlashSettingsStore<MockFlash> {
        FlashSettingsStore::new(MockFlash::new(), 0)
    }

    #[test]
    fn blank_flash_has_nothing_to_load() {
        assert!(store().load().is_none());
    }

    #[test]
    fn what_was_saved_comes_back() {
        let mut s = store();
        let mut settings = Settings::defaults();
        settings.chamber_target = Celsius(-38.5);
        settings.ipa_heater_target = Celsius(42.0);

        s.save(&settings).unwrap();
        assert_eq!(s.load(), Some(settings));
    }

    #[test]
    fn the_newest_record_wins() {
        let mut s = store();
        for i in 0..5 {
            let mut settings = Settings::defaults();
            settings.chamber_target = Celsius(-30.0 - i as f32);
            s.save(&settings).unwrap();
        }
        assert_eq!(s.load().unwrap().chamber_target, Celsius(-34.0));
    }

    #[test]
    fn the_sector_is_erased_only_once_it_is_full() {
        let mut s = store();
        let settings = Settings::defaults();

        for _ in 0..SLOTS {
            s.save(&settings).unwrap();
        }
        assert_eq!(s.flash.erases, 0, "seize sauvegardes tiennent sans effacer");

        s.save(&settings).unwrap();
        assert_eq!(s.flash.erases, 1);
        assert_eq!(s.load(), Some(settings), "et on relit toujours quelque chose");
    }

    #[test]
    fn a_torn_write_falls_back_to_the_previous_record() {
        let mut s = store();
        let good = Settings::defaults();
        s.save(&good).unwrap();

        let mut half_written = Settings::defaults();
        half_written.chamber_target = Celsius(-12.0);
        s.save(&half_written).unwrap();
        // Coupure de courant juste après l'en-tête : tout ce qui suit est
        // resté à 0xFF. Le magic est là, donc l'emplacement compte comme
        // occupé, mais les consignes s'y relisent en NaN et
        // `Settings::from_bytes` refuse.
        s.flash.data[PAGE_SIZE + 8..PAGE_SIZE + RECORD_LEN].fill(0xFF);

        assert_eq!(s.load(), Some(good));
    }

    #[test]
    fn a_write_failure_is_reported() {
        let mut s = FlashSettingsStore::new(
            MockFlash { fail_after: Some(0), ..MockFlash::new() },
            0,
        );
        assert_eq!(s.save(&Settings::defaults()), Err(StoreError::Write));
    }

    #[test]
    fn flash_that_does_not_take_is_caught_by_the_readback() {
        let mut s =
            FlashSettingsStore::new(MockFlash { stuck_bit: true, ..MockFlash::new() }, 0);
        assert_eq!(s.save(&Settings::defaults()), Err(StoreError::Verify));
    }
}
