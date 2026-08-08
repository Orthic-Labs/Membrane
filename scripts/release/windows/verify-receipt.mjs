import { readJson, validateReceipt } from "./contract.mjs";

const [manifestPath, receiptPath] = process.argv.slice(2);
if (!manifestPath || !receiptPath) { console.error("usage: node verify-receipt.mjs <manifest.json> <receipt.json>"); process.exit(2); }
try { validateReceipt(await readJson(receiptPath), await readJson(manifestPath)); console.log("PASS windows installer receipt"); }
catch (error) { console.error(`FAIL CLOSED: ${error.message}`); process.exit(1); }
