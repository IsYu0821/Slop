#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
python3 - <<'PY'
from pathlib import Path
r=Path('.')
required={
 'loader/entry.S':['sub $8, %rsp','call kmain_bootinfo'],
 'arch/x86_64/syscall.S':['push %r11','push %r15','mov 48(%rsp), %r8','push 56(%rsp)','call syscall_dispatch_raw','EXEC_PENDING','EXEC_RIP','sysretq'],
 'arch/x86_64/exceptions.S':['isr_invalid_tss','isr_stack_segment','isr_alignment_check','call exception_rust'],
 'kernel/lib.rs':['spawn_user(loaded)','wait_child(user_pid)','scheduler: user process reaped'],
 'kernel/usercopy.rs':['root.query','PRESENT|USER|WRITABLE'],
 'kernel/process_runtime.rs':['bootstrap.activate()','destroy_private_slot','free_frame(s.process_cr3)','qemu::exit'],
 'tools/acceptance-test.sh':['expected 33','EFAULT passed','acceptance: PASS'],
 'kernel/console.rs':['THR_EMPTY','TX_EMPTY','pub fn flush()'],
 'kernel/arch/x86_64/cpu.rs':['CPU lacks SYSCALL/SYSRET','CPU lacks NX','cr0|=1<<16'],
 'kernel/acpi.rs':['RSDP missing','MADT missing','IOAPIC missing'],
 'kernel/arch/x86_64/apic.rs':['TIMER_VECTOR','write(0xb0,0)','1<<17'],
 'arch/x86_64/irq.S':['apic_timer_entry','call apic_timer_rust','iretq'],
 'kernel/sync.rs':['IrqSpinLock','irq_save','irq_restore'],
 'kernel/task.rs':['struct Scheduler','spawn_kernel','yield_now','scheduler_self_test','wake_sleepers','apply_arch_state','IA32_FS_BASE'],
 'kernel/arch/x86_64/msr.rs':['IA32_FS_BASE','IA32_KERNEL_GS_BASE'],
 'kernel/arch/x86_64/descriptors.rs':['set_rsp0'],
 'kernel/arch/x86_64/syscall.rs':['set_kernel_stack'],
 'kernel/wait_queue.rs':['struct WaitQueue','wake_one','wake_all'],
 'kernel/vfs.rs':['mount_initramfs','pub fn lookup','pub fn image'],
 'kernel/fd.rs':['pub fn open','pub fn read','pub fn seek','pub fn close'],
 'tools/build-initramfs.py':['NSAR0001','/init','/etc/motd'],
 'kernel/process.rs':['map_zeroed','map_page','unmap_page','protect_page','BRK_BASE'],
 'tests/minimal-linux.S':['mov $59,%rax','/bin/hello','argv','envp'],
 'tests/hello-linux.S':['mov $9,%rax','mov $10,%rax','mov $11,%rax','mov $12,%rax','cmpq $1,(%rsp)'],
 'kernel/linux_abi/mod.rs':['sys_execve','copy_exec_vec','load_static_args','commit_exec_return'],
 'kernel/process_runtime.rs':['replace_image','bootstrap_cr3'],
 'arch/x86_64/context.S':['switch_context','mov %rsp,(%rdi)','mov %rsi,%rsp'],
}
for name,tokens in required.items():
    text=(r/name).read_text()
    for token in tokens:
        assert token in text,(name,token)
assert (r/'Cargo.toml').read_text().count('version = "0.25.2"')==1
print('source consistency checks passed')
PY
