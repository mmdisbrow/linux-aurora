# SEP host-file publication fault tests

The atomic suite compiles the production C replacement primitive, Rust StoreFile wrapper and catacomb writer against a synthetic VFS. It checks short writes, allocation/open/security/rename/fsync failures, preservation of the old generation before rename, and a complete visible generation after a failed directory fsync. The C boundary tests reject symlinks and non-regular destinations, exclusive-publication races, and unsafe cleanup. The suite also compiles a deliberate regression and requires its named behavioral assertion to fail. No real files, SEP state or hardware are opened.

Run `python3 -I tools/aurora-sep/tests/persistence/persistence-tests.py --suite atomic --output /tmp/sep-atomic`. CI pins Rust 1.85.0. These host fixtures do not establish power-loss durability on physical storage or SEP anti-replay safety.

The store suite compiles the real host-store implementation and verifies atomic header publication, old-handle replacement after uncertain directory sync, and reopen/retry behavior. Run the same command with `--suite store` and a different output directory.

The identity suite compiles the actual enrollment completion and reconciliation paths. It checks that Done follows a durable index write, cancellation/close during that write prevents late completion, and failed commits or partial reconciliation retain a dirty marker for retry. The index-format suite also rejects zero/duplicate UUIDs in present records; exact size/count checks already come from #234.

The keybag suite compiles the real record writer/parser and checks intent, refusal and committed snapshot replacement under VFS faults. It preserves #234's canonical refusal validation without another reader implementation. Temporary secret-bearing buffers are wiped on drop.
