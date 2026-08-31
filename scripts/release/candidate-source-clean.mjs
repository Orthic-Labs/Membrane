function paths(output, separator) {
  return output.split(separator).filter(Boolean);
}

export function assertCandidateSourceClean({ git, allowGeneratedSchemaOutput = false }) {
  // Tauri rewrites its manifest even where serialised bytes are unchanged.
  // Porcelain can retain a stat-cache `M` on Windows in that case, so prove
  // semantic cleanliness from Git's content comparison instead.
  const tracked = paths(git(["diff", "--name-only", "HEAD", "--"]), /\r?\n/);
  const untracked = paths(git(["ls-files", "--others", "--exclude-standard", "-z"]), "\0");
  const changed = [...tracked, ...untracked];
  const unexpected = changed.filter((path) => !allowGeneratedSchemaOutput || !path.startsWith("apps/membrane-hub/src-tauri/gen/"));
  if (unexpected.length) throw new Error(`candidate source must be clean: ${unexpected.join(", ")}`);
}
