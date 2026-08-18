// MBR-402 limits derived from engine/crates/membrane-runtime/src/code_batch.rs
// Single source of truth is code_batch.rs: MAX_ITEMS, MAX_BYTES, MAX_TOKENS, MAX_DEADLINE_MS
export const CODE_BATCH_MAX_ITEMS = 50;
export const CODE_BATCH_MAX_BYTES = 1024 * 1024;
export const CODE_BATCH_MAX_TOKENS = 50_000;
export const CODE_BATCH_MAX_DEADLINE_MS = 5_000;
