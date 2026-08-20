# Push fixtures

Frozen command output, source, protected-span, compression, prep, spill, &
restore cases for `membrane cli push` belong here. Cover the `runc` → `skel` →
`compress` → `truncate` lineage. Pull supplies selection/headroom; every lossy
case includes an exact recovery expectation & byte/token accounting.
