// Node entry of `@vutex/watex-fonts`.
//
// A browser bundle resolves `browser.js` instead, so the bundler never sees
// `node:fs`. Node hosts read the two math faces from the installed package
// rather than fetching them from the CDN.

import { readFile } from "node:fs/promises";

const fontDir = new URL("./assets/fonts/", import.meta.url);

async function readFont(name) {
    return new Uint8Array(await readFile(new URL(name, fontDir)));
}

/** Read both math faces shipped beside this module. */
export async function loadMathFonts() {
    const [latinModern, xits] = await Promise.all([
        readFont("LatinModernMath-Regular.otf"),
        readFont("XITSMath-Regular.otf"),
    ]);
    return { latinModern, xits };
}
