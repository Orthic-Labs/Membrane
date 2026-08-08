import { readJson } from "./contract.mjs";
import { signingPlan } from "./contract.mjs";

const [manifestPath, installerPath] = process.argv.slice(2);
if (!manifestPath || !installerPath) { console.error("usage: node prepare-signing.mjs <manifest.json> <Membrane_*_x64-setup.exe>"); process.exit(2); }
try { console.log(JSON.stringify(signingPlan(await readJson(manifestPath), installerPath), null, 2)); }
catch (error) { console.error(`FAIL CLOSED: ${error.message}`); process.exit(1); }
