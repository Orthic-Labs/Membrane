// D32: SDK embedded read-only client — calls the application service
// directly without a daemon (read-only; writes require daemon ownership).

import { createBlueprintApplicationService } from "../lib/application/service.mjs";

export class EmbeddedBlueprintClient {
  constructor({ rootRegistry = null, allowEmbeddedRoot = true, outDir = ".agent" } = {}) {
    this.service = createBlueprintApplicationService({ rootRegistry, allowEmbeddedRoot, outDir });
  }

  async status(input = {}) { return this.service.status(input); }
  async search(input = {}) { return this.service.search(input); }
  async resolve(input = {}) { return this.service.resolve(input); }
  async orient(input = {}) { return this.service.orient(input); }
  async expand(input = {}) { return this.service.expand(input); }
  async impact(input = {}) { return this.service.impact(input); }
  async architecture(input = {}) { return this.service.architecture(input); }
  async documentTruth(input = {}) { return this.service.documentTruth(input); }

  close() { return Promise.resolve(); }
}
