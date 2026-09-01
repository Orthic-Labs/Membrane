import catalog from "../../../schemas/catalog.json" with { type: "json" };

export const CONTRACT_CATALOG = Object.freeze(catalog.contracts.map((entry) => Object.freeze({ ...entry })));

export function contractByName(name) {
  const found = CONTRACT_CATALOG.find((entry) => entry.name === String(name));
  if (!found) {
    throw Object.assign(new Error(`unknown contract: ${name}`), {
      code: "contract_unknown",
      details: { name: String(name) },
    });
  }
  return found;
}
