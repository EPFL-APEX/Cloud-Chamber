/*
* SPDX-License-Identifier: MIT OR Apache-2.0
*
* Copyright (c) 2021–2024 The rp-rs Developers
* Copyright (c) 2021 rp-rs organization
* Copyright (c) 2025 Raspberry Pi Ltd.
*/

MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    /*
     * Here we assume you have 2048 KiB of Flash. This is what the Pi Pico
     * has, but your board may have more or less Flash and you should adjust
     * this value to suit.
     */
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100 - 4K
    /*
     * Dernier secteur de la flash, réservé aux réglages persistants
     * (`drivers::flash_store`). Aucune section n'y atterrit : c'est ce qui
     * garantit qu'un effacement n'emportera jamais du code, même si le
     * binaire grossit.
     *
     * 4 Ko = un secteur d'effacement W25Q, la plus petite chose effaçable.
     * Si vous changez le 2048K ci-dessus, changez-le ici aussi.
     */
    SETTINGS : ORIGIN = 0x10000000 + 2048K - 4K, LENGTH = 4K
    /*
     * RAM consists of 4 banks, SRAM0-SRAM3, with a striped mapping.
     * This is usually good for performance, as it distributes load on
     * those banks evenly.
     */
    RAM : ORIGIN = 0x20000000, LENGTH = 256K
    /*
     * RAM banks 4 and 5 use a direct mapping. They can be used to have
     * memory areas dedicated for some specific job, improving predictability
     * of access times.
     * Example: Separate stacks for core0 and core1.
     */
    SRAM4 : ORIGIN = 0x20040000, LENGTH = 4k
    SRAM5 : ORIGIN = 0x20041000, LENGTH = 4k

    /* SRAM banks 0-3 can also be accessed directly. However, those ranges
       alias with the RAM mapping, above. So don't use them at the same time!
    SRAM0 : ORIGIN = 0x21000000, LENGTH = 64k
    SRAM1 : ORIGIN = 0x21010000, LENGTH = 64k
    SRAM2 : ORIGIN = 0x21020000, LENGTH = 64k
    SRAM3 : ORIGIN = 0x21030000, LENGTH = 64k
    */
}

EXTERN(BOOT2_FIRMWARE)

/*
 * Position du secteur des réglages, comptée depuis le début de la flash —
 * c'est ce qu'attendent les routines ROM, qui ne connaissent pas la fenêtre
 * XIP. Lu côté Rust par `drivers::flash_rp2040::settings_offset`.
 */
__settings_flash_offset = ORIGIN(SETTINGS) - ORIGIN(BOOT2);
__settings_flash_len = LENGTH(SETTINGS);

SECTIONS {
    /* ### Boot loader
     *
     * An executable block of code which sets up the QSPI interface for
     * 'Execute-In-Place' (or XIP) mode. Also sends chip-specific commands to
     * the external flash chip.
     *
     * Must go at the start of external flash, where the Boot ROM expects it.
     */
    .boot2 ORIGIN(BOOT2) :
    {
        KEEP(*(.boot2));
    } > BOOT2
} INSERT BEFORE .text;

SECTIONS {
    /* ### Boot ROM info
     *
     * Goes after .vector_table, to keep it in the first 512 bytes of flash,
     * where picotool can find it
     */
    .boot_info : ALIGN(4)
    {
        KEEP(*(.boot_info));
    } > FLASH

} INSERT AFTER .vector_table;

/* move .text to start /after/ the boot info */
_stext = ADDR(.boot_info) + SIZEOF(.boot_info);

SECTIONS {
    /* ### Picotool 'Binary Info' Entries
     *
     * Picotool looks through this block (as we have pointers to it in our
     * header) to find interesting information.
     */
    .bi_entries : ALIGN(4)
    {
        /* We put this in the header */
        __bi_entries_start = .;
        /* Here are the entries */
        KEEP(*(.bi_entries));
        /* Keep this block a nice round size */
        . = ALIGN(4);
        /* We put this in the header */
        __bi_entries_end = .;
    } > FLASH
} INSERT AFTER .text;
