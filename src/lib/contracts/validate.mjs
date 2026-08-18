import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";
import { CONTRACT_CATALOG, contractByName } from "./catalog.mjs";

const SCHEMA_DIR = fileURLToPath(new URL("../../../schemas/", import.meta.url));
const ajv = new Ajv2020({
  allErrors: true,
  strict: true,
  allowUnionTypes: true,
  validateFormats: false,
});
const validators = new Map();

for (const entry of CONTRACT_CATALOG) {
  const schema = JSON.parse(readFileSync(new URL(entry.file, new URL("../../../schemas/", import.meta.url)), "utf8"));
  const validator = ajv.compile(schema);
  validators.set(entry.name, { entry, schema, validator });
}

export function contractError(schemaId, validationErrors = []) {
  const first = validationErrors[0] ?? {};
  return {
    schemaVersion: 1,
    error: {
      code: "contract_invalid",
      schemaId,
      pointer: first.instancePath ?? "",
      keyword: first.keyword ?? null,
      message: String(first.message ?? "contract validation failed"),
      errors: validationErrors.map((item) => ({
        pointer: item.instancePath ?? "",
        keyword: item.keyword ?? null,
        message: String(item.message ?? "validation failed"),
        params: item.params ?? {},
      })),
    },
  };
}

export function validateContract(name, value) {
  const entry = contractByName(name);
  const record = validators.get(entry.name);
  if (!record.validator(value)) {
    const envelope = contractError(record.schema.$id ?? entry.name, record.validator.errors ?? []);
    throw Object.assign(new Error(envelope.error.message), {
      code: envelope.error.code,
      details: envelope.error,
    });
  }
  return value;
}

export function validateContractResult(name, value) {
  try {
    return { ok: true, value: validateContract(name, value), error: null };
  } catch (error) {
    return {
      ok: false,
      value: null,
      error: {
        schemaVersion: 1,
        error: {
          code: error.code ?? "contract_invalid",
          message: String(error.message ?? error),
          ...(error.details ?? {}),
        },
      },
    };
  }
}

export function schemaDirectory() {
  return SCHEMA_DIR;
}
