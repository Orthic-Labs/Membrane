// Thin wrapper — exact-first resolution owner is src/graph/resolution/index.mjs.
// This module preserves public signatures for existing callers (grep callers
// first) and re-exports the owner. Do not add duplicate scanned-file
// resolution logic here; all candidate ladder and fileSet matching lives in
// the resolution owner.

export {
  isRelativeSpecifier,
  normalizeRepoPath,
  candidatePaths,
  resolveSpecifier,
  resolutionUnsupportedOmission,
  classifyResolution,
  RESOLUTION_OMISSION_CODE,
  SUPPORTED_RESOLUTION_EXTENSIONS,
} from "../../graph/resolution/index.mjs";
