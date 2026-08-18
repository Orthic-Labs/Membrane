// D32: SDK daemon client — typed access to the resident daemon over IPC.

import { DaemonClient } from "../service/client.mjs";
import { validateContractResult } from "../lib/contracts/validate.mjs";

export class CortexClient {
  constructor({ endpoint = null, contract = null } = {}) {
    this.client = new DaemonClient({ endpoint });
    this.contract = contract;
  }

  async #call(method, input) {
    const response = await this.client.request({ method, input });
    if (!response.ok) {
      const error = new Error(response.error?.message ?? "cortex request failed");
      error.code = response.error?.code ?? "internal_error";
      throw error;
    }
    if (this.contract) {
      const result = validateContractResult(this.contract, response.result);
      if (!result.ok) {
        const error = new Error(result.error.error.message);
        error.code = "contract_invalid";
        throw error;
      }
      return result.value;
    }
    return response.result;
  }

  async status(input = {}) { return this.#call("status", input); }
  async search(input = {}) { return this.#call("search", input); }
  async resolve(input = {}) { return this.#call("resolve", input); }
  async orient(input = {}) { return this.#call("orient", input); }
  async expand(input = {}) { return this.#call("expand", input); }
  async impact(input = {}) { return this.#call("impact", input); }
  async architecture(input = {}) { return this.#call("architecture", input); }
  async documentTruth(input = {}) { return this.#call("documentTruth", input); }

  close() { return this.client.close(); }
}
