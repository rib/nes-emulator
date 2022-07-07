
; NSF Playback Bios
; Assemble with ca65 tool from https://github.com/rib/asm6502

        jsr $501e       ; $5000: call PLAY <-- needs relocation (initially calls a place-holder RTS)
loop:
        jmp $5003       ; $5003: JMP to self (TODO: support use of label here)

interrupt:
        pha             ; $5006: Save A
        txa             ; $5007:
        pha             ; $5008: Save X
        tya             ; $5009:
        pha             ; $500a: Save Y
        lda #$00        ; $500b:
        sta $2000       ; $500d: disable NMI
        jsr $501e       ; $5010: call PLAY < -- needs relocation
        lda #$80        ; $5013:
        sta $2000       ; $5015: enable NMI
        pla             ; $5018:
        tay             ; $5019: restore Y
        pla             ; $501a:
        tax             ; $501b: restore X
        pla             ; $501c: restore A
        rti             ; $501d:

noop:
        rts             ; $501e:

