MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 256K
}

/* Place les 256 octets du second-stage bootloader à 0x10000000.
 * Ce bloc est inclus par link.x (via INCLUDE memory.x) au moment où
 * le bloc MEMORY est déjà défini, donc ORIGIN(BOOT2) est résolu. */
SECTIONS {
    .boot2 ORIGIN(BOOT2) :
    {
        KEEP(*(.boot2));
    } > BOOT2
} INSERT BEFORE .text;
