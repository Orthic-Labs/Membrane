# Source-coupled migration

Run `membrane migrate-legacy --legacy-root <workspace> --target-root <new-root>` after stopping its legacy supervisor. The command recognizes `tools/.cache/memory` and `.claude/crypt`, moves each whole layout by rename, preserves DB, grants, events, and aliases byte-for-byte, and prints a reversible receipt. It refuses an existing target or any legacy PID marker, so it neither copies state nor starts a second daemon.
