# Milestone 49: io_uring basic engine

Added ring allocation in `io_uring_setup`, parameter reporting, and synchronous
processing of NOP, read and write submission queue entries with completion queue
entries.

Acceptance suite remains green.
