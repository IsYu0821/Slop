# Milestone 68: the virtio 1.0 notification path and a TCP stack that closes

## Goal

Make the hardware drivers work with the device models a Linux user expects
and make the network stack finish what it starts. The acceptance gate covers
both: the guest moves real packets through the network driver and the
connection has to be closed when the program closes the socket.

The milestone found four defects that had been hiding behind each other. The
first one made every transmit fail, the second one deadlocked the guest as
soon as a transmit was outstanding, the third one made the connection never
close, and the fourth one was a leftover debug print that deadlocked the
guest on its own.

## The notification capability was read with the wrong offsets

The virtio 1.0 interface describes itself through vendor specific PCI
capabilities. The common configuration and the device configuration share one
layout: the capability type, the bar and the offset of the structure inside
that bar. The notification capability has a different one: the offset
multiplier, the offset of the notification area inside its bar and the bar
number.

The driver read the notification capability as if it were a configuration
capability, so the multiplier was the bar number, the offset was the padding
after the capability and the bar was the low byte of the offset. The
notification register of a queue was then computed from wrong values, the
device was never told that work was waiting, and no transmit ever completed.
The queue index is now written to the area base plus the offset plus the
multiplier times the queue index, which is where the device reads it.

## The feature negotiation accepted features that are not implemented

The negotiation acknowledged every feature the device offered. A virtio-net
device offers a merged receive buffer, a control queue and a queue pair, and a
virtio-blk device offers a flush, a discard, a write of zeroes and a secure
erase. Accepting a feature means the device may use a path the driver does
not serve.

The driver now accepts the version 1 feature and nothing else, which is the
minimum a modern device needs.

The queue size of a modern device is also read back after the driver offers
it, so the ring layout follows the size the device really uses.

## The network driver held its lock across the transmit wait

The driver state sat behind a plain spin lock, and the transmit path held
that lock while it waited for the device to consume the buffer. The timer
interrupt polls the network, and it runs on the same processor, so an
interrupt that arrived during the wait took the same lock and spun for ever:
the guest stopped.

The driver state now sits behind an interrupt safe lock, and the transmit
path releases the lock while it waits.

## The TCP send window never moved

The stack removed the acknowledged bytes from the send buffer but never moved
the base of the window forward. The number of bytes in flight therefore never
shrank, the same bytes were retransmitted for ever, and the condition that
sends the close request was never met, so a socket close spun until the test
gave up.

The acknowledged base now moves forward with the acknowledgement, the close
request is sent, and the connection reaches its final state.

## The user address space gave every region its own page table slot

The program break, the anonymous mappings, the dynamic linker and the
position independent image were placed at addresses that are a multiple of
64 gigabytes apart, so they shared the same page table slot and, with it, the
same lower level indices. A mapping in one of the regions overwrote the page
table entry of another.

Every region now lives alone in a page table slot:

| region | address | page table slot |
| --- | --- | --- |
| the dynamic linker | `0x4000_0000_0000` | 128 |
| the position independent image | `0x4080_0000_0000` | 129 |
| the anonymous mappings | `0x4100_0000_0000` | 130 |
| the program break | `0x4180_0000_0000` | 131 |

The program break also has an upper limit, so a runaway break cannot reach
the region above it.

## The vector registers survive an exception and an interrupt

A faulting instruction is retried after the handler returns, so the vector
register file has to survive the call into the kernel. An interrupt can
preempt user code and return to it, and it can preempt kernel code in the
middle of a vector computation. Both paths now save and restore the sixteen
vector registers.

## A blocked fatal signal does not end the process that sent it

A fatal signal that the caller sends to itself ends the caller before the
syscall returns, but only when the signal is not blocked. A process that
blocks a signal and then raises it keeps running, which is what Linux does.

## Acceptance criteria

1. `./tools/acceptance-test.sh` prints `acceptance: PASS`, including the
   UDP and TCP echo test that moves real packets through the network driver.
2. `./tools/busybox-test.sh` prints `busybox compat: PASS (115 checks match
   Linux)`.
3. `./tools/heap-test.sh` prints `heap: PASS`.
4. `./tools/check-source.sh` passes.
5. `cargo +nightly build -Zbuild-std=core --release` is clean.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/busybox-test.sh` PASS, 115 checks match Linux.
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

## Open items

- The differential battery stops at the processor time accounting. The
  reason is the missing vector system call entry point: on Linux the clock
  is read without entering the kernel, so a loop that measures the clock
  spends its time in user mode, while this kernel charges the time to the
  kernel. The measurement is recorded in `docs/priority-status.md`.
- The processor time accounting still counts one tick as a millisecond,
  while the local APIC timer of the test machine runs at a different rate.
  The tick is not calibrated against a real time source yet.
