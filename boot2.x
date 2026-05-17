/* Linker script pour la section .boot2 du RP2040.
 * Place les 256 octets du second-stage bootloader à 0x10000000
 * (adresse lue par le ROM bootloader au démarrage).
 */
SECTIONS {
  /* Place les 256 octets du boot2 à l'adresse fixe 0x10000000 —
   * adresse lue par le ROM bootloader du RP2040 au démarrage.
   * On n'utilise pas ORIGIN(BOOT2) car memory.x n'est pas encore chargé
   * à ce stade (il est inclus par link.x qui vient après). */
  .boot2 0x10000000 :
  {
    KEEP(*(.boot2));
  }
} INSERT BEFORE .text;
