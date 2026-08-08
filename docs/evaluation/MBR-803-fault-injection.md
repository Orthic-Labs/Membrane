# MBR-803 fault injection

`tests/fault-injection/fault-matrix.v1.json` is closed at seven scenarios. `fault-runner.mjs` emits content-free `membrane-fault-receipt` records with stable codes, bounded deadline, input digest, observed non-health, recovery, cleanup, and deterministic clock injection.

Run `node --test tests/fault-injection/fault-runner.test.mjs` for contract coverage. Generate an evidence bundle with `node scripts/qualification/run-fault-injection.mjs /tmp/membrane-fault-suite.json`. These are deterministic black-box canonical seams; they do not claim installed-path qualification. Production activation still requires loopback/supervisor adapters and archived receipts.
