//! File d'attente des événements encodeur, entre l'interruption qui scrute
//! et la boucle qui dessine.
//!
//! # Le problème qu'elle résout
//!
//! L'encodeur est scruté à 1 ms par `TIMER_IRQ_0`, parce qu'un cran dure
//! moins qu'un rendu : scruter depuis la boucle principale en perdait — pas
//! en retardait, en perdait, deux rotations rapprochées ne comptant que pour
//! une.
//!
//! Reste à décider ce que l'interruption fait de l'événement. La réponse
//! naïve — appliquer la navigation tout de suite — oblige à partager
//! `UiApp` avec l'ISR, donc à le mettre derrière un `Mutex`, donc à prendre
//! une section critique **autour du dessin**. Et une section critique, c'est
//! exactement ce qui empêche l'interruption de tourner : on retombe sur le
//! bug d'origine par l'autre bout, avec en prime, sur la machine réelle, le
//! cœur 1 bloqué sur le spinlock pendant des dizaines de millisecondes.
//!
//! Avec cette file, l'ISR ne fait qu'empiler — quelques instructions, une
//! section critique de longueur bornée — et `UiApp` n'est partagé avec
//! personne. La boucle dépile, applique, copie l'état, et dessine **sans
//! aucun verrou**.
//!
//! # Débordement
//!
//! Sur file pleine, le nouvel événement est abandonné plutôt que d'écraser
//! le plus ancien : réordonner serait pire que perdre, un clic ne doit
//! jamais doubler une rotation qui l'a précédé. Le débordement est compté,
//! pour que l'appelant puisse le journaliser — perdre un cran doit se voir.

use crate::drivers::encoder::EncoderEvent;

/// Profondeur de la file.
///
/// À 1 ms de scrutation et un rendu de quelques dizaines de ms, une
/// rotation même rapide en produit une poignée entre deux passages de la
/// boucle. 32 places laissent de la marge sans peser (un `EncoderEvent`
/// tient dans un octet).
pub const EVENT_QUEUE_LEN: usize = 32;

/// File circulaire à taille fixe, remplie par l'interruption et vidée par
/// la boucle principale. Cf. la documentation de module.
pub struct EventQueue {
    buffer: [EncoderEvent; EVENT_QUEUE_LEN],
    head: usize,
    len: usize,
    dropped: u32,
}

impl EventQueue {
    /// `const` : c'est ce qui permet de la déclarer en `static` sans
    /// initialisation paresseuse.
    pub const fn new() -> Self {
        Self { buffer: [EncoderEvent::None; EVENT_QUEUE_LEN], head: 0, len: 0, dropped: 0 }
    }

    /// Empile un événement, ou compte un débordement si la file est pleine.
    pub fn push(&mut self, event: EncoderEvent) {
        if self.len == EVENT_QUEUE_LEN {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let tail = (self.head + self.len) % EVENT_QUEUE_LEN;
        self.buffer[tail] = event;
        self.len += 1;
    }

    /// Dépile le plus ancien événement.
    pub fn pop(&mut self) -> Option<EncoderEvent> {
        if self.len == 0 {
            return None;
        }
        let event = self.buffer[self.head];
        self.head = (self.head + 1) % EVENT_QUEUE_LEN;
        self.len -= 1;
        Some(event)
    }

    /// Relève le compteur de débordements et le remet à zéro.
    pub fn take_dropped(&mut self) -> u32 {
        core::mem::take(&mut self.dropped)
    }
}

impl Default for EventQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const CW: EncoderEvent = EncoderEvent::RotateClockwise;
    const CCW: EncoderEvent = EncoderEvent::RotateCounterClockwise;
    const CLICK: EncoderEvent = EncoderEvent::ButtonPressed;

    #[test]
    fn an_empty_queue_yields_nothing() {
        let mut q = EventQueue::new();
        assert_eq!(q.pop(), None);
        assert_eq!(q.take_dropped(), 0);
    }

    /// L'ordre est ce qui compte : un clic derrière deux rotations doit
    /// ressortir derrière ces deux rotations.
    #[test]
    fn events_come_back_in_order() {
        let mut q = EventQueue::new();
        for event in [CW, CW, CCW, CLICK] {
            q.push(event);
        }
        assert_eq!(q.pop(), Some(CW));
        assert_eq!(q.pop(), Some(CW));
        assert_eq!(q.pop(), Some(CCW));
        assert_eq!(q.pop(), Some(CLICK));
        assert_eq!(q.pop(), None);
    }

    /// Le cas normal : la boucle vide la file, l'ISR la remplit, et
    /// l'enroulement des indices ne doit rien changer à l'ordre.
    #[test]
    fn indices_wrap_without_reordering() {
        let mut q = EventQueue::new();
        for round in 0..10 {
            for _ in 0..EVENT_QUEUE_LEN - 1 {
                q.push(CW);
            }
            q.push(CLICK);
            for _ in 0..EVENT_QUEUE_LEN - 1 {
                assert_eq!(q.pop(), Some(CW), "tour {round}");
            }
            assert_eq!(q.pop(), Some(CLICK), "tour {round}");
        }
        assert_eq!(q.take_dropped(), 0, "aucun debordement : la file etait videe a chaque tour");
    }

    /// Sur file pleine c'est le **nouvel** événement qui tombe, pas
    /// l'ancien : ce qui est déjà accepté garde son rang.
    #[test]
    fn a_full_queue_drops_the_newcomer_not_the_backlog() {
        let mut q = EventQueue::new();
        for _ in 0..EVENT_QUEUE_LEN {
            q.push(CW);
        }
        q.push(CLICK);
        q.push(CLICK);

        assert_eq!(q.take_dropped(), 2);
        for _ in 0..EVENT_QUEUE_LEN {
            assert_eq!(q.pop(), Some(CW), "le clic perdu ne doit pas avoir double une rotation");
        }
        assert_eq!(q.pop(), None);
    }

    /// Le compteur se relève une fois : deux relevés d'affilée ne doivent
    /// pas journaliser deux fois la même perte.
    #[test]
    fn taking_the_drop_count_clears_it() {
        let mut q = EventQueue::new();
        for _ in 0..EVENT_QUEUE_LEN + 3 {
            q.push(CW);
        }
        assert_eq!(q.take_dropped(), 3);
        assert_eq!(q.take_dropped(), 0);
    }

    /// Une file saturée puis vidée redevient utilisable — le débordement
    /// n'est pas un état bloquant.
    #[test]
    fn the_queue_recovers_after_an_overflow() {
        let mut q = EventQueue::new();
        for _ in 0..EVENT_QUEUE_LEN + 5 {
            q.push(CW);
        }
        while q.pop().is_some() {}
        q.take_dropped();

        q.push(CLICK);
        assert_eq!(q.pop(), Some(CLICK));
        assert_eq!(q.take_dropped(), 0);
    }
}
