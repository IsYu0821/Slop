.text
.global slop_mnu_kernel_entry
slop_mnu_kernel_entry:
    /* Microsoft x64 caller enters with RSP % 16 == 8. kmain_bootinfo is SysV64,
       so align RSP to 16 before CALL and pass BootInfo in RDI. */
    mov g_boot_info(%rip), %rdi
    sub $8, %rsp
    call kmain_bootinfo
1:  cli
    hlt
    jmp 1b
