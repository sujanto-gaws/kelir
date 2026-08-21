// JFSS S3.3 rule 2: a generated operator is server-side only — the client must
// not evaluate it. Registering it on the Rust engine and not on the WASM engine
// is exactly that, enforced by the engine rather than by convention.
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { Engine } = require('@goplasmatic/datalogic-wasm');

const client = new Engine({});
const expr = JSON.stringify({ generateInvoiceId: ['invoice_default'] });
try {
  console.log('client evaluated it:', client.evalStr(expr, '{}'));
} catch (error) {
  console.log('client refused it:  ', String(error.message || error).split('\n')[0]);
}
