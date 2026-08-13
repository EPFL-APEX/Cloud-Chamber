; PWM synchronisé sur le réseau pour éviter d'emettre des ondes électromagnétiques.
; Implémenté pour qu'il fonctionne sur les states machines du PIO de RP2040 ou RP235X du Pico
; Attention, si on veut 10 cycles, il faut mettre 9 dans y car le cycle 0 compte, pareil
; pour le duty dans x, si on veut que ce soit activé durant 7 cycles, il faut renter 6...

.program zero_cross
.wrap_target
    set pins, 0
    pull noblock
    mov isr, osr
    out x, 16
    out y, 16
inner_loop:
    wait 1 pin, 0
    wait 0 pin, 0  ; évite qu'on compte plusieurs fois le même 0
    jmp x != y no_set
    set pins, 1
no_set:
    jmp y-- inner_loop
    mov x, isr
.wrap

