import Ajv2020 from "ajv/dist/2020.js";

export function validateJsonSchema(schema, payload, ref = null) {
  const validator = new Ajv2020({ allErrors: true, strict: false, validateFormats: false });
  const target = ref
    ? { $schema: schema.$schema, $ref: ref, $defs: schema.$defs }
    : schema;
  const valid = validator.validate(target, payload);
  return { valid, errors: validator.errors ?? [] };
}
