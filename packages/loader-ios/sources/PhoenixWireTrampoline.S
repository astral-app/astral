    .text
    .p2align 2
    .globl _astral_wire_trampoline
_astral_wire_trampoline:
    stp x29, x30, [sp, #-32]!
    mov x29, sp
    stp x19, x20, [sp, #16]
    mov x2, x20
    bl _astral_wire_intercept
    ldr x20, [sp, #24]
    adrp x16, _astral_wire_original@PAGE
    ldr x16, [x16, _astral_wire_original@PAGEOFF]
    blr x16
    stp x0, x1, [sp, #-16]!
    mov x19, x21
    bl _astral_wire_unlock
    mov x21, x19
    ldp x0, x1, [sp], #16
    ldp x19, x20, [sp, #16]
    ldp x29, x30, [sp], #32
    ret

    .p2align 2
    .globl _astral_wire_call
_astral_wire_call:
    stp x29, x30, [sp, #-32]!
    mov x29, sp
    stp x20, x21, [sp, #16]
    mov x20, x2
    mov x21, xzr
    blr x3
    ldp x20, x21, [sp, #16]
    ldp x29, x30, [sp], #32
    ret
