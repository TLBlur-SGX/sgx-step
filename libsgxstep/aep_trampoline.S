/*
 *  This file is part of the SGX-Step enclave execution control framework.
 *
 *  Copyright (C) 2017 Jo Van Bulck <jo.vanbulck@cs.kuleuven.be>,
 *                     Raoul Strackx <raoul.strackx@cs.kuleuven.be>
 *
 *  SGX-Step is free software: you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation, either version 3 of the License, or
 *  (at your option) any later version.
 *
 *  SGX-Step is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with SGX-Step. If not, see <http://www.gnu.org/licenses/>.
 */

/* 32-bit compatibility ~~following Intel SGX-SDK convention. */
#if defined(M32)
    #define xax  eax
    #define xbx  ebx
    #define xcx  ecx
    #define xdi  edi
    #define movx movl
#else
    #define xax  rax
    #define xbx  rbx
    #define xcx  rcx
    #define xdi  rdi
    #define movx movq
#endif

/* 
 * ---------------------------------------------------------------------
 * Function: sgx_step_trampoline
 *      Code that will be returned to after interrupting an enclave.
 *
 * Registers:
 *      XAX - ERESUME leaf
 *      XBX - TSC address
 *      XCX - AEP address
 */
    .text
    .global sgx_step_aep_trampoline
    .type sgx_step_aep_trampoline, @function
sgx_step_aep_trampoline:
    /*
     * Save TCS address thread-local on the stack
     * NOTE: maintain 16-byte stack alignment (ABI calling convention)
     */
    push %xbx
    mov %rsp, %rbp
    and $~0xf, %rsp

    /* optional C function callback */
    mov sgx_step_aep_cb@GOTPCREL(%rip), %xax
    mov (%xax), %xax
    test %xax, %xax
    je .Leresume
    call *%xax

.Leresume:
    mov sgx_step_eresume_cnt@GOTPCREL(%rip), %rax
    incl (%rax)

    /* restore stack and TCS address */
    mov %rbp, %rsp
    pop %xbx

    mov sgx_step_aep_trampoline@GOTPCREL(%rip), %xcx   /* AEP address */

    mov $3, %xax                        /* ERESUME leaf */

.global sgx_step_aep_eresume
sgx_step_aep_eresume:
    .byte 0x0f, 0x01, 0xd7              /* ENCLU */
    ud2
