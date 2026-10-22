/* The vector system call entry point.
 *
 * Linux gives every program a page of code that it can call without
 * entering the kernel, so a program that reads the clock does not pay for
 * a system call. The page holds the usual entry points, and the time comes
 * from a shared page that the kernel keeps up to date.
 *
 * The shared page holds the rate of the time stamp counter, the value of
 * the counter at the last update of the page, and the monotonic and the
 * wall clock in nanoseconds at that moment. A program reads the page under
 * a sequence lock, so it either sees a complete update or the update that
 * follows it.
 *
 *   offset 0:  the sequence of the update
 *   offset 8:  the nanoseconds per counter cycle, with 32 fractional bits
 *   offset 16: the counter value at the last update
 *   offset 24: the monotonic nanoseconds at the last update
 *   offset 32: the wall clock nanoseconds at the last update
 */

/* The address of the shared page. The kernel maps it at the same address
   in every process. */
	.set VVAR, 0x610000000000
	.set V_SEQ, 0
	.set V_SCALE, 8
	.set V_TSC, 16
	.set V_MONO, 24
	.set V_REAL, 32

	.text
	.globl __vdso_clock_gettime
	.type __vdso_clock_gettime,@function
	.globl __vdso_gettimeofday
	.type __vdso_gettimeofday,@function
	.globl __vdso_time
	.type __vdso_time,@function
	.globl __vdso_getcpu
	.type __vdso_getcpu,@function

/* Fills the timespec at rsi with the time of the clock named by rdi.
   The clocks that count the time a process spends on the processor are not
   known here, so they report that they are not available and the caller
   falls back to the system call. */
__vdso_clock_gettime:
	test %rdi, %rdi
	jz .Lwall
	cmp $1, %rdi
	je .Lmonotonic
	cmp $7, %rdi
	je .Lmonotonic
	mov $-38, %rax
	ret
.Lwall:
	mov $V_REAL, %ecx
	jmp .Lclock
.Lmonotonic:
	mov $V_MONO, %ecx
.Lclock:
	push %rbx
	push %r12
	push %r13
	push %r14
	push %r15
	movabs $VVAR, %r15
	call .Lnow
	/* rax holds the nanoseconds. */
	mov $1000000000, %r8
	xor %edx, %edx
	div %r8
	mov %rax, (%rsi)
	mov %rdx, %r10
	mov %r10, 8(%rsi)
	xor %eax, %eax
	pop %r15
	pop %r14
	pop %r13
	pop %r12
	pop %rbx
	ret

/* Fills the timeval at rdi with the wall clock. */
__vdso_gettimeofday:
	test %rdi, %rdi
	jz .Lzero
	push %rbx
	push %r12
	push %r13
	push %r14
	push %r15
	mov $V_REAL, %ecx
	movabs $VVAR, %r15
	call .Lnow
	mov $1000000, %r8
	xor %edx, %edx
	div %r8
	mov %rax, (%rdi)
	mov %rdx, %r10
	mov %r10, 8(%rdi)
	xor %eax, %eax
	pop %r15
	pop %r14
	pop %r13
	pop %r12
	pop %rbx
	ret
.Lzero:
	xor %eax, %eax
	ret

/* Returns the wall clock in seconds and stores it at rdi when that is
   given. */
__vdso_time:
	push %rbx
	push %r12
	push %r13
	push %r14
	push %r15
	mov $V_REAL, %ecx
	movabs $VVAR, %r15
	call .Lnow
	mov $1000000000, %r8
	xor %edx, %edx
	div %r8
	test %rdi, %rdi
	jz .Ltime_done
	mov %rax, (%rdi)
.Ltime_done:
	pop %r15
	pop %r14
	pop %r13
	pop %r12
	pop %rbx
	ret

/* Reports the processor the program runs on. The kernel has one
   processor, so the answer is always the first one. */
__vdso_getcpu:
	test %rsi, %rsi
	jz .Lcpu_done
	movl $0, (%rsi)
.Lcpu_done:
	test %rdx, %rdx
	jz .Lcpu_none
	movl $0, (%rdx)
.Lcpu_none:
	xor %eax, %eax
	ret

/* Reads the shared page under the sequence lock and returns the time in
   nanoseconds in rax. The offset of the base time is in rcx. */
.Lnow:
.Lretry:
	mov V_SEQ(%r15), %r8
	test $1, %r8
	jnz .Lretry
	mov V_SCALE(%r15), %r13
	mov V_TSC(%r15), %rbx
	mov (%r15,%rcx), %r12
	lfence
	rdtsc
	mov %rax, %r9
	lfence
	mov V_SEQ(%r15), %rax
	cmp %r8, %rax
	jne .Lretry
	/* The nanoseconds since the last update of the page. The scale holds
	   the nanoseconds per cycle with 32 fractional bits, so the product is
	   shifted down to drop the fraction. */
	sub %rbx, %r9
	mov %r9, %rax
	mulq %r13
	shl $32, %rdx
	shr $32, %rax
	or %rax, %rdx
	add %r12, %rdx
	mov %rdx, %rax
	ret

	.section .note.GNU-stack,"",@progbits
