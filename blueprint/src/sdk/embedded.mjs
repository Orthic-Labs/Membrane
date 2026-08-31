// D32: SDK embedded read-only client — calls the application service
// directly without a daemon (read-only; writes require daemon ownership).

import { createBlueprintApplicationService } from "../lib/application/service.mjs";

export class EmbeddedBlueprintClient {
  constructor({ rootRegistry = null, allowEmbeddedRoot = false, outDir = ".agent" } = {}) {
    this.service = createBlueprintApplicationService({ rootRegistry, allowEmbeddedRoot, outDir });
  }

  async status(input = {}) { return this.service.status(input); }
  async search(input = {}) { return this.service.search(input); }
  async resolve(input = {}) { return this.service.resolve(input); }
  async recall(input = {}) { return this.service.recall(input); }
  async expand(input = {}) { return this.service.expand(input); }
  async impact(input = {}) { return this.service.impact(input); }
  async path(input = {}) { return this.service.path(input); }
  async architecture(input = {}) { return this.service.architecture(input); }
  async documentTruth(input = {}) { return this.service.documentTruth(input); }
  async federate(input = {}) { return this.service.federate(input); }

  close() { return Promise.resolve(); }
}
