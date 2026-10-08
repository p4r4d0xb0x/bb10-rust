    .syntax unified
    .arm
    .text
    .globl _start
    .type _start, %function
_start:
    bl main
    bl exit
    .size _start, .-_start
