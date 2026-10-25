# Roadmap next: from compatible to practical (MS82-MS99)

Status: MS81 completes priority #1 (the 32 KiB file cap is gone).
What remains impractical for real use: single-CPU execution, no
inbound/outbound Internet connectivity, an incomplete syscall/ABI
surface inventory, no ext4, and a minimal driver set. The milestones
below are ordered by the standing priority list:

1. 32 KiB limit removal — DONE in MS81.
2. SMP and multicore foundation.
3. External networking (DHCP, DNS, TCP).
4. Linux ABI inventory and gaps.
5. Staged ext4.
6. USB / input / storage / Ethernet drivers.

Each milestone states purpose, implementation, dependencies, new
tests, the differential method, completion criteria, known gaps and
the rollback plan. A milestone is review-sized: it must be possible
to tell from its gate whether it regressed anything. If a gate turns
red, the fix lands as a new milestone; broken trees are reverted,
not patched in place. Rollback for every item below is: keep the
previous subsystem intact behind its existing tests (acceptance,
differential, busybox, heap) and revert the milestone commit.

Quality bar for every milestone (no exceptions):

- same ELF runs on native Linux and in the guest; stdout, stderr,
  exit status and errno compared;
- on-disk bytes verified on the platter where storage is touched;
- fsck clean wherever a filesystem is touched;
- multi-CPU stress once SMP exists; long-run network tests once
  external networking exists; packet loss / disconnect / timeout
  cases for every new network path;
- no new warnings, `check-source` green, snapshot regenerated.

## Track SMP (priority 2)

### MS82 — CPU enumeration and parked APs
- Purpose: know how many CPUs exist and get them out of reset
  without disturbing the running system.
- Implement: walk ACPI MADT (or equivalent) for all enabled Local
  APIC IDs; allocate a per-CPU stack and state record; start APs
  through INIT/SIPI into a parked halt loop; report the count on
  the boot console.
- Depends on: existing ACPI/MADT discovery, APIC setup, frame
  allocator.
- New tests: boot log asserts the APIC-ID count for `-smp 1/2/4/8`;
  APs stay parked (no scheduler changes yet).
- Differential: host counts QEMU `-smp N` vs guest boot log.
- Completion: 2/4/8 vCPU boots show N parked APs and a working
  single-CPU system afterwards.
- Known gaps: no parallel execution yet; no IPI; no TLB shootdown.
- Rollback: single `smp` module; revert returns to UP boot.

### MS83 — SMP scheduler with IPIs
- Purpose: run user processes and kernel work on all CPUs.
- Implement: per-CPU run queues, cross-CPU wakeup/reschedule IPIs,
  per-CPU kernel stacks and TSS entries, spinlock IRQ-safety audit
  for state shared with IRQ handlers.
- Depends on: MS82.
- New tests: N forked CPU-burners with distinct results, cross-CPU
  wakeups, timer balance; run the full gate suite with `-smp 1/4`.
- Differential: parallel workloads print identical results natively
  and in the guest.
- Completion: 2/4/8 vCPU boots run parallel user tasks with correct
  results; UP behavior unchanged.
- Known gaps: TLB shootdown still missing (no cross-CPU unmap yet).
- Rollback: scheduler files; UP path preserved.

### MS84 — TLB shootdown, interrupt routing, memory ordering
- Purpose: make SMP correct, not just parallel.
- Implement: TLB shootdown IPIs on unmap/protect, IOAPIC routing
  review, acquire/release ordering at every lock and flag
  handoff, SEV/SNP-style page-state notes where applicable.
- Depends on: MS83.
- New tests: cross-CPU mmap/mprotect/munmap stress, interrupt
  distribution counters, a lock-torture battery with native
  comparison.
- Differential: stress outputs match native byte for byte.
- Completion: no stale-TLB faults under stress; race detector
  (best-effort lock-order assertions) quiet for a full gate run.
- Known gaps: no CPU hotplug yet (design must not preclude it:
  per-CPU state stays allocatable).
- Rollback: paging + apic files.

### MS85 — SMP hardening and race/deadlock audit
- Purpose: prove SMP stays up under adversarial load.
- Implement: lock-order documentation, watchdog for stuck APs,
  systematic audit of sleep/wakeup paths for lost wakeups.
- Depends on: MS84.
- New tests: 8-vCPU overnight-style stress (bounded for CI),
  fault-injection sleeps at yield points.
- Completion: full gate green on `-smp 8` three consecutive runs.
- Known gaps: CPU hotplug still future work.
- Rollback: audit-only plus targeted fixes.

## Track network (priority 3)

### MS86 — DHCP client, routing table, default gateway
- Purpose: the guest configures itself from the network instead of
  hardcoded addresses.
- Implement: DHCP discover/offer/request/ack state machine with
  retransmit, lease timers, routing table with longest-prefix
  match, default gateway plumbing through the existing stack.
- Depends on: UDP, timers, current Ethernet/ARP/IPv4.
- New tests: guest boots on QEMU user networking, obtains the
  expected address, installs the default route, renews the lease.
- Differential: compare obtained config and a UDP echo against
  native (a Linux VM on the same virtual network).
- Completion: `ip addr`/`ip route` equivalents show DHCP data; UDP
  to the outside works through the gateway.
- Known gaps: no DNS yet (numeric IPs only).
- Rollback: net.rs DHCP/routing files.

### MS87 — external TCP robustness
- Purpose: real connections to real hosts, with loss and timeouts.
- Implement: RTO estimation and retransmit, receive window updates,
  RST/timeout paths, connection teardown races, poll/epoll readiness
  integration for external sockets.
- Depends on: MS86.
- New tests: long-lived transfers, induced packet loss and delay
  (host-side `tc`), peer reset mid-stream, connect timeout.
- Differential: byte-identical transfers and identical errno
  sequences vs native under the same faults.
- Completion: a guest TCP client talks to a host server reliably;
  no stalls or silent data loss across the fault battery.
- Known gaps: no congestion control beyond go-back-N yet.
- Rollback: TCP state machine files.

### MS88 — HTTPS-capable socket surface
- Purpose: let existing Linux TLS libraries run in the guest
  (no kernel crypto).
- Implement: whatever socket surface a real TLS stack needs on top
  of TCP: complete non-blocking semantics, `MSG_PEEK`, common
  `setsockopt` (keepalive, nodelay, buffers), `TCP_INFO`-class
  introspection if cheap, `poll` edge cases.
- Depends on: MS87, in-tree libc socket wrappers.
- New tests: a bundled TLS client (e.g. BearSSL-style or musl-based)
  completes a handshake with a local TLS server through guest
  sockets; failure paths match native.
- Differential: handshake transcripts and error codes match native.
- Completion: HTTPS GET against a local server works from a guest
  program using an unmodified TLS library.
- Known gaps: kernel implements no crypto itself, by design.
- Rollback: socket option table.

## Track ABI (priority 4)

### MS89 — syscall inventory and trivial wins
- Purpose: replace folklore with a list: every number classified as
  implemented / stub-or-partial / intentionally-ENOSYS, with the
  reason recorded in the compat ledger.
- Implement: publish the matrix; implement the cheap high-frequency
  wins (`settimeofday`, `readahead`, `syncfs`, `vhangup`,
  `name_to_handle_at`/`open_by_handle_at` if the VFS side is small).
- Depends on: nothing new.
- New tests: one differential marker per newly covered call with
  native-agreed errors.
- Differential: per-call byte/errno comparison, as usual.
- Completion: zero unclassified syscalls in the ledger; no
  previously-passing test regresses.
- Known gaps: the hard calls move to later milestones.
- Rollback: additive; revert removes new arms.

### MS90 — SCM_RIGHTS and AF_UNIX SEQPACKET
- Purpose: the IPC primitives real daemons need.
- Implement: ancillary data over Unix sockets (`SCM_RIGHTS` fd
  passing with refcount-correct table handoff), `SOCK_SEQPACKET`
  (connected, message boundaries, no unconnected sendto).
- Depends on: MS78 socket table.
- New tests: fd passing round trip (send a pipe, use it across the
  socket), seqpacket boundary/connection semantics.
- Differential: native vs guest, byte and fd behavior identical.
- Completion: a forked pair passes fds and datagrams like Linux.
- Known gaps: `SCM_CREDENTIALS` stays stubbed unless free.
- Rollback: socket files.

### MS91 — namespaces/cgroups/bpf triage plus minimal set
- Purpose: decide, per interface, implement vs stub vs refuse, with
  reasons, instead of drifting.
- Implement: the triage matrix; then the cheapest load-bearing
  subset (likely: more `unshare`/`setns` coverage already present,
  pidfd completion, mount-namespace propagation fixes as needed by
  container runtimes).
- Depends on: MS89 matrix.
- New tests: flag-matrix tests comparing success/errno vs native.
- Completion: matrix complete; each newly covered flag behaves like
  native.
- Known gaps: full cgroup controllers and bpf remain future work,
  explicitly listed.
- Rollback: per-call arms.

## Track ext4 (priority 5)

### MS92 — ext4 read-only mount
- Purpose: read Linux-created filesystems without risking them.
- Implement: ext4 superblock/inode/dir parsing in a new `ext4.rs`,
  read-only open/read/readdir/getdents/stat, feature-flag gating:
  unknown incompat features force mount failure or read-only
  fallback, never read-write.
- Depends on: VFS mount routing, block layer.
- New tests: mount images created by native `mke2fs` (several
  feature sets), read/compare every file, then host `fsck` clean.
- Differential: guest reads vs native reads, byte identical.
- Completion: a native-written ext4 image mounts read-only and
  reads correctly; hostile feature sets are refused read-write.
- Known gaps: no writes at all.
- Rollback: one new file plus VFS route entries.

### MS93 — ext4 read-write without a journal
- Purpose: create, write, rename, link, unlink on ext4.
- Implement: block/inode allocators, extent write path, directory
  grow/split, Orlov-style placement simplified for small images.
- Depends on: MS92.
- New tests: the ext2 battery ported to ext4 (same markers, new
  mount), plus host re-mount and byte comparison.
- Differential: guest-written image re-mounted natively reads back
  identical bytes.
- Completion: read/write workloads match native; `fsck` clean.
- Known gaps: no journal yet (crash consistency explicitly out).
- Rollback: ext4 write path files.

### MS94 — large files, large dirs, 64-bit, checksums
- Purpose: remove the training wheels: big files and big dirs.
- Implement: 64-bit block numbers, huge_file extents, htree
  directory reads, metadata checksum verification on read and
  update on write.
- Depends on: MS93.
- New tests: multi-GB sparse images (backed by a larger test disk),
  million-entry directory listings vs native.
- Completion: large objects match native; corrupt-block injection
  is detected, not misread.
- Known gaps: journal still missing.
- Rollback: extent/dir/checksum files.

### MS95 — ordered journaling, recovery, durable rename/fsync
- Purpose: crash-safe ext4.
- Implement: jbd2 subset (commit blocks, revoke, checkpoint),
  recovery scan at mount, ordered data mode, `fsync`/`fdatasync`
  durability ordering, rename atomicity vs the journal.
- Depends on: MS93+MS94.
- New tests: kill-QEMU-mid-write then recover; power-loss script
  (host kills the VM at random points); post-recovery `fsck` clean
  and data-or-nothing semantics.
- Differential: recovered content matches one of the legal native
  crash outcomes, never garbage.
- Completion: the crash battery passes repeatedly; rename is
  durable.
- Known gaps: performance tuning explicitly out of scope.
- Rollback: journal files.

## Track drivers (priority 6)

### MS96 — driver model split plus PCI hardening
- Purpose: stop adding one-off drivers; common bus/device/driver
  separation with shared IRQ, DMA, MMIO and error recovery.
- Implement: `bus`/`device`/`driver` traits and registry, port the
  virtio drivers onto it, harden PCI/PCIe enumeration (bridges,
  BAR sizing, MSI-X bookkeeping).
- Depends on: existing virtio drivers (behavior must not change).
- New tests: identical device behavior before/after (existing
  batteries are the gate), plus a PCI topology dump vs QEMU config.
- Completion: zero behavioral delta; new drivers only need the
  registry from here on.
- Known gaps: no new hardware yet.
- Rollback: driver model files.

### MS97 — NVMe plus AHCI/SATA plus GPT/MBR
- Purpose: boot and store on real PC hardware paths.
- Implement: NVMe queues over the new model, AHCI HBA/port state
  machines, GPT/MBR partition parsing as block topology.
- Depends on: MS96.
- New tests: read/write/partition batteries on emulated NVMe and
  AHCI disks, `fsck` on filesystems created there.
- Differential: guest block I/O vs native `dd`/`fio` patterns.
- Completion: ext2/ext4 tested on NVMe and AHCI images, not just
  virtio-blk.
- Known gaps: no USB storage yet.
- Rollback: per-driver files.

### MS98 — USB xHCI plus HID keyboard/mouse (and PS/2)
- Purpose: real input devices.
- Implement: xHCI rings and event processing behind the driver
  model, HID report parsing, PS/2 controller as a fallback path.
- Depends on: MS96.
- New tests: scripted key/mouse event injection in QEMU compared
  against native evdev streams.
- Completion: guest console reacts to injected input like native.
- Known gaps: USB mass storage stays future work.
- Rollback: USB/HID files.

### MS99 — Ethernet (e1000/rtl8139 class) plus framebuffer/GOP
- Purpose: non-virtio NICs and a real display path.
- Implement: one Intel-style and one Realtek-style Ethernet driver
  on the new model, UEFI GOP/framebuffer console.
- Depends on: MS96, MS86-MS88 network stack.
- New tests: DHCP+TCP over the new NICs in QEMU, pixel-exact boot
  splash comparison.
- Completion: external networking works without virtio-net too.
- Known gaps: Wi-Fi and audio stay future work with a survey note.
- Rollback: per-driver files.

## Sequencing notes

- Tracks B and C need quiet hosts for timing-sensitive gates;
  schedule their QEMU runs when load is low or raise the harness
  timeout with justification recorded in the milestone doc.
- Track D is largely independent and can interleave with any track.
- Track E item MS92 (ext4 read-only) only needs VFS routing and
  can start as soon as Track A is free; MS93+ need MS92.
- No milestone may break the existing gates. A red gate stops the
  line until it is green again, by fix-forward milestone or revert.
