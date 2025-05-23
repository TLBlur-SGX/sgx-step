/*
 * NOTE: code/data in the dedicated `isr_section` will be mapped by libsgxstep
 * into the global kernel address range. This ensures that the handler is always
 * available across CR3 switches (as the APIC timer may sometimes fire after
 * the libsgxstep process has been context switched).
 *
 * Important considerations when programming APIC interrupt handlers:
 *
 *      1. All code and global data referenced in the handler should be placed
 *         in the special `isr_section`.
 *      2. All code should be written position-independent.
 *
 */
/* ********************************************************************** */
#include "config.h"

    .section isr_section,"awx",@progbits
    .align 0x1000
    .global __ss_irq_fired, __ss_irq_count, __ss_irq_cpl, nemesis_tsc_aex, __ss_irq_rip
__ss_irq_fired:
    .int 0x0
__ss_irq_count:
    .int 0x0
__ss_irq_cpl:
    .int 0xff
#if !X2APIC
    .global apic_base
apic_base:
    .quad 0x0
#endif
nemesis_tsc_aex:
    .quad 0x0;
    /* not sure there's a kernel stack we can use(?) */
__ss_irq_rax:
    .quad 0x0
__ss_irq_rdx:
    .quad 0x0
__ss_irq_rcx:
    .quad 0x0
__ss_irq_rip:
    .quad 0x0

/* ********************************************************************** */
    .section isr_section,"awx",@progbits
    .align 0x1000
    .global __ss_irq_handler
__ss_irq_handler:
    /* Nemesis IRQ latency timestamp */
    push %rax
    push %rcx
    push %rdx
    rdtsc
    mov nemesis_tsc_aex@GOTPCREL(%rip), %rcx
    mov %eax, (%rcx)
    mov %edx, 4(%rcx)

    /* IRQ bookkeeping */
    mov 12(%rsp), %rax
    mov __ss_irq_rip@GOTPCREL(%rip), %rcx
    mov %rax, (%rcx)
    mov __ss_irq_cpl@GOTPCREL(%rip), %rcx
    movw %cs,  (%rcx)
    andw $0x3, (%rcx)
    mov __ss_irq_count@GOTPCREL(%rip), %rcx
    incl (%rcx)
    mov __ss_irq_fired@GOTPCREL(%rip), %rcx
    incl (%rcx)

    /* apic_write(APIC_EOI, 0x0); */
#if !X2APIC
    mov apic_base@GOTPCREL(%rip), %rax
    mov (%rax),%rax
    test %rax, %rax
    jz 1f
    add $0xb0, %rax
    movl $0x0, (%rax)
1:
#else
    xor %eax,%eax
    xor %edx,%edx
    mov $0x80b, %ecx
    wrmsr
#endif
    pop %rdx
    pop %rcx
    pop %rax
    iretq

/* ********************************************************************** */
/*
 * NOTE: The following handler is only ever supposed to be triggered from
 * within the libsgxstep process (CR3) by means of a dedicated software
 * interrupt `int` instruction. Hence, we don't have to map it into the global
 * kernel virtual address range.
 */
    .data
    .align 0x1000
    .global __ss_irq_gate_cb
__ss_irq_gate_cb:
    .quad 0x0

    .text
    .align 0x1000
    .global __ss_irq_gate
__ss_irq_gate:
    mov __ss_irq_gate_cb@GOTPCREL(%rip), %rax
    call *(%rax)
    iretq

    /*
     * void trigger_sw_irq(void);
     *
     * \note            Separate asm function to make sure caller-save
     *                  registers are properly stored and restored.
     */
    .global trigger_sw_irq
trigger_sw_irq:
    int $IRQ_PRIV_VECTOR
    retq

    /*
     * void wrmsr(uint32_t reg, uint64_t val);
     *
     * \arg rdi         MSR address (32-bit)
     * \arg rsi         value (64-bit)
     *
     * \note            clobbers RDI, RSI, RAX, RCX, RDX (all caller-save)
     */
    .global wrmsr
wrmsr:
    int $WRMSR_GATE_VECTOR
    retq

    .global __wrmsr_gate
__wrmsr_gate:
    mov %edi, %ecx
    mov %esi, %eax
    shr $32, %rsi
    mov %esi, %edx
    wrmsr
    iretq

    /*
     * uint64_t rdmsr(uint32_t reg);
     *
     * \arg rdi         MSR address (32-bit)
     * \ret rax         value (64-bit)
     *
     * \note            clobbers RDI, RAX, RCX, RDX (all caller-save)
     */
    .global rdmsr
rdmsr:
    int $RDMSR_GATE_VECTOR
    retq

    .global __rdmsr_gate
__rdmsr_gate:
    mov %edi, %ecx
    rdmsr
    shl $32, %rdx
    or  %rdx, %rax
    iretq
