// Bundle the same field renderers as the platform. Runtime needs only Node.
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const here=path.dirname(fileURLToPath(import.meta.url));
const web=path.resolve(here,'../../web');
const require=createRequire(path.join(web,'package.json'));
require('esbuild').buildSync({stdin:{contents:`export {paymentFormHtml} from './paymentFormFields';export {rentApplicationHtml} from './rentalFormFields';export {rentPreviewFields,rentCalculationDescription} from './rentPreviewFields';export {rentDocumentChecklistHtml} from './rentDocuments';`,resolveDir:path.join(web,'src/admin/domain-logic/components/energyOps'),loader:'ts'},bundle:true,platform:'node',format:'esm',outfile:path.join(here,'form-fields.generated.mjs')});
