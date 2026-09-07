// D32: stable direct Blueprint client. Hub-hosted IPC is primary; when Hub is
// absent, explicit direct calls use bounded in-process one-shot service calls.

import { DaemonClient } from "../service/client.mjs";
import { validateContractResult } from "../lib/contracts/validate.mjs";
import { createBlueprintApplicationService } from "../lib/application/service.mjs";
import { RootRegistry } from "../lib/application/root-registry.mjs";
import { BlueprintError } from "../lib/application/errors.mjs";
import { readWatchConfig } from "../../watchman/supervisor.mjs";

const TRANSPORT_CODES = new Set(["connect_timeout", "socket_closed", "ECONNREFUSED", "ENOENT", "EPIPE", "ERROR_FILE_NOT_FOUND", "ERROR_PIPE_BUSY"]);

export class BlueprintClient {
  constructor({ endpoint = null, contract = null, allowOneShot = true, outDir = ".agent", rootRegistry = null } = {}) {
    this.client = new DaemonClient({ endpoint });
    this.contract = contract;
    this.oneShot = allowOneShot ? createBlueprintApplicationService({
      rootRegistry: rootRegistry ?? new RootRegistry(readWatchConfig().repos),
      allowEmbeddedRoot: false,
      outDir,
      freshnessOwnership: "one_shot",
    }) : null;
  }

  async #call(method, input) {
    let response;
    try {
      response = await this.client.request({ method, input });
    } catch (error) {
      if (!this.oneShot || !TRANSPORT_CODES.has(String(error?.code ?? ""))) throw error;
      return this.#validated(await this.oneShot[method](input));
    }
    if (!response.ok) {
      // Rebuild the canonical typed error rather than a bare Error: constructing
      // a BlueprintError re-derives `retryable` and `remediation` from the code,
      // so a caller reached over Hub IPC sees exactly the taxonomy an in-process
      // caller sees (BPT-044). Values the server did send win over the derived
      // ones, so a future server carrying richer metadata is not flattened here.
      const code = response.error?.code ?? "internal_error";
      const error = new BlueprintError(
        code,
        response.error?.message ?? "blueprint request failed",
        response.error?.details ?? {},
      );
      if (response.error?.retryable !== undefined) error.retryable = response.error.retryable;
      if (response.error?.remediation !== undefined) error.remediation = response.error.remediation;
      throw error;
    }
    return this.#validated(response.result);
  }

  #validated(value) {
    if (this.contract) {
      const result = validateContractResult(this.contract, value);
      if (!result.ok) {
        const error = new Error(result.error.error.message);
        error.code = "contract_invalid";
        throw error;
      }
      return result.value;
    }
    return value;
  }

  async status(input = {}) { return this.#call("status", input); }
  async search(input = {}) { return this.#call("search", input); }
  async resolve(input = {}) { return this.#call("resolve", input); }
  async recall(input = {}) { return this.#call("recall", input); }
  async expand(input = {}) { return this.#call("expand", input); }
  async impact(input = {}) { return this.#call("impact", input); }
  async path(input = {}) { return this.#call("path", input); }
  async architecture(input = {}) { return this.#call("architecture", input); }
  async documentTruth(input = {}) { return this.#call("documentTruth", input); }
  async federate(input = {}) { return this.#call("federate", input); }

  close() { return this.client.close(); }
}
