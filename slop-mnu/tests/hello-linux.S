.global _start
.section .text
_start:
    cmpq $1,(%rsp); jne failure
    mov 8(%rsp),%rax; cmpl $0x6c6c6568,(%rax); jne failure
    mov 24(%rsp),%rax; test %rax,%rax; je failure
    cmpl $0x504f4c53,(%rax); jne failure
    mov $1,%rax; mov $1,%rdi; mov $0x1234,%rsi; mov $8,%rdx; syscall
    cmp $-14,%rax; jne failure
    mov $257,%rax; mov $-100,%rdi; lea motd_path(%rip),%rsi; xor %rdx,%rdx; xor %r10,%r10; syscall
    cmp $3,%rax; jl failure
    mov %rax,%r12
    xor %rax,%rax; mov %r12,%rdi; lea buffer(%rip),%rsi; mov $64,%rdx; syscall
    cmp $0,%rax; jle failure
    mov %rax,%rdx; mov $1,%rax; mov $1,%rdi; lea buffer(%rip),%rsi; syscall
    cmp %rdx,%rax; jne failure
    mov $3,%rax; mov %r12,%rdi; syscall
    test %rax,%rax; jne failure
    /* Anonymous mmap must install zeroed, writable PTEs. */
    mov $9,%rax; xor %rdi,%rdi; mov $4096,%rsi; mov $3,%rdx; mov $0x22,%r10; mov $-1,%r8; xor %r9,%r9; syscall
    test %rax,%rax; js failure
    mov %rax,%r13
    cmpq $0,(%r13); jne failure
    movq $0x11223344,(%r13)
    cmpq $0x11223344,(%r13); jne failure
    mov $10,%rax; mov %r13,%rdi; mov $4096,%rsi; mov $1,%rdx; syscall
    test %rax,%rax; jne failure
    mov $11,%rax; mov %r13,%rdi; mov $4096,%rsi; syscall
    test %rax,%rax; jne failure
    /* brk growth must map zero-filled heap pages. */
    mov $12,%rax; xor %rdi,%rdi; syscall
    mov %rax,%r14
    lea 4096(%r14),%rdi; mov $12,%rax; syscall
    lea 4096(%r14),%rcx; cmp %rcx,%rax; jne failure
    cmpq $0,(%r14); jne failure
    movq $0x12345678,(%r14)
    cmpq $0x12345678,(%r14); jne failure
    /* clock_gettime(CLOCK_MONOTONIC) */
    mov $228,%rax; mov $1,%rdi; lea timespec(%rip),%rsi; syscall
    test %rax,%rax; jne failure
    /* getrandom must fill the buffer. */
    mov $318,%rax; lea random_buf(%rip),%rdi; mov $16,%rsi; xor %rdx,%rdx; syscall
    cmp $16,%rax; jne failure
    mov random_buf(%rip),%rax; or random_buf+8(%rip),%rax; je failure
    /* nanosleep for one bootstrap tick. */
    movq $0,sleep_req(%rip); movq $1000000,sleep_req+8(%rip)
    mov $35,%rax; lea sleep_req(%rip),%rdi; xor %rsi,%rsi; syscall
    test %rax,%rax; jne failure
    mov $1,%rax; mov $1,%rdi; lea message(%rip),%rsi; mov $message_end-message,%rdx; syscall
    cmp $message_end-message,%rax; jne failure
    mov $231,%rax; xor %rdi,%rdi; syscall
failure:
    mov $1,%rax; mov $2,%rdi; lea failed(%rip),%rsi; mov $failed_end-failed,%rdx; syscall
    mov $231,%rax; mov $1,%rdi; syscall
1:  jmp 1b
.section .rodata
motd_path: .asciz "/etc/motd"
message: .ascii "hello from execve /bin/hello; argv, envp, VFS, VM, time, random and EFAULT passed\n"
message_end:
failed: .ascii "Slop/MNU Linux ABI acceptance failed\n"
failed_end:
.section .bss
.align 16
buffer: .skip 64
timespec: .skip 16
sleep_req: .skip 16
random_buf: .skip 16
